//! Background persistence for lifecycle events on the in-process ACP bus.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use sea_orm::DatabaseConnection;
use tokio::sync::{broadcast, mpsc, RwLock};

use crate::acp::internal_bus::InternalEventBus;
use crate::acp::manager::ConnectionManager;
use crate::acp::session_state::SessionState;
use crate::acp::types::{AcpEvent, ConnectionStatus, EventEnvelope};
use crate::db::entities::conversation::ConversationStatus;
use crate::db::error::DbError;
use crate::db::service::conversation_service;
use crate::logging::throttle::{LagLogThrottle, LAG_LOG_WINDOW};
use crate::web::event_bridge::{emit_with_state, EventEmitter};

const WORKER_QUEUE_CAPACITY: usize = 64;
/// Cursor's generic title for an MCP call before the server/tool identity is known.
pub(crate) const CURSOR_IDENTITYLESS_MCP_TITLE: &str = "MCP: tool";
const HANDLE_EVENT_RETRY_BACKOFFS: &[Duration] =
    &[Duration::from_millis(100), Duration::from_millis(500)];

fn is_lifecycle_relevant(event: &AcpEvent) -> bool {
    matches!(
        event,
        AcpEvent::SessionStarted { .. }
            | AcpEvent::TurnComplete { .. }
            | AcpEvent::ConversationLinked { .. }
            | AcpEvent::ConversationForked { .. }
            | AcpEvent::StatusChanged {
                status: ConnectionStatus::Disconnected
            }
            | AcpEvent::Error { .. }
    )
}

fn is_dispatcher_terminal(event: &AcpEvent) -> bool {
    matches!(
        event,
        AcpEvent::StatusChanged {
            status: ConnectionStatus::Disconnected
        } | AcpEvent::Error { terminal: true, .. }
    )
}

struct CachedConn {
    conversation_id: i32,
    state: Arc<RwLock<SessionState>>,
    emitter: EventEmitter,
}

async fn handle_event_with_retry(
    db: &DatabaseConnection,
    manager: &ConnectionManager,
    envelope: &EventEnvelope,
) {
    match handle_event(db, manager, envelope).await {
        Ok(()) => return,
        // B1: a permanent refusal, not a transient failure — retrying a
        // takeover that's already been rejected can never succeed, since the
        // holder of the external_id does not change on its own. Log once at
        // WARN (this is an expected outcome of a lost race, not a bug) and
        // skip the backoff loop entirely rather than burning two retries and
        // an ERROR log on something retrying cannot fix.
        Err(DbError::ExternalIdTaken(msg)) => {
            tracing::warn!(
                "[lifecycle][WARN] handle_event: external_id already bound elsewhere for {:?}, \
                 not retrying: {msg}",
                envelope.payload
            );
            return;
        }
        Err(_) => {}
    }
    for (attempt, backoff) in HANDLE_EVENT_RETRY_BACKOFFS.iter().enumerate() {
        tokio::time::sleep(*backoff).await;
        match handle_event(db, manager, envelope).await {
            Ok(()) => return,
            Err(DbError::ExternalIdTaken(msg)) => {
                tracing::warn!(
                    "[lifecycle][WARN] handle_event: external_id already bound elsewhere for \
                     {:?}, not retrying further: {msg}",
                    envelope.payload
                );
                return;
            }
            Err(err) => tracing::warn!(
                "[lifecycle][{}] handle_event attempt {} failed for {:?}: {err}",
                if attempt + 1 == HANDLE_EVENT_RETRY_BACKOFFS.len() {
                    "ERROR"
                } else {
                    "WARN"
                },
                attempt + 2,
                envelope.payload
            ),
        }
    }
}

