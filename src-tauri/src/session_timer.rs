//! Backend-authoritative idle continuation loop for a managed Session.
//!
//! A completed Turn opens an idle window. Once the timer's current reminder
//! delay has elapsed, the timer text is appended to the ordinary durable
//! PromptQueue and is therefore indistinguishable from the user's next
//! follow-up at the Harness boundary — except that the queue schedules it
//! behind the user's own drafts and pending letters.
//!
//! The delay starts at the timer's `idle_grace` and doubles after each fire
//! that the Agent does not answer with `timer.reset_delay`, capping around
//! 30 minutes. Mail, Room posts, and `@` do not change this cadence; they
//! still wake the Session through the Dispatcher. Call `timer.reset_delay`
//! after real progress (or when new information unblocks the goal) so the
//! next reminder returns to the shortest interval. Skip it when still
//! waiting with nothing else to do.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use tokio::sync::{broadcast, mpsc};

use crate::acp::manager::ConnectionManager;
use crate::acp::types::{AcpEvent, ConnectionStatus, PromptInputBlock};
use crate::acp::InternalEventBus;
use crate::db::service::{prompt_queue_service, session_timer_service};
use crate::db::AppDatabase;
use crate::models::prompt_queue::{EnqueuePromptQueueItem, PromptQueueDraft, PromptQueueSource};
use crate::web::event_bridge::{
    emit_event, EventEmitter, SessionTimerChanged, PROMPT_QUEUE_CHANGED_EVENT,
    SESSION_TIMER_CHANGED_EVENT,
};

const SCAN_INTERVAL_SECS: u64 = 1;
/// Longest wait between idle reminders. The delay starts at `idle_grace`
/// and doubles after each unanswered fire until it hits this cap.
const MAX_REMINDER_DELAY_SECS: i64 = 30 * 60;

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
    /// Session happens to complete another turn. Treating "idle since
    /// process start" as the boundary is conservative: each fire still waits
    /// a full reminder delay measured from startup, and every pre-fire
    /// guard (queued work yields, busy runtime skips) applies unchanged.
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
            let required_secs = reminder_delay_secs(timer.idle_grace_secs, timer.strike_count);
            if idle_for < Duration::seconds(required_secs) {
                continue;
            }

            // Anything already queued will continue the Session by itself; a
            // timer poke on top would spend a second turn on the same idle
            // window.
            match prompt_queue_service::has_queued_items(&self.db.conn, timer.conversation_id).await
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

            let next_strike = timer.strike_count.saturating_add(1);
            let mut facts = continuation_hint(
                timer.fire_count.saturating_add(1),
                reminder_delay_secs(timer.idle_grace_secs, next_strike),
                timer.strike_count,
            )
            .unwrap_or_default();
            // Host-observed debt counts ride every fire: the Agent may have
            // compacted away the letters themselves, and the counts tell it
            // whether idling is really "waiting on others" or "debts unpaid".
            match crate::db::service::collaboration_service::open_obligation_digest(
                &self.db.conn,
                timer.conversation_id,
            )
            .await
            {
                Ok(digest) => facts.push_str(&obligation_debt_line(&digest)),
                Err(error) => {
                    tracing::warn!("[session-timer] obligation count failed: {error}")
                }
            }
            let facts = if facts.is_empty() { None } else { Some(facts) };
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
                task_id: None,
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

/// Host-authored appendix for a continuation prompt. The first reminder
/// stays the user's configured text. After the delay has grown, name the
/// tool that resets it.
fn continuation_hint(occurrence: i32, next_delay_secs: i64, current_strike: i32) -> Option<String> {
    if current_strike <= 0 {
        return None;
    }
    Some(format!(
        "\n\n——\nCodeg idle reminder #{occurrence}. Next reminder delay is about {}. Call timer.reset_delay if you made progress or can continue. Skip it if you are still waiting and have nothing else to do.",
        humanize_duration(Duration::seconds(next_delay_secs))
    ))
}

/// Host-observed debt counts appended to a continuation fire. Counts only,
/// never bodies; the titles themselves already ride ordinary turns via the
/// open-obligation tail note. Empty when the Session owes nothing, so a
/// debt-free fire keeps the user's configured text verbatim.
fn obligation_debt_line(
    digest: &crate::db::service::collaboration_service::OpenObligationDigest,
) -> String {
    let mut parts = Vec::new();
    if digest.letters_owed > 0 {
        parts.push(format!("{} 封会话信件", digest.letters_owed));
    }
    if digest.room_mentions_owed > 0 {
        parts.push(format!("{} 条群点名", digest.room_mentions_owed));
    }
    if parts.is_empty() {
        return String::new();
    }
    format!(
        "\n\n——\nCodeg 宿主事实：你欠回复 {}（仅计数，不含正文；用 list_inbox / read_room 查看并回复）。",
        parts.join("，")
    )
}

