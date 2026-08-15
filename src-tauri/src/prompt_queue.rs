//! Backend-authoritative follow-up queue for one persistent Session.
//!
//! The frontend may render the queue in several Workbenches/windows, but it
//! never owns dispatch. One process worker claims the FIFO head, hands it to
//! the already-running Harness through `ConnectionManager::send_prompt_linked`,
//! and removes it only after that call accepts the prompt. Closed Sessions are
//! not cold-started by V1: their durable items simply wait for a normal resume.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::Duration;
use sea_orm::EntityTrait;
use tokio::sync::{broadcast, mpsc};

use crate::acp::error::AcpError;
use crate::acp::manager::ConnectionManager;
use crate::acp::types::{AcpEvent, ConnectionStatus};
use crate::acp::InternalEventBus;
use crate::db::entities::{conversation, folder};
use crate::db::service::prompt_queue_service;
use crate::db::AppDatabase;
use crate::models::{AgentType, PromptQueueSnapshot};
use crate::parsers::path_eq_for_matching;
use crate::web::event_bridge::{emit_event, EventEmitter, PROMPT_QUEUE_CHANGED_EVENT};

const CLAIM_LEASE_SECS: i64 = 30;
const DISPATCH_LEASE_SECS: i64 = 300;
const LEASE_SWEEP_SECS: u64 = 15;
const ACCEPT_RETRY_DELAYS_MS: [u64; 3] = [25, 75, 200];

#[derive(Clone)]
pub struct PromptQueueHandle {
    wake_tx: mpsc::UnboundedSender<i32>,
}

impl PromptQueueHandle {
    pub fn wake(&self, conversation_id: i32) {
        let _ = self.wake_tx.send(conversation_id);
    }

    /// A non-running handle for handler-only tests. Queue mutations remain
    /// durable; they simply do not dispatch until a real runtime is installed.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn disconnected_for_test() -> Self {
        let (wake_tx, wake_rx) = mpsc::unbounded_channel();
        drop(wake_rx);
        Self { wake_tx }
    }
}

fn emit_snapshot(emitter: &EventEmitter, snapshot: PromptQueueSnapshot) {
    emit_event(emitter, PROMPT_QUEUE_CHANGED_EVENT, snapshot);
}

pub fn build_prompt_queue_runtime(
    db_conn: sea_orm::DatabaseConnection,
    manager: ConnectionManager,
    emitter: EventEmitter,
    bus: Arc<InternalEventBus>,
) -> (PromptQueueHandle, impl Future<Output = ()> + Send + 'static) {
    // Subscribe before returning the future, matching the lifecycle subscriber:
    // events emitted between construction and spawn are buffered, not lost.
    let bus_rx = bus.subscribe();
    let (wake_tx, wake_rx) = mpsc::unbounded_channel();
    let handle = PromptQueueHandle { wake_tx };
    let runtime = PromptQueueRuntime {
        db: AppDatabase { conn: db_conn },
        manager,
        emitter,
        worker_id: format!("prompt-queue-{}", uuid::Uuid::new_v4()),
        bus,
        bus_rx,
        wake_rx,
    };
    (handle, async move { runtime.run().await })
}

struct PromptQueueRuntime {
    db: AppDatabase,
    manager: ConnectionManager,
    emitter: EventEmitter,
    worker_id: String,
    bus: Arc<InternalEventBus>,
    bus_rx: broadcast::Receiver<Arc<crate::acp::EventEnvelope>>,
    wake_rx: mpsc::UnboundedReceiver<i32>,
}

impl PromptQueueRuntime {
    async fn run(mut self) {
        self.recover_and_scan().await;
        let mut sweep = tokio::time::interval(StdDuration::from_secs(LEASE_SWEEP_SECS));
        sweep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                wake = self.wake_rx.recv() => {
                    let Some(conversation_id) = wake else { break; };
                    self.process(conversation_id).await;
                }
                event = self.bus_rx.recv() => {
                    match event {
                        Ok(event) => self.on_acp_event(&event).await,
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            self.bus.metrics().lagged_count.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
                            self.recover_and_scan().await;
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = sweep.tick() => self.recover_and_scan().await,
            }
        }
    }