pub(crate) async fn handle_event(
    db: &DatabaseConnection,
    manager: &ConnectionManager,
    envelope: &EventEnvelope,
) -> Result<(), DbError> {
    match &envelope.payload {
        AcpEvent::SessionStarted { session_id } => {
            let Some((state, emitter)) =
                manager.get_state_and_emitter(&envelope.connection_id).await
            else {
                return Ok(());
            };
            if let Some(conversation_id) = state.read().await.conversation_id {
                let outcome =
                    conversation_service::bind_external_id(db, conversation_id, session_id.clone())
                        .await?;
                crate::commands::conversations::emit_conversation_upsert(
                    &emitter,
                    db,
                    conversation_id,
                )
                .await;
                if let Some(preserved_id) = outcome.preserved_conversation_id {
                    // A1/A3: an unrelated previous session was split off this
                    // row rather than silently overwritten — broadcast it too.
                    crate::commands::conversations::emit_conversation_upsert(
                        &emitter,
                        db,
                        preserved_id,
                    )
                    .await;
                }
            }
            Ok(())
        }
        AcpEvent::TurnComplete { stop_reason, .. } => {
            let target = match stop_reason.as_str() {
                "end_turn" => Some(ConversationStatus::PendingReview),
                "refusal" | "max_tokens" | "max_turn_requests" | "unknown" | "empty" => {
                    Some(ConversationStatus::Cancelled)
                }
                _ => None,
            };
            let Some(target) = target else {
                return Ok(());
            };
            let Some((state, emitter)) =
                manager.get_state_and_emitter(&envelope.connection_id).await
            else {
                return Ok(());
            };
            let Some(conversation_id) = state.read().await.conversation_id else {
                return Ok(());
            };
            conversation_service::update_status(db, conversation_id, target.clone()).await?;
            emit_with_state(
                &state,
                &emitter,
                AcpEvent::ConversationStatusChanged {
                    conversation_id,
                    status: target,
                },
            )
            .await;
            Ok(())
        }
        _ => Ok(()),
    }
}

async fn try_cache_link(
    cache: &mut HashMap<String, CachedConn>,
    manager: &ConnectionManager,
    connection_id: &str,
    conversation_id: i32,
    allow_rebind: bool,
) {
    if cache.contains_key(connection_id) && !allow_rebind {
        return;
    }
    let Some((state, emitter)) = manager.get_state_and_emitter(connection_id).await else {
        return;
    };
    cache.insert(
        connection_id.to_string(),
        CachedConn {
            conversation_id,
            state,
            emitter,
        },
    );
}

async fn handle_terminal_event(
    db: &DatabaseConnection,
    cache: &mut HashMap<String, CachedConn>,
    connection_id: &str,
) -> Result<(), DbError> {
    let Some(entry) = cache.remove(connection_id) else {
        return Ok(());
    };
    let changed = conversation_service::update_status_if(
        db,
        entry.conversation_id,
        ConversationStatus::InProgress,
        ConversationStatus::Cancelled,
    )
    .await?;
    if changed {
        emit_with_state(
            &entry.state,
            &entry.emitter,
            AcpEvent::ConversationStatusChanged {
                conversation_id: entry.conversation_id,
                status: ConversationStatus::Cancelled,
            },
        )
        .await;
    }
    Ok(())
}

async fn connection_worker_loop(
    connection_id: String,
    db: DatabaseConnection,
    manager: ConnectionManager,
    mut rx: mpsc::Receiver<Arc<EventEnvelope>>,
) {
    let mut cache = HashMap::new();
    let mut terminal_dispatched = false;
    while let Some(envelope) = rx.recv().await {
        match &envelope.payload {
            AcpEvent::ConversationLinked {
                conversation_id, ..
            } => {
                try_cache_link(
                    &mut cache,
                    &manager,
                    &connection_id,
                    *conversation_id,
                    false,
                )
                .await;
            }
            AcpEvent::ConversationForked {
                forked_conversation_id,
                ..
            } => {
                try_cache_link(
                    &mut cache,
                    &manager,
                    &connection_id,
                    *forked_conversation_id,
                    true,
                )
                .await;
            }
            AcpEvent::StatusChanged {
                status: ConnectionStatus::Disconnected,
            }
            | AcpEvent::Error { terminal: true, .. } => {
                if !terminal_dispatched {
                    if let Err(err) = handle_terminal_event(&db, &mut cache, &connection_id).await {
                        tracing::error!(
                            "[lifecycle][ERROR] terminal event for {connection_id}: {err}"
                        );
                    }
                    terminal_dispatched = true;
                }
            }
            AcpEvent::Error {
                terminal: false, ..
            } => {}
            _ => handle_event_with_retry(&db, &manager, &envelope).await,
        }
    }
}

