//! Backend-authoritative idle continuation loop for a managed Session.
//!
//! A completed Turn opens an idle window. Once the timer's grace has elapsed,
//! the timer text is appended to the ordinary durable PromptQueue and is
//! therefore indistinguishable from the user's next follow-up at the Harness
//! boundary — except that the queue schedules it behind the user's own drafts
//! and pending letters.
//!
//! The cadence is obligation-aware. Sending with expects_reply (a letter, or
//! a Room @) is an implicit "I am waiting" declaration. While anything I sent
//! is unanswered and nothing new has arrived, the timer does NOT keep poking:
//! it stays quiet, surfaces ONE inspection poke at the half-hour mark (so the
//! Session can check whether the peer is alive and re-plan if not), and then
//! parks until real news — an inbound letter or @, a resolved obligation, or
//! a user edit — revives it. Heuristics only ever stretch the interval — they
//! never decide whether the user's continuation is allowed to exist.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use tokio::sync::{broadcast, mpsc};

use crate::acp::manager::ConnectionManager;
use crate::acp::types::{AcpEvent, ConnectionStatus, PromptInputBlock};
use crate::acp::InternalEventBus;
use crate::db::service::collaboration_service::OutboundAwaitingSummary;
use crate::db::service::{collaboration_service, prompt_queue_service, session_timer_service};
use crate::db::AppDatabase;
use crate::models::prompt_queue::{EnqueuePromptQueueItem, PromptQueueDraft, PromptQueueSource};
use crate::web::event_bridge::{
    emit_event, EventEmitter, SessionTimerChanged, PROMPT_QUEUE_CHANGED_EVENT,
    SESSION_TIMER_CHANGED_EVENT,
};

const SCAN_INTERVAL_SECS: u64 = 1;
/// While the Session is waiting on unanswered outbound work, the only
/// automatic poke is one inspection at this horizon: long enough that a
/// healthy peer has finished, short enough that a stuck one gets looked at.
const WAITING_CHECK_AFTER_SECS: i64 = 30 * 60;
/// Zero-information pokes allowed while waiting before the brake parks the
/// timer. One: the half-hour inspection, then silence until real news.
const MAX_ZERO_PROGRESS_POKES: i32 = 1;
pub const AUTO_PAUSE_REASON_WAITING: &str = "waiting_no_progress";

#[derive(Clone)]
pub struct SessionTimerHandle {
    wake_tx: mpsc::UnboundedSender<i32>,
}

impl SessionTimerHandle {
    pub fn wake(&self, conversation_id: i32) {
        let _ = self.wake_tx.send(conversation_id);
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub fn disconnected_for_test() -> Self {
        let (wake_tx, wake_rx) = mpsc::unbounded_channel();
        drop(wake_rx);
        Self { wake_tx }
    }
}

pub fn build_session_timer_runtime(
    db_conn: sea_orm::DatabaseConnection,
    manager: ConnectionManager,
    emitter: EventEmitter,
    bus: Arc<InternalEventBus>,
    prompt_queue: crate::prompt_queue::PromptQueueHandle,
) -> (
    SessionTimerHandle,
    impl Future<Output = ()> + Send + 'static,
) {
    let bus_rx = bus.subscribe();
    let (wake_tx, wake_rx) = mpsc::unbounded_channel();
    let handle = SessionTimerHandle { wake_tx };
    let runtime = SessionTimerRuntime {
        db: AppDatabase { conn: db_conn },
        manager,
        emitter,
        prompt_queue,
        idle_since: HashMap::new(),
        bus_rx,
        wake_rx,
    };
    (handle, async move { runtime.run().await })
}

struct SessionTimerRuntime {
    db: AppDatabase,
    manager: ConnectionManager,
    emitter: EventEmitter,
    prompt_queue: crate::prompt_queue::PromptQueueHandle,
    /// Absent means the process did not observe a trustworthy idle boundary.
    idle_since: HashMap<i32, DateTime<Utc>>,
    bus_rx: broadcast::Receiver<Arc<crate::acp::EventEnvelope>>,
    wake_rx: mpsc::UnboundedReceiver<i32>,
}

impl SessionTimerRuntime {
    async fn run(mut self) {
        self.seed_idle_boundaries().await;
        let mut scan = tokio::time::interval(StdDuration::from_secs(SCAN_INTERVAL_SECS));
        scan.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                wake = self.wake_rx.recv() => {
                    let Some(conversation_id) = wake else { break };
                    self.scan_conversation(conversation_id).await;
                }
                event = self.bus_rx.recv() => {
                    match event {
                        Ok(event) => self.on_acp_event(&event).await,
                        Err(broadcast::error::RecvError::Lagged(_)) => self.idle_since.clear(),
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = scan.tick() => self.fire_due(None).await,
            }
        }
    }