fn reminder_delay_secs(grace: i64, strike: i32) -> i64 {
    let shift = u32::try_from(strike.max(0)).unwrap_or(0).min(20);
    let multiplier = 1i64.checked_shl(shift).unwrap_or(i64::MAX);
    grace
        .saturating_mul(multiplier)
        .min(MAX_REMINDER_DELAY_SECS)
        .max(grace.max(1))
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
        let peer_id = seed_conversation(&db, folder_id, crate::models::AgentType::ClaudeCode).await;
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
    async fn unanswered_outbound_does_not_stretch_or_park_the_timer() {
        let (db, conversation_id, peer_id) = setup().await;
        create_timer(&db, conversation_id, "mail-is-not-cadence").await;
        crate::db::service::collaboration_service::send(
            &db.conn,
            letter(conversation_id, peer_id, "delegate-1", true),
        )
        .await
        .unwrap();

        let mut runtime = runtime(crate::db::AppDatabase {
            conn: db.conn.clone(),
        });
        runtime
            .idle_since
            .insert(conversation_id, Utc::now() - Duration::seconds(5));
        runtime.fire_due(None).await;
        let texts = queue_text(&db, conversation_id).await;
        assert_eq!(
            texts,
            ["Read docs/current-task.md and continue"],
            "unanswered mail must not delay or park the idle reminder"
        );
        let timer = session_timer_service::list(&db.conn, conversation_id)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert!(timer.auto_paused_at.is_none());
        assert_eq!(timer.strike_count, 1);
    }

    #[tokio::test]
    async fn fire_appends_open_debt_counts_to_the_continuation() {
        let (db, conversation_id, peer_id) = setup().await;
        create_timer(&db, conversation_id, "debt-facts").await;
        // An inbound letter the Session owes a reply: the fire must say so.
        crate::db::service::collaboration_service::send(
            &db.conn,
            letter(peer_id, conversation_id, "owed-1", true),
        )
        .await
        .unwrap();
        // Debt counts only cover embedded deliveries — a letter still in the
        // queue is not yet the Session's forgotten debt.
        sea_orm::ConnectionTrait::execute(
            &db.conn,
            sea_orm::Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Sqlite,
                "UPDATE collaboration_delivery \
                 SET state = 'embedded', embedded_turn_ref = 'turn-debt-1' \
                 WHERE target_conversation_id = ?",
                vec![conversation_id.into()],
            ),
        )
        .await
        .unwrap();
        // An outbound await is not a debt and must not appear.
        crate::db::service::collaboration_service::send(
            &db.conn,
            letter(conversation_id, peer_id, "waiting-1", true),
        )
        .await
        .unwrap();

        let mut runtime = runtime(crate::db::AppDatabase {
            conn: db.conn.clone(),
        });
        runtime
            .idle_since
            .insert(conversation_id, Utc::now() - Duration::seconds(5));
        runtime.fire_due(None).await;
        let texts = queue_text(&db, conversation_id).await;
        assert_eq!(texts.len(), 1);
        assert!(
            texts[0].contains("你欠回复 1 封会话信件"),
            "the fire names the open debt count: {}",
            texts[0]
        );
        assert!(
            !texts[0].contains('2'),
            "outbound awaits are not the Session's debt: {}",
            texts[0]
        );
    }

    #[tokio::test]
    async fn delay_doubles_until_reset_delay_returns_to_grace() {
        let (db, conversation_id, _) = setup().await;
        let timer = create_timer(&db, conversation_id, "grown-delay").await;
        backdate_last_fire(&db, &timer.id, 3).await;

        let mut runtime = runtime(crate::db::AppDatabase {
            conn: db.conn.clone(),
        });
        runtime
            .idle_since
            .insert(conversation_id, Utc::now() - Duration::seconds(5));
        runtime.fire_due(None).await;
        assert!(
            queue_text(&db, conversation_id).await.is_empty(),
            "strike 3 with grace 1 waits 8s, not 5s"
        );

        runtime
            .idle_since
            .insert(conversation_id, Utc::now() - Duration::seconds(9));
        runtime.fire_due(None).await;
        let texts = queue_text(&db, conversation_id).await;
        assert_eq!(texts.len(), 1);
        assert!(texts[0].starts_with("Read docs/current-task.md and continue"));
        assert!(texts[0].contains("timer.reset_delay"));
        let grown = session_timer_service::list(&db.conn, conversation_id)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(grown.strike_count, 4);

        let reset = session_timer_service::reset_delay(&db.conn, &grown.id, grown.updated_at)
            .await
            .unwrap();
        assert_eq!(reset.strike_count, 0);
        assert!(reset.auto_paused_at.is_none());
    }

    #[tokio::test]
    async fn reset_delay_lets_the_next_idle_window_use_grace() {
        let (db, conversation_id, _) = setup().await;
        let timer = create_timer(&db, conversation_id, "reset-to-grace").await;
        backdate_last_fire(&db, &timer.id, 8).await;
        let current = session_timer_service::find(&db.conn, &timer.id)
            .await
            .unwrap();
        session_timer_service::reset_delay(&db.conn, &current.id, current.updated_at)
            .await
            .unwrap();

        let mut runtime = runtime(crate::db::AppDatabase {
            conn: db.conn.clone(),
        });
        runtime
            .idle_since
            .insert(conversation_id, Utc::now() - Duration::seconds(2));
        runtime.fire_due(None).await;
        assert_eq!(
            queue_text(&db, conversation_id).await,
            ["Read docs/current-task.md and continue"]
        );
        let fired = session_timer_service::list(&db.conn, conversation_id)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(fired.strike_count, 1);
    }

    #[test]
    fn reminder_delay_doubles_then_caps() {
        assert_eq!(reminder_delay_secs(2, 0), 2);
        assert_eq!(reminder_delay_secs(2, 1), 4);
        assert_eq!(reminder_delay_secs(2, 2), 8);
        assert_eq!(reminder_delay_secs(2, 20), MAX_REMINDER_DELAY_SECS);
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
                task_id: None,
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