pub fn lifecycle_subscriber_task(
    db: DatabaseConnection,
    manager: ConnectionManager,
    bus: Arc<InternalEventBus>,
) -> impl Future<Output = ()> + Send + 'static {
    let mut rx = bus.subscribe();
    let metrics = Arc::clone(bus.metrics());
    async move {
        let mut workers: HashMap<String, mpsc::Sender<Arc<EventEnvelope>>> = HashMap::new();
        let mut lag_throttle = LagLogThrottle::new(LAG_LOG_WINDOW);
        loop {
            match rx.recv().await {
                Ok(envelope) => {
                    if !is_lifecycle_relevant(&envelope.payload) {
                        continue;
                    }
                    let connection_id = envelope.connection_id.clone();
                    let is_terminal = is_dispatcher_terminal(&envelope.payload);
                    let tx = workers.entry(connection_id.clone()).or_insert_with(|| {
                        let (tx, worker_rx) = mpsc::channel(WORKER_QUEUE_CAPACITY);
                        tokio::spawn(connection_worker_loop(
                            connection_id.clone(),
                            db.clone(),
                            manager.clone_ref(),
                            worker_rx,
                        ));
                        tx
                    });
                    let result = match tx.try_send(envelope) {
                        Ok(()) => Ok(()),
                        Err(mpsc::error::TrySendError::Full(envelope)) => {
                            metrics
                                .worker_queue_full_count
                                .fetch_add(1, Ordering::Relaxed);
                            tx.send(envelope).await.map_err(|_| ())
                        }
                        Err(mpsc::error::TrySendError::Closed(_)) => Err(()),
                    };
                    if result.is_err() || is_terminal {
                        workers.remove(&connection_id);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    metrics.lagged_count.fetch_add(skipped, Ordering::Relaxed);
                    if let Some(summary) = lag_throttle.record(skipped) {
                        tracing::warn!(
                            "[lifecycle][WARN] internal bus lagged: dropped {} events across {} occurrence(s) in {}s",
                            summary.dropped,
                            summary.occurrences,
                            LAG_LOG_WINDOW.as_secs()
                        );
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers;
    use crate::models::agent::AgentType;
    use sea_orm::EntityTrait;
    use tokio::sync::{mpsc, RwLock};

    fn fake_connection(
        id: &str,
        conversation_id: Option<i32>,
    ) -> crate::acp::connection::AgentConnection {
        let (cmd_tx, _cmd_rx) = mpsc::channel(1);
        let mut state = SessionState::new(
            id.to_string(),
            AgentType::ClaudeCode,
            None,
            "test-window".to_string(),
            None,
        );
        state.conversation_id = conversation_id;
        crate::acp::connection::AgentConnection {
            id: id.to_string(),
            agent_type: AgentType::ClaudeCode,
            status: ConnectionStatus::Connected,
            owner_window_label: "test-window".to_string(),
            cmd_tx,
            state: Arc::new(RwLock::new(state)),
            emitter: EventEmitter::Noop,
            prompt_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_fingerprint: String::new(),
            last_observed_fingerprint: String::new(),
            child_pid: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }
    }

    async fn row_status(db: &crate::db::AppDatabase, conversation_id: i32) -> ConversationStatus {
        crate::db::entities::conversation::Entity::find_by_id(conversation_id)
            .one(&db.conn)
            .await
            .unwrap()
            .expect("conversation row")
            .status
    }

    async fn seed_bound_conversation(
        path: &str,
    ) -> (crate::db::AppDatabase, ConnectionManager, i32) {
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, path).await;
        let conversation =
            conversation_service::create(&db.conn, folder_id, AgentType::ClaudeCode, None, None)
                .await
                .unwrap();
        let manager = ConnectionManager::new();
        manager.connections.lock().await.insert(
            "connection-1".to_string(),
            fake_connection("connection-1", Some(conversation.id)),
        );
        (db, manager, conversation.id)
    }

    #[tokio::test]
    async fn session_started_persists_native_session_id() {
        let (db, manager, conversation_id) =
            seed_bound_conversation("/tmp/lifecycle-session-started").await;
        handle_event(
            &db.conn,
            &manager,
            &EventEnvelope {
                seq: 1,
                connection_id: "connection-1".into(),
                payload: AcpEvent::SessionStarted {
                    session_id: "native-session-1".into(),
                },
            },
        )
        .await
        .unwrap();

        let conversation = conversation_service::get_by_id(&db.conn, conversation_id)
            .await
            .unwrap();
        assert_eq!(
            conversation.external_id.as_deref(),
            Some("native-session-1")
        );
    }

    /// A1 via the SessionStarted consumer: the bound row already carries an
    /// unrelated session (a reconnect lost the old id, or `session/load`
    /// fell back to `session/new`). `handle_event` must split the old value
    /// off onto its own row via `bind_external_id` and broadcast an upsert
    /// for BOTH rows (A3) rather than silently overwriting the old session.
    #[tokio::test]
    async fn session_started_splits_and_broadcasts_an_unrelated_previous_session() {
        let (db, manager, conversation_id) =
            seed_bound_conversation("/tmp/lifecycle-session-started-split").await;
        conversation_service::bind_external_id(&db.conn, conversation_id, "session-old".into())
            .await
            .unwrap();

        handle_event(
            &db.conn,
            &manager,
            &EventEnvelope {
                seq: 1,
                connection_id: "connection-1".into(),
                payload: AcpEvent::SessionStarted {
                    session_id: "session-new".into(),
                },
            },
        )
        .await
        .unwrap();

        let live = conversation_service::get_by_id(&db.conn, conversation_id)
            .await
            .unwrap();
        assert_eq!(live.external_id.as_deref(), Some("session-new"));

        // The old session must not have vanished: some OTHER row now carries
        // it.
        let all = conversation_service::list_all(&db.conn, None, None, None, None, None, false, false)
            .await
            .unwrap();
        let preserved = all
            .iter()
            .find(|c| c.id != conversation_id && c.external_id.as_deref() == Some("session-old"))
            .expect("the abandoned session must be preserved on its own row");
        assert_eq!(
            preserved.status, "cancelled",
            "the abandoned in-flight session is marked cancelled, matching the \
             disconnect convention `handle_terminal_event` already applies"
        );
    }

    /// B1 at the lifecycle consumer: a permanent refusal must not be retried.
    /// `handle_event_with_retry`'s only observable signal for "did it retry"
    /// is elapsed time (nothing in the DB changes between attempts, so a
    /// retried and a non-retried call reach the same final state) — this
    /// asserts completion well under the first backoff
    /// (`HANDLE_EVENT_RETRY_BACKOFFS[0]` = 100ms), which a retry loop could
    /// not achieve.
    #[tokio::test]
    async fn handle_event_with_retry_skips_backoff_on_permanent_refusal() {
        let (db, manager, conversation_id) =
            seed_bound_conversation("/tmp/lifecycle-retry-skip").await;
        conversation_service::bind_external_id(&db.conn, conversation_id, "session-mine".into())
            .await
            .unwrap();
        // A second, unrelated row already holds the id this event will try
        // to bind — guaranteed permanent refusal.
        let folder_id = test_helpers::seed_folder(&db, "/tmp/lifecycle-retry-skip-2").await;
        let other =
            conversation_service::create(&db.conn, folder_id, AgentType::ClaudeCode, None, None)
                .await
                .unwrap();
        conversation_service::bind_external_id(&db.conn, other.id, "session-taken".into())
            .await
            .unwrap();

        let start = std::time::Instant::now();
        handle_event_with_retry(
            &db.conn,
            &manager,
            &EventEnvelope {
                seq: 1,
                connection_id: "connection-1".into(),
                payload: AcpEvent::SessionStarted {
                    session_id: "session-taken".into(),
                },
            },
        )
        .await;
        assert!(
            start.elapsed() < Duration::from_millis(80),
            "a permanent ExternalIdTaken refusal must return before the first \
             100ms backoff, not after exhausting the retry loop"
        );

        // The refusal must not have mutated the row that was already correct.
        let unchanged = conversation_service::get_by_id(&db.conn, conversation_id)
            .await
            .unwrap();
        assert_eq!(unchanged.external_id.as_deref(), Some("session-mine"));
    }

    #[tokio::test]
    async fn turn_complete_maps_normal_and_failure_statuses() {
        for (stop_reason, expected) in [
            ("end_turn", ConversationStatus::PendingReview),
            ("refusal", ConversationStatus::Cancelled),
            ("max_tokens", ConversationStatus::Cancelled),
            ("max_turn_requests", ConversationStatus::Cancelled),
            ("unknown", ConversationStatus::Cancelled),
            ("empty", ConversationStatus::Cancelled),
        ] {
            let (db, manager, conversation_id) =
                seed_bound_conversation(&format!("/tmp/lifecycle-{stop_reason}")).await;
            handle_event(
                &db.conn,
                &manager,
                &EventEnvelope {
                    seq: 1,
                    connection_id: "connection-1".into(),
                    payload: AcpEvent::TurnComplete {
                        session_id: "native-session-1".into(),
                        stop_reason: stop_reason.into(),
                        agent_type: "claude_code".into(),
                    },
                },
            )
            .await
            .unwrap();
            assert_eq!(row_status(&db, conversation_id).await, expected);
        }
    }

    #[tokio::test]
    async fn cancelled_turn_complete_does_not_overwrite_user_cancel_owner() {
        let (db, manager, conversation_id) =
            seed_bound_conversation("/tmp/lifecycle-user-cancel").await;
        handle_event(
            &db.conn,
            &manager,
            &EventEnvelope {
                seq: 1,
                connection_id: "connection-1".into(),
                payload: AcpEvent::TurnComplete {
                    session_id: "native-session-1".into(),
                    stop_reason: "cancelled".into(),
                    agent_type: "claude_code".into(),
                },
            },
        )
        .await
        .unwrap();
        assert_eq!(
            row_status(&db, conversation_id).await,
            ConversationStatus::InProgress
        );
    }

    #[tokio::test]
    async fn fork_rebinds_terminal_cache_without_rewriting_original_identity() {
        let manager = ConnectionManager::new();
        manager.connections.lock().await.insert(
            "connection-fork".to_string(),
            fake_connection("connection-fork", Some(1)),
        );
        let mut cache = HashMap::new();
        try_cache_link(&mut cache, &manager, "connection-fork", 1, false).await;
        assert_eq!(cache["connection-fork"].conversation_id, 1);

        try_cache_link(&mut cache, &manager, "connection-fork", 2, true).await;
        assert_eq!(cache["connection-fork"].conversation_id, 2);
    }

    #[tokio::test]
    async fn terminal_disconnect_only_cancels_in_progress_conversation() {
        for (before, expected) in [
            (
                ConversationStatus::InProgress,
                ConversationStatus::Cancelled,
            ),
            (
                ConversationStatus::PendingReview,
                ConversationStatus::PendingReview,
            ),
            (ConversationStatus::Completed, ConversationStatus::Completed),
        ] {
            let (db, manager, conversation_id) =
                seed_bound_conversation("/tmp/lifecycle-terminal").await;
            conversation_service::update_status(&db.conn, conversation_id, before)
                .await
                .unwrap();
            let mut cache = HashMap::new();
            try_cache_link(&mut cache, &manager, "connection-1", conversation_id, false).await;
            handle_terminal_event(&db.conn, &mut cache, "connection-1")
                .await
                .unwrap();
            assert_eq!(row_status(&db, conversation_id).await, expected);
            assert!(!cache.contains_key("connection-1"));
        }
    }
}