    /// A restart wipes this in-memory idle map while the timers themselves
    /// persist, so without a seed every enabled timer stays silent until its
    /// Session happens to complete another turn — which a backed-off Session
    /// waiting on replies may never do on its own. Treating "idle since
    /// process start" as the boundary is conservative: each fire still waits
    /// a full grace (or backoff interval) measured from startup, and every
    /// pre-fire guard (queued work yields, busy runtime skips, auto-pause
    /// holds) applies unchanged.
    async fn seed_idle_boundaries(&mut self) {
        let timers = match session_timer_service::enabled(&self.db.conn).await {
            Ok(timers) => timers,
            Err(error) => {
                tracing::warn!("[session-timer] idle-boundary seed failed: {error}");
                return;
            }
        };
        let now = Utc::now();
        for timer in timers {
            self.idle_since.entry(timer.conversation_id).or_insert(now);
        }
    }

    async fn on_acp_event(&mut self, event: &Arc<crate::acp::EventEnvelope>) {
        let conversation_id = match &event.payload {
            AcpEvent::TurnComplete { .. } | AcpEvent::UserMessage { .. } => self
                .manager
                .get_state(&event.connection_id)
                .await
                .and_then(|state| state.try_read().ok()?.conversation_id),
            _ => return,
        };
        let Some(conversation_id) = conversation_id else {
            return;
        };
        match event.payload {
            AcpEvent::TurnComplete { .. } => {
                self.idle_since.insert(conversation_id, Utc::now());
            }
            AcpEvent::UserMessage { .. } => {
                self.idle_since.remove(&conversation_id);
            }
            _ => {}
        }
    }

    async fn scan_conversation(&mut self, conversation_id: i32) {
        if conversation_id < 0 {
            self.fire_due(None).await;
        } else {
            self.fire_due(Some(conversation_id)).await;
        }
    }

