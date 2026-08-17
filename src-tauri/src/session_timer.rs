//! Backend-authoritative idle continuation loop for a managed Session.
//!
//! A completed Turn opens an idle window. Once the timer's short grace has
//! elapsed and the same ACP Session is still connected and idle, the timer
//! text is appended to the ordinary durable PromptQueue. It is therefore
//! indistinguishable from the user's next follow-up at the Harness boundary.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Utc};
use tokio::sync::{broadcast, mpsc};

use crate::acp::manager::ConnectionManager;
use crate::acp::types::{AcpEvent, ConnectionStatus, PromptInputBlock};
use crate::acp::InternalEventBus;
use crate::db::service::{prompt_queue_service, session_timer_service};
use crate::db::AppDatabase;
use crate::models::prompt_queue::{EnqueuePromptQueueItem, PromptQueueDraft};
use crate::web::event_bridge::{
    emit_event, EventEmitter, SessionTimerChanged, PROMPT_QUEUE_CHANGED_EVENT,
    SESSION_TIMER_CHANGED_EVENT,
};

const SCAN_INTERVAL_SECS: u64 = 1;

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
            let Some(idle_since) = self.idle_since.get(&timer.conversation_id) else {
                continue;
            };
            if now - *idle_since < chrono::Duration::seconds(timer.idle_grace_secs) {
                continue;
            }
            if !self
                .runtime_is_idle(timer.conversation_id)
                .await
                .unwrap_or(false)
            {
                continue;
            }
            self.fire(timer).await;
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

    async fn fire(&mut self, timer: crate::models::session_timer::SessionTimerInfo) {
        // Claim before enqueue so a concurrent pause or second backend cannot
        // create a prompt after losing the authoritative timer revision.
        let claimed =
            match session_timer_service::claim_fire(&self.db.conn, &timer.id, timer.updated_at)
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

        let dedupe_id = session_timer_service::fire_dedupe_id(&claimed.id, claimed.fire_count);
        let snapshot = match prompt_queue_service::enqueue(
            &self.db.conn,
            EnqueuePromptQueueItem {
                conversation_id: claimed.conversation_id,
                id: dedupe_id.clone(),
                client_dedupe_id: dedupe_id,
                draft: PromptQueueDraft {
                    blocks: vec![PromptInputBlock::Text {
                        text: claimed.prompt_text.clone(),
                    }],
                    display_text: claimed.prompt_text.clone(),
                },
                mode_id: None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::session_timer::CreateSessionTimerInput;

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

    async fn setup() -> (crate::db::AppDatabase, i32) {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/timer-engine-test").await;
        let conversation_id =
            seed_conversation(&db, folder_id, crate::models::AgentType::Codex).await;
        (db, conversation_id)
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

    #[tokio::test]
    async fn continuation_fire_is_a_normal_prompt_and_timer_rearms() {
        let (db, conversation_id) = setup().await;
        let timer = session_timer_service::create(
            &db.conn,
            CreateSessionTimerInput {
                conversation_id,
                prompt_text: "Read docs/current-task.md and continue".into(),
                idle_grace_secs: 2,
                client_dedupe_id: Some("idle-loop".into()),
            },
        )
        .await
        .unwrap();
        let mut runtime = runtime(crate::db::AppDatabase {
            conn: db.conn.clone(),
        });
        runtime
            .idle_since
            .insert(conversation_id, Utc::now() - chrono::Duration::seconds(5));
        runtime.fire(timer).await;

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
    async fn timer_never_guesses_idle_without_a_live_runtime() {
        let (db, conversation_id) = setup().await;
        session_timer_service::create(
            &db.conn,
            CreateSessionTimerInput {
                conversation_id,
                prompt_text: "Continue".into(),
                idle_grace_secs: 1,
                client_dedupe_id: Some("requires-runtime".into()),
            },
        )
        .await
        .unwrap();
        let mut runtime = runtime(crate::db::AppDatabase {
            conn: db.conn.clone(),
        });
        runtime
            .idle_since
            .insert(conversation_id, Utc::now() - chrono::Duration::seconds(5));
        runtime.fire_due(None).await;
        assert!(queue_text(&db, conversation_id).await.is_empty());
    }
}