    async fn recover_and_scan(&self) {
        match prompt_queue_service::recover_expired_claims(&self.db.conn).await {
            Ok(snapshots) => {
                for snapshot in snapshots {
                    emit_snapshot(&self.emitter, snapshot);
                }
            }
            Err(err) => tracing::error!("[prompt-queue] lease recovery failed: {err}"),
        }
        match prompt_queue_service::pending_conversation_ids(&self.db.conn).await {
            Ok(ids) => {
                for id in ids {
                    self.process(id).await;
                }
            }
            Err(err) => tracing::error!("[prompt-queue] pending scan failed: {err}"),
        }
    }

    async fn on_acp_event(&self, event: &crate::acp::EventEnvelope) {
        let explicit = match &event.payload {
            AcpEvent::ConversationLinked {
                conversation_id, ..
            }
            | AcpEvent::ConversationStatusChanged {
                conversation_id, ..
            } => Some(*conversation_id),
            AcpEvent::TurnComplete { .. } | AcpEvent::SessionStarted { .. } => None,
            _ => return,
        };
        let conversation_id = match explicit {
            Some(id) => Some(id),
            None => match self.manager.get_state(&event.connection_id).await {
                Some(state) => state.read().await.conversation_id,
                None => None,
            },
        };
        if let Some(id) = conversation_id {
            self.process(id).await;
        }
    }

    async fn active_connection(
        &self,
        row: &conversation::Model,
    ) -> Option<(String, Arc<tokio::sync::RwLock<crate::acp::SessionState>>)> {
        let id = if let Some(id) = self
            .manager
            .find_connection_by_conversation_id(row.id)
            .await
        {
            Some(id)
        } else if let (Some(external_id), Some(agent_type)) = (
            row.external_id.as_deref(),
            AgentType::from_wire(&row.agent_type),
        ) {
            self.manager
                .find_connection_by_external_id(external_id, agent_type)
                .await
        } else {
            None
        }?;
        let state = self.manager.get_state(&id).await?;
        Some((id, state))
    }

    async fn process(&self, conversation_id: i32) {
        let Some(row) = (match conversation::Entity::find_by_id(conversation_id)
            .one(&self.db.conn)
            .await
        {
            Ok(row) => row,
            Err(err) => {
                tracing::error!("[prompt-queue] conversation lookup failed: {err}");
                return;
            }
        }) else {
            return;
        };
        if row.deleted_at.is_some() {
            return;
        }
        let Some((connection_id, state)) = self.active_connection(&row).await else {
            // V1 deliberately does not cold-start a Session just because a
            // follow-up exists. The next ordinary resume emits SessionStarted
            // and wakes this worker.
            return;
        };
        {
            let state = state.read().await;
            if state.status != ConnectionStatus::Connected || state.turn_in_flight {
                return;
            }
        }

        let claimed = match prompt_queue_service::claim_head(
            &self.db.conn,
            conversation_id,
            &self.worker_id,
            Duration::seconds(CLAIM_LEASE_SECS),
        )
        .await
        {
            Ok(Some((item, snapshot))) => {
                emit_snapshot(&self.emitter, snapshot);
                item
            }
            Ok(None) => return,
            Err(err) => {
                tracing::error!("[prompt-queue] claim failed for {conversation_id}: {err}");
                return;
            }
        };

        // Re-check after the durable claim: a competing client may have begun a
        // turn in the small window between the idle snapshot and the claim.
        let (status, turn_in_flight, actual_cwd) =
            match self.manager.get_state(&connection_id).await {
                Some(state) => {
                    let state = state.read().await;
                    (
                        state.status.clone(),
                        state.turn_in_flight,
                        state
                            .working_dir
                            .as_ref()
                            .map(|path| path.to_string_lossy().into_owned()),
                    )
                }
                None => {
                    self.release_busy(&claimed).await;
                    return;
                }
            };
        if status != ConnectionStatus::Connected || turn_in_flight {
            self.release_busy(&claimed).await;
            return;
        }

        let expected_cwd = if let Some(origin) = row.origin_cwd.clone() {
            Some(origin)
        } else {
            match folder::Entity::find_by_id(row.folder_id)
                .one(&self.db.conn)
                .await
            {
                Ok(folder) => folder.map(|folder| folder.path),
                Err(err) => {
                    self.fail(
                        &claimed,
                        &format!("Could not resolve Session working directory: {err}"),
                    )
                    .await;
                    return;
                }
            }
        };
        let cwd_matches = match (actual_cwd.as_deref(), expected_cwd.as_deref()) {
            (Some(actual), Some(expected)) => path_eq_for_matching(actual, expected),
            (None, None) => true,
            _ => false,
        };
        if !cwd_matches {
            self.fail(
                &claimed,
                "The active Harness is running in a different working directory",
            )
            .await;
            return;
        }

        if let Some(mode_id) = claimed.mode_id.clone() {
            if let Err(err) = self.manager.set_mode(&connection_id, mode_id).await {
                self.fail(&claimed, &format!("Could not apply queued mode: {err}"))
                    .await;
                return;
            }
        }

        match prompt_queue_service::mark_dispatch_started(
            &self.db.conn,
            &claimed,
            Duration::seconds(DISPATCH_LEASE_SECS),
        )
        .await
        {
            Ok(true) => {}
            Ok(false) => {
                tracing::warn!(
                    "[prompt-queue] claim {} expired before dispatch; prompt was not sent",
                    claimed.id
                );
                self.release_busy(&claimed).await;
                return;
            }
            Err(err) => {
                tracing::error!(
                    "[prompt-queue] could not mark dispatch for {}: {err}",
                    claimed.id
                );
                self.release_busy(&claimed).await;
                return;
            }
        }

        let outcome = self
            .manager
            .send_prompt_linked_with_message_id(
                &self.db,
                &connection_id,
                claimed.draft.blocks.clone(),
                Some(row.folder_id),
                Some(row.id),
                None,
                Some(claimed.id.clone()),
            )
            .await;
        match outcome {
            Ok(_) => self.accept_after_dispatch(&claimed).await,
            Err(AcpError::TurnInProgress) => self.release_busy(&claimed).await,
            Err(err) => self.fail(&claimed, &err.to_string()).await,
        }
    }