    async fn fire_due(&mut self, only_conversation: Option<i32>) {
        let timers = match session_timer_service::enabled(&self.db.conn).await {
            Ok(timers) => timers,
            Err(error) => {
                tracing::error!("[session-timer] timer scan failed: {error}");
                return;
            }
        };
        let now = Utc::now();
        for timer in timers {
            if only_conversation.is_some_and(|id| id != timer.conversation_id) {
                continue;
            }
            let Some(idle_since) = self.idle_since.get(&timer.conversation_id).copied() else {
                continue;
            };
            let idle_for = now - idle_since;
            if idle_for < Duration::seconds(timer.idle_grace_secs) {
                continue;
            }

            // Host-observed facts that set the cadence: did the mailbox
            // produce anything new since the last poke, and is this Session
            // still waiting on unanswered outbound letters?
            let new_info = match collaboration_service::latest_mailbox_info_at(
                &self.db.conn,
                timer.conversation_id,
            )
            .await
            {
                Ok(latest) => match (latest, timer.last_fired_at) {
                    (Some(info), Some(fired)) => {
                        // Mailbox timestamps are normalized to second
                        // precision while the fire clock keeps sub-seconds;
                        // news landing within the fire's own second must
                        // still count as new (the cost is at most one extra
                        // normal-cadence poke).
                        use chrono::Timelike;
                        info >= fired.with_nanosecond(0).unwrap_or(fired)
                    }
                    (_, None) => true,
                    (None, Some(_)) => false,
                },
                Err(error) => {
                    tracing::warn!("[session-timer] mailbox info check failed: {error}");
                    true
                }
            };
            let waiting = match collaboration_service::outbound_awaiting_summary(
                &self.db.conn,
                timer.conversation_id,
            )
            .await
            {
                Ok(summary) => summary,
                Err(error) => {
                    tracing::warn!("[session-timer] obligation check failed: {error}");
                    OutboundAwaitingSummary::default()
                }
            };
            let strike = if new_info { 0 } else { timer.strike_count };
            let repeat_poke = !new_info && waiting.letter_count > 0;

            if timer.auto_paused_at.is_some() && !new_info {
                continue;
            }
            if repeat_poke && strike >= MAX_ZERO_PROGRESS_POKES {
                if timer.auto_paused_at.is_none() {
                    match session_timer_service::auto_pause(
                        &self.db.conn,
                        &timer.id,
                        timer.updated_at,
                        AUTO_PAUSE_REASON_WAITING,
                    )
                    .await
                    {
                        Ok(paused) => {
                            tracing::info!(
                                "[session-timer] parked {} after {} zero-information pokes",
                                paused.id,
                                strike
                            );
                            emit_event(
                                &self.emitter,
                                SESSION_TIMER_CHANGED_EVENT,
                                SessionTimerChanged {
                                    conversation_ids: vec![timer.conversation_id],
                                },
                            );
                        }
                        Err(error) => {
                            tracing::debug!("[session-timer] auto-pause raced: {error}")
                        }
                    }
                }
                continue;
            }

            let required_secs = if repeat_poke {
                timer.idle_grace_secs.max(WAITING_CHECK_AFTER_SECS)
            } else {
                timer.idle_grace_secs
            };
            if idle_for < Duration::seconds(required_secs) {
                continue;
            }

            // Anything already queued will continue the Session by itself; a
            // timer poke on top would spend a second turn on the same idle
            // window.
            match prompt_queue_service::has_queued_items(&self.db.conn, timer.conversation_id)
                .await
            {
                Ok(false) => {}
                Ok(true) => continue,
                Err(error) => {
                    tracing::warn!("[session-timer] queue check failed: {error}");
                    continue;
                }
            }

            // A live runtime must actually be idle. A missing runtime is
            // allowed through: the idle sweep reclaims a waiting Session's
            // process long before a backed-off poke becomes due, and the
            // queued continuation then rides the same dispatcher
            // start/resume path a letter takes.
            if let Some(false) = self.runtime_is_idle(timer.conversation_id).await {
                continue;
            }

            let next_strike = if repeat_poke { strike + 1 } else { 0 };
            let facts = continuation_facts(
                timer.fire_count.saturating_add(1),
                idle_for,
                &waiting,
                if repeat_poke { next_strike } else { 0 },
                now,
            );
            self.fire(timer, next_strike, facts).await;
        }
    }

    async fn runtime_is_idle(&self, conversation_id: i32) -> Option<bool> {
        let connection_id = self
            .manager
            .find_connection_by_conversation_id(conversation_id)
            .await?;
        let state = self.manager.get_state(&connection_id).await?;
        let guard = state.read().await;
        Some(guard.status == ConnectionStatus::Connected && !guard.turn_in_flight)
    }

    async fn fire(
        &mut self,
        timer: crate::models::session_timer::SessionTimerInfo,
        next_strike: i32,
        facts: Option<String>,
    ) {
        // Claim before enqueue so a concurrent pause or second backend cannot
        // create a prompt after losing the authoritative timer revision.
        let claimed = match session_timer_service::claim_fire(
            &self.db.conn,
            &timer.id,
            timer.updated_at,
            next_strike,
        )
        .await
        {
            Ok(timer) => timer,
            Err(error) => {
                tracing::debug!("[session-timer] skipped raced fire {}: {error}", timer.id);
                return;
            }
        };
        emit_event(
            &self.emitter,
            SESSION_TIMER_CHANGED_EVENT,
            SessionTimerChanged {
                conversation_ids: vec![claimed.conversation_id],
            },
        );

        // The user's configured text goes out verbatim; the host may only
        // append facts it observed itself.
        let mut prompt_text = claimed.prompt_text.clone();
        if let Some(facts) = facts {
            prompt_text.push_str(&facts);
        }
        let dedupe_id = session_timer_service::fire_dedupe_id(&claimed.id, claimed.fire_count);
        let snapshot = match prompt_queue_service::enqueue(
            &self.db.conn,
            EnqueuePromptQueueItem {
                conversation_id: claimed.conversation_id,
                id: dedupe_id.clone(),
                client_dedupe_id: dedupe_id,
                draft: PromptQueueDraft {
                    blocks: vec![PromptInputBlock::Text {
                        text: prompt_text.clone(),
                    }],
                    display_text: prompt_text,
                },
                mode_id: None,
                source: PromptQueueSource::Timer,
            },
        )
        .await
        {
            Ok(snapshot) => snapshot,
            Err(error) => {
                // Leave the idle window open. The next scan creates a new
                // occurrence and retries without starting a hidden Turn.
                tracing::error!(
                    "[session-timer] queue enqueue failed for {}: {error}",
                    claimed.id
                );
                return;
            }
        };

        self.idle_since.remove(&claimed.conversation_id);
        emit_event(&self.emitter, PROMPT_QUEUE_CHANGED_EVENT, snapshot);
        self.prompt_queue.wake(claimed.conversation_id);
        tracing::info!(
            "[session-timer] queued continuation {} occurrence {}",
            claimed.id,
            claimed.fire_count
        );
    }
}

/// Host-authored appendix for a continuation prompt: only facts the host
/// observed itself, never advice. `None` when there is nothing worth
/// reporting, so the user's configured text goes out exactly as written.
fn continuation_facts(
    occurrence: i32,
    idle_for: Duration,
    waiting: &OutboundAwaitingSummary,
    strike: i32,
    now: DateTime<Utc>,
) -> Option<String> {
    if strike == 0 && waiting.letter_count == 0 {
        return None;
    }
    let mut text = format!(
        "\n\n——\nCodeg 宿主观测（自动续跑第 {occurrence} 次）：距上一轮结束{}。",
        humanize_duration(idle_for)
    );
    if waiting.letter_count > 0 {
        match (
            waiting.peer_conversation_ids.first().copied(),
            waiting
                .oldest_since
                .map(|since| humanize_duration(now - since)),
        ) {
            (Some(peer), Some(oldest)) => text.push_str(&format!(
                "你派出的 {} 条消息仍未收到回复（最早的一条发往 Session #{peer}，已等待{oldest}）。",
                waiting.letter_count
            )),
            _ => text.push_str(&format!(
                "你派出的 {} 条消息仍未收到回复。",
                waiting.letter_count
            )),
        }
    }
    if strike > 0 {
        text.push_str(
            "这是一次等待巡查：如果协作者还在正常干活，可以继续等；如果卡住或出错了，换个办法（查一下对方状态、换人、或先推进别的）。本次之后将退避停靠，收到新消息（回复销账/来信/@）会自动恢复。",
        );
    }
    Some(text)
}

fn humanize_duration(duration: Duration) -> String {
    let secs = duration.num_seconds().max(0);
    if secs < 120 {
        format!("约 {secs} 秒")
    } else {
        format!("约 {} 分钟", secs / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::session_timer::CreateSessionTimerInput;
    use crate::models::{
        CollaborationDeliveryHint, CollaborationInvocationPolicy, CollaborationUrgency,
        SendCollaborationMessageInput,
    };
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    fn runtime(db: crate::db::AppDatabase) -> SessionTimerRuntime {
        SessionTimerRuntime {
            db,
            manager: ConnectionManager::new(),
            emitter: EventEmitter::Noop,
            prompt_queue: crate::prompt_queue::PromptQueueHandle::disconnected_for_test(),
            idle_since: HashMap::new(),
            bus_rx: broadcast::channel(16).1,
            wake_rx: mpsc::unbounded_channel().1,
        }
    }

    async fn setup() -> (crate::db::AppDatabase, i32, i32) {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/timer-engine-test").await;
        let conversation_id =
            seed_conversation(&db, folder_id, crate::models::AgentType::Codex).await;
        let peer_id =
            seed_conversation(&db, folder_id, crate::models::AgentType::ClaudeCode).await;
        (db, conversation_id, peer_id)
    }

    async fn create_timer(
        db: &crate::db::AppDatabase,
        conversation_id: i32,
        dedupe: &str,
    ) -> crate::models::session_timer::SessionTimerInfo {
        session_timer_service::create(
            &db.conn,
            CreateSessionTimerInput {
                conversation_id,
                prompt_text: "Read docs/current-task.md and continue".into(),
                idle_grace_secs: 1,
                client_dedupe_id: Some(dedupe.into()),
            },
        )
        .await
        .unwrap()
    }

    fn letter(
        source: i32,
        target: i32,
        dedupe: &str,
        expects_reply: bool,
    ) -> SendCollaborationMessageInput {
        SendCollaborationMessageInput {
            source_conversation_id: source,
            target_conversation_ids: vec![target],
            subject: "Delegated task".into(),
            body: "please handle this".into(),
            client_dedupe_id: dedupe.into(),
            invocation_policy: CollaborationInvocationPolicy::StoreOnly,
            delivery_hint: CollaborationDeliveryHint::Default,
            expects_reply,
            urgency: CollaborationUrgency::Normal,
            reply_to_event_id: None,
        }
    }

    async fn queue_text(db: &crate::db::AppDatabase, conversation_id: i32) -> Vec<String> {
        prompt_queue_service::snapshot(&db.conn, conversation_id)
            .await
            .expect("snapshot")
            .items
            .iter()
            .filter_map(|item| item.draft.as_ref().map(|draft| draft.display_text.clone()))
            .collect()
    }

    async fn backdate_last_fire(db: &crate::db::AppDatabase, timer_id: &str, strike: i32) {
        db.conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE conversation_timer \
                 SET last_fired_at = ?, strike_count = ?, fire_count = fire_count + 1 \
                 WHERE id = ?",
                vec![Utc::now().into(), strike.into(), timer_id.into()],
            ))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn continuation_fire_is_a_normal_prompt_and_timer_rearms() {
        let (db, conversation_id, _) = setup().await;
        let timer = create_timer(&db, conversation_id, "idle-loop").await;
        let mut runtime = runtime(crate::db::AppDatabase {
            conn: db.conn.clone(),
        });
        runtime
            .idle_since
            .insert(conversation_id, Utc::now() - Duration::seconds(5));
        runtime.fire(timer, 0, None).await;

        assert_eq!(
            queue_text(&db, conversation_id).await,
            ["Read docs/current-task.md and continue"]
        );
        assert!(!runtime.idle_since.contains_key(&conversation_id));
        let timer = session_timer_service::list(&db.conn, conversation_id)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert!(timer.enabled);
        assert_eq!(timer.fire_count, 1);
    }

    #[tokio::test]
    async fn restart_seed_lets_an_enabled_timer_fire_without_a_turn() {
        // A restart wipes the in-memory idle map; nothing completes a turn
        // on its own while a Session waits on replies, so the seed must
        // stand in for the lost boundary or the continuation sleeps forever.
        let (db, conversation_id, _) = setup().await;
        create_timer(&db, conversation_id, "post-restart").await;
        let mut runtime = runtime(crate::db::AppDatabase {
            conn: db.conn.clone(),
        });

        // An already-observed boundary must survive the seed untouched.
        let observed = Utc::now() - Duration::seconds(120);
        runtime.idle_since.insert(-999, observed);
        runtime.seed_idle_boundaries().await;
        assert_eq!(runtime.idle_since.get(&-999), Some(&observed));

        let seeded = runtime
            .idle_since
            .get(&conversation_id)
            .copied()
            .expect("restart seeds every enabled timer's conversation");
        runtime
            .idle_since
            .insert(conversation_id, seeded - Duration::seconds(5));
        runtime.fire_due(None).await;
        assert_eq!(
            queue_text(&db, conversation_id).await,
            ["Read docs/current-task.md and continue"]
        );
    }

    #[tokio::test]
    async fn missing_runtime_enqueues_so_the_dispatcher_can_resume() {
        // The idle sweep reclaims a waiting Session's process after three
        // minutes; a backed-off continuation must still reach the queue so
        // the dispatcher can start or resume the Session, exactly like a
        // letter would.
        let (db, conversation_id, _) = setup().await;
        create_timer(&db, conversation_id, "resume-via-queue").await;
        let mut runtime = runtime(crate::db::AppDatabase {
            conn: db.conn.clone(),
        });
        runtime
            .idle_since
            .insert(conversation_id, Utc::now() - Duration::seconds(5));
        runtime.fire_due(None).await;
        assert_eq!(
            queue_text(&db, conversation_id).await,
            ["Read docs/current-task.md and continue"]
        );
    }

    #[tokio::test]
    async fn waiting_on_replies_gets_one_inspection_poke_at_the_half_hour() {
        let (db, conversation_id, peer_id) = setup().await;
        let timer = create_timer(&db, conversation_id, "waiting-backoff").await;
        crate::db::service::collaboration_service::send(
            &db.conn,
            letter(conversation_id, peer_id, "delegate-1", true),
        )
        .await
        .unwrap();
        // One continuation already went out and nothing new arrived since.
        backdate_last_fire(&db, &timer.id, 0).await;

        let mut runtime = runtime(crate::db::AppDatabase {
            conn: db.conn.clone(),
        });
        runtime
            .idle_since
            .insert(conversation_id, Utc::now() - Duration::seconds(301));
        runtime.fire_due(None).await;
        assert!(
            queue_text(&db, conversation_id).await.is_empty(),
            "waiting in silence: no poke before the half-hour inspection"
        );

        runtime
            .idle_since
            .insert(conversation_id, Utc::now() - Duration::seconds(1801));
        runtime.fire_due(None).await;
        let texts = queue_text(&db, conversation_id).await;
        assert_eq!(texts.len(), 1, "the single inspection poke goes out");
        assert!(texts[0].starts_with("Read docs/current-task.md and continue"));
        assert!(
            texts[0].contains("仍未收到回复"),
            "host-observed waiting facts ride along: {}",
            texts[0]
        );
        assert!(texts[0].contains("这是一次等待巡查"));
        assert!(texts[0].contains(&format!("#{peer_id}")));
        let timer = session_timer_service::list(&db.conn, conversation_id)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(timer.strike_count, 1, "the inspection is the only poke");
    }

    #[tokio::test]
    async fn brake_parks_the_timer_and_a_reply_revives_it() {
        let (db, conversation_id, peer_id) = setup().await;
        let timer = create_timer(&db, conversation_id, "brake-then-revive").await;
        let sent = crate::db::service::collaboration_service::send(
            &db.conn,
            letter(conversation_id, peer_id, "delegate-2", true),
        )
        .await
        .unwrap();
        backdate_last_fire(&db, &timer.id, MAX_ZERO_PROGRESS_POKES).await;

        let mut runtime = runtime(crate::db::AppDatabase {
            conn: db.conn.clone(),
        });
        runtime
            .idle_since
            .insert(conversation_id, Utc::now() - Duration::seconds(3600));
        runtime.fire_due(None).await;
        assert!(
            queue_text(&db, conversation_id).await.is_empty(),
            "the exhausted streak parks the timer instead of poking forever"
        );
        let parked = session_timer_service::list(&db.conn, conversation_id)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert!(parked.enabled, "auto-pause never flips the user's switch");
        assert!(parked.auto_paused_at.is_some());
        assert_eq!(
            parked.auto_pause_reason.as_deref(),
            Some(AUTO_PAUSE_REASON_WAITING)
        );

        // The peer finally answers: real news clears the brake on the next
        // scan and the continuation goes out at the ordinary grace again.
        let mut reply = letter(peer_id, conversation_id, "delegate-2-reply", false);
        reply.reply_to_event_id = Some(sent.event_id.clone());
        crate::db::service::collaboration_service::send(&db.conn, reply)
            .await
            .unwrap();
        runtime
            .idle_since
            .insert(conversation_id, Utc::now() - Duration::seconds(5));
        runtime.fire_due(None).await;
        let texts = queue_text(&db, conversation_id).await;
        assert_eq!(texts.len(), 1, "real news revives the parked timer");
        let revived = session_timer_service::list(&db.conn, conversation_id)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert!(revived.auto_paused_at.is_none());
        assert_eq!(revived.strike_count, 0);
    }

    #[tokio::test]
    async fn queued_work_defers_the_timer_instead_of_stacking_a_second_turn() {
        let (db, conversation_id, _) = setup().await;
        create_timer(&db, conversation_id, "defer-to-queue").await;
        prompt_queue_service::enqueue(
            &db.conn,
            EnqueuePromptQueueItem {
                conversation_id,
                id: "user-draft".into(),
                client_dedupe_id: "user-draft".into(),
                draft: PromptQueueDraft {
                    blocks: vec![PromptInputBlock::Text {
                        text: "user follow-up".into(),
                    }],
                    display_text: "user follow-up".into(),
                },
                mode_id: None,
                source: PromptQueueSource::User,
            },
        )
        .await
        .unwrap();
        let mut runtime = runtime(crate::db::AppDatabase {
            conn: db.conn.clone(),
        });
        runtime
            .idle_since
            .insert(conversation_id, Utc::now() - Duration::seconds(30));
        runtime.fire_due(None).await;
        assert_eq!(
            queue_text(&db, conversation_id).await,
            ["user follow-up"],
            "queued work already continues the Session; the timer stands down"
        );
    }
}