    async fn accept_after_dispatch(
        &self,
        item: &crate::models::prompt_queue::ClaimedPromptQueueItem,
    ) {
        let mut last_error = None;
        for delay_ms in ACCEPT_RETRY_DELAYS_MS {
            match prompt_queue_service::accept_claim(&self.db.conn, item).await {
                Ok(snapshot) => {
                    emit_snapshot(&self.emitter, snapshot);
                    return;
                }
                Err(err) => {
                    last_error = Some(err);
                    tokio::time::sleep(StdDuration::from_millis(delay_ms)).await;
                }
            }
        }
        tracing::error!(
            "[prompt-queue] Harness accepted {}, but queue acknowledgement failed after retries: {}",
            item.id,
            last_error
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "unknown database error".to_string())
        );
        match prompt_queue_service::pause_dispatch_unknown(&self.db.conn, item).await {
            Ok(snapshot) => emit_snapshot(&self.emitter, snapshot),
            Err(err) => tracing::error!(
                "[prompt-queue] could not persist unknown dispatch state for {}: {err}",
                item.id
            ),
        }
    }

    async fn release_busy(&self, item: &crate::models::prompt_queue::ClaimedPromptQueueItem) {
        match prompt_queue_service::release_claim_busy(&self.db.conn, item).await {
            Ok(snapshot) => emit_snapshot(&self.emitter, snapshot),
            Err(err) => tracing::error!("[prompt-queue] busy release failed: {err}"),
        }
    }

    async fn fail(&self, item: &crate::models::prompt_queue::ClaimedPromptQueueItem, reason: &str) {
        match prompt_queue_service::fail_claim(&self.db.conn, item, reason).await {
            Ok(snapshot) => emit_snapshot(&self.emitter, snapshot),
            Err(err) => tracing::error!("[prompt-queue] pause failed: {err}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::connection::ConnectionCommand;
    use crate::acp::internal_bus::EventBusMetrics;
    use crate::acp::types::{EventEnvelope, PromptInputBlock};
    use crate::db::service::prompt_queue_service;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::{EnqueuePromptQueueItem, PromptQueueDraft, PromptQueueItemState};
    use std::path::PathBuf;

    fn input(conversation_id: i32, id: &str, text: &str) -> EnqueuePromptQueueItem {
        EnqueuePromptQueueItem {
            conversation_id,
            id: id.to_string(),
            client_dedupe_id: id.to_string(),
            draft: PromptQueueDraft {
                blocks: vec![PromptInputBlock::Text {
                    text: text.to_string(),
                }],
                display_text: text.to_string(),
            },
            mode_id: None,
        }
    }

    async fn wait_until<F, Fut>(mut predicate: F)
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = bool>,
    {
        tokio::time::timeout(StdDuration::from_secs(2), async {
            loop {
                if predicate().await {
                    return;
                }
                tokio::time::sleep(StdDuration::from_millis(10)).await;
            }
        })
        .await
        .expect("condition did not converge");
    }

    async fn setup(
        path: &str,
    ) -> (
        crate::db::AppDatabase,
        i32,
        i32,
        ConnectionManager,
        Arc<InternalEventBus>,
    ) {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, path).await;
        let conversation_id = seed_conversation(&db, folder_id, AgentType::Codex).await;
        let manager = ConnectionManager::new();
        let bus = Arc::new(InternalEventBus::new(Arc::new(EventBusMetrics::default())));
        (db, folder_id, conversation_id, manager, bus)
    }

    async fn bind_live_connection(
        manager: &ConnectionManager,
        connection_id: &str,
        working_dir: &str,
        folder_id: i32,
        conversation_id: i32,
    ) -> tokio::sync::mpsc::Receiver<ConnectionCommand> {
        let rx = manager
            .insert_test_connection_live(
                connection_id,
                AgentType::Codex,
                Some(PathBuf::from(working_dir)),
                EventEmitter::Noop,
            )
            .await;
        let state = manager
            .get_state(connection_id)
            .await
            .expect("connection state");
        let mut state = state.write().await;
        state.folder_id = Some(folder_id);
        state.conversation_id = Some(conversation_id);
        drop(state);
        rx
    }

    #[tokio::test]
    async fn closed_session_stays_queued_without_cold_start() {
        let (db, _, conversation_id, manager, bus) = setup("/tmp/codeg-queue-no-cold-start").await;
        let (handle, task) = build_prompt_queue_runtime(
            db.conn.clone(),
            manager.clone_ref(),
            EventEmitter::Noop,
            bus,
        );
        let worker = tokio::spawn(task);
        prompt_queue_service::enqueue(&db.conn, input(conversation_id, "one", "wait"))
            .await
            .expect("enqueue");
        handle.wake(conversation_id);
        tokio::time::sleep(StdDuration::from_millis(80)).await;

        let snapshot = prompt_queue_service::snapshot(&db.conn, conversation_id)
            .await
            .expect("snapshot");
        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(snapshot.items[0].state, PromptQueueItemState::Queued);
        assert!(manager.list_connections().await.is_empty());
        worker.abort();
    }

    #[tokio::test]
    async fn active_idle_session_accepts_once_and_removes_the_claim() {
        let path = "/tmp/codeg-queue-active";
        let (db, folder_id, conversation_id, manager, bus) = setup(path).await;
        let mut commands =
            bind_live_connection(&manager, "active", path, folder_id, conversation_id).await;
        prompt_queue_service::enqueue(&db.conn, input(conversation_id, "stable-id", "hello"))
            .await
            .expect("enqueue");
        let (_handle, task) =
            build_prompt_queue_runtime(db.conn.clone(), manager, EventEmitter::Noop, bus);
        let worker = tokio::spawn(task);

        let command = tokio::time::timeout(StdDuration::from_secs(2), commands.recv())
            .await
            .expect("prompt timeout")
            .expect("prompt command");
        let ConnectionCommand::Prompt {
            blocks,
            user_message,
        } = command
        else {
            panic!("expected prompt command");
        };
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            user_message.as_ref().map(|(id, _)| id.as_str()),
            Some("stable-id")
        );
        wait_until(|| async {
            prompt_queue_service::snapshot(&db.conn, conversation_id)
                .await
                .is_ok_and(|snapshot| snapshot.items.is_empty())
        })
        .await;
        assert!(
            commands.try_recv().is_err(),
            "the accepted item must not replay"
        );
        worker.abort();
    }

    #[tokio::test]
    async fn turn_complete_drains_the_next_item_but_busy_state_does_not() {
        let path = "/tmp/codeg-queue-turn-complete";
        let (db, folder_id, conversation_id, manager, bus) = setup(path).await;
        let mut commands =
            bind_live_connection(&manager, "active", path, folder_id, conversation_id).await;
        prompt_queue_service::enqueue(&db.conn, input(conversation_id, "one", "first"))
            .await
            .expect("one");
        prompt_queue_service::enqueue(&db.conn, input(conversation_id, "two", "second"))
            .await
            .expect("two");
        let (_handle, task) = build_prompt_queue_runtime(
            db.conn.clone(),
            manager.clone_ref(),
            EventEmitter::Noop,
            bus.clone(),
        );
        let worker = tokio::spawn(task);

        tokio::time::timeout(StdDuration::from_secs(2), commands.recv())
            .await
            .expect("first timeout")
            .expect("first prompt");
        tokio::time::sleep(StdDuration::from_millis(80)).await;
        assert!(
            commands.try_recv().is_err(),
            "busy Session must not receive item two"
        );

        let state = manager.get_state("active").await.expect("state");
        {
            let mut state = state.write().await;
            state.turn_in_flight = false;
            state.status = ConnectionStatus::Connected;
        }
        bus.send(Arc::new(EventEnvelope {
            seq: 1,
            connection_id: "active".into(),
            payload: AcpEvent::TurnComplete {
                session_id: "native-session".into(),
                stop_reason: "end_turn".into(),
                agent_type: "codex".into(),
            },
        }));
        let second = tokio::time::timeout(StdDuration::from_secs(2), commands.recv())
            .await
            .expect("second timeout")
            .expect("second prompt");
        let ConnectionCommand::Prompt { user_message, .. } = second else {
            panic!("expected second prompt");
        };
        assert_eq!(
            user_message.as_ref().map(|(id, _)| id.as_str()),
            Some("two")
        );
        worker.abort();
    }

    #[tokio::test]
    async fn cwd_mismatch_pauses_instead_of_sending_to_the_wrong_session() {
        let (db, folder_id, conversation_id, manager, bus) =
            setup("/tmp/codeg-queue-expected-cwd").await;
        let mut commands = bind_live_connection(
            &manager,
            "wrong-cwd",
            "/tmp/codeg-queue-other-cwd",
            folder_id,
            conversation_id,
        )
        .await;
        prompt_queue_service::enqueue(&db.conn, input(conversation_id, "one", "do not send"))
            .await
            .expect("enqueue");
        let (_handle, task) =
            build_prompt_queue_runtime(db.conn.clone(), manager, EventEmitter::Noop, bus);
        let worker = tokio::spawn(task);

        wait_until(|| async {
            prompt_queue_service::snapshot(&db.conn, conversation_id)
                .await
                .is_ok_and(|snapshot| snapshot.paused_reason.is_some())
        })
        .await;
        let snapshot = prompt_queue_service::snapshot(&db.conn, conversation_id)
            .await
            .expect("snapshot");
        assert_eq!(snapshot.items[0].state, PromptQueueItemState::Paused);
        assert!(
            snapshot
                .paused_reason
                .unwrap()
                .contains("working directory"),
            "pause should explain the safety boundary"
        );
        assert!(commands.try_recv().is_err());
        worker.abort();
    }

    #[tokio::test]
    async fn cancelling_a_turn_pauses_followups_before_they_can_continue() {
        let path = "/tmp/codeg-queue-cancel";
        let (db, folder_id, conversation_id, manager, _bus) = setup(path).await;
        let mut commands =
            bind_live_connection(&manager, "active", path, folder_id, conversation_id).await;
        prompt_queue_service::enqueue(&db.conn, input(conversation_id, "later", "wait"))
            .await
            .expect("enqueue");

        crate::commands::acp::acp_cancel_core(&db, &manager, &EventEmitter::Noop, "active")
            .await
            .expect("cancel");
        let command = commands.recv().await.expect("cancel command");
        assert!(matches!(command, ConnectionCommand::Cancel));
        let snapshot = prompt_queue_service::snapshot(&db.conn, conversation_id)
            .await
            .expect("snapshot");
        assert_eq!(
            snapshot.paused_reason.as_deref(),
            Some("cancelled_current_turn")
        );
        assert_eq!(snapshot.items[0].state, PromptQueueItemState::Queued);
    }
}
