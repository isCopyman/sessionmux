//! Durable execution-control operations for an already-persisted collaboration
//! message. Interrupt is deliberately not a delivery mode: the message exists
//! first, then this state machine coordinates one cancellation and the next
//! queue dispatch without making the renderer the source of truth.

use chrono::{DateTime, Utc};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, QueryResult, Statement,
    TransactionTrait,
};

use crate::db::error::DbError;
use crate::models::{
    CollaborationInterruptOperationView, CollaborationInterruptState, InterruptCollaborationInput,
};

pub const PAUSE_REASON_PREFIX: &str = "collaboration_interrupt:";
pub const FAILED_PAUSE_REASON_PREFIX: &str = "collaboration_interrupt_failed:";
const MAX_DEDUPE_ID_BYTES: usize = 200;
const MAX_REASON_BYTES: usize = 2_000;

fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, values)
}

fn validation(message: impl Into<String>) -> DbError {
    DbError::Validation(message.into())
}

fn parse_operation(row: &QueryResult) -> Result<CollaborationInterruptOperationView, DbError> {
    let state_raw: String = row.try_get("", "state")?;
    let state = CollaborationInterruptState::parse(&state_raw).ok_or_else(|| {
        validation(format!(
            "Unknown collaboration interrupt state: {state_raw}"
        ))
    })?;
    Ok(CollaborationInterruptOperationView {
        id: row.try_get("", "id")?,
        event_id: row.try_get("", "event_id")?,
        target_conversation_id: row.try_get("", "target_conversation_id")?,
        client_dedupe_id: row.try_get("", "client_dedupe_id")?,
        reason: row.try_get("", "reason")?,
        state,
        connection_id_snapshot: row.try_get("", "connection_id_snapshot")?,
        error: row.try_get("", "error")?,
        created_at: row.try_get::<DateTime<Utc>>("", "created_at")?,
        updated_at: row.try_get::<DateTime<Utc>>("", "updated_at")?,
    })
}

async fn operation_by_id<C: ConnectionTrait>(
    conn: &C,
    operation_id: &str,
) -> Result<CollaborationInterruptOperationView, DbError> {
    conn.query_one(statement(
        "SELECT id, event_id, target_conversation_id, client_dedupe_id, reason, state, \
                connection_id_snapshot, error, created_at, updated_at \
         FROM collaboration_interrupt_operation WHERE id = ?",
        vec![operation_id.into()],
    ))
    .await?
    .map(|row| parse_operation(&row))
    .transpose()?
    .ok_or_else(|| DbError::NotFound(format!("Collaboration interrupt {operation_id}")))
}

async fn bump_collaboration_participants(
    txn: &DatabaseTransaction,
    target_conversation_id: i32,
    event_id: &str,
) -> Result<(), DbError> {
    let Some(row) = txn
        .query_one(statement(
            "SELECT e.source_conversation_id FROM collaboration_event e \
             JOIN collaboration_delivery d ON d.event_id = e.id \
             WHERE d.event_id = ? AND d.target_conversation_id = ?",
            vec![event_id.into(), target_conversation_id.into()],
        ))
        .await?
    else {
        return Err(validation("Collaboration delivery is missing"));
    };
    let source_conversation_id: i32 = row.try_get("", "source_conversation_id")?;
    for conversation_id in [source_conversation_id, target_conversation_id] {
        txn.execute(statement(
            "INSERT OR IGNORE INTO conversation_collaboration_state \
             (conversation_id, revision, updated_at) \
             SELECT id, 0, CURRENT_TIMESTAMP FROM conversation \
             WHERE id = ? AND deleted_at IS NULL",
            vec![conversation_id.into()],
        ))
        .await?;
        txn.execute(statement(
            "UPDATE conversation_collaboration_state \
             SET revision = revision + 1, updated_at = CURRENT_TIMESTAMP \
             WHERE conversation_id = ?",
            vec![conversation_id.into()],
        ))
        .await?;
    }
    Ok(())
}

async fn clear_owned_pause(
    txn: &DatabaseTransaction,
    target_conversation_id: i32,
    operation_id: &str,
) -> Result<bool, DbError> {
    let changed = txn
        .execute(statement(
            "UPDATE conversation_prompt_queue_state \
             SET paused_reason = NULL, revision = revision + 1, updated_at = CURRENT_TIMESTAMP \
             WHERE conversation_id = ? AND paused_reason = ?",
            vec![
                target_conversation_id.into(),
                format!("{PAUSE_REASON_PREFIX}{operation_id}").into(),
            ],
        ))
        .await?
        .rows_affected()
        == 1;
    Ok(changed)
}

#[derive(Debug)]
pub struct InterruptPreparation {
    pub operation: CollaborationInterruptOperationView,
    pub deduplicated: bool,
    pub should_cancel: bool,
}

pub async fn prepare(
    conn: &DatabaseConnection,
    mut input: InterruptCollaborationInput,
    connection_id: &str,
    turn_active: bool,
) -> Result<InterruptPreparation, DbError> {
    input.event_id = input.event_id.trim().to_string();
    input.client_dedupe_id = input.client_dedupe_id.trim().to_string();
    input.reason = input.reason.trim().to_string();
    if input.event_id.is_empty() {
        return Err(validation("Collaboration event id cannot be empty"));
    }
    if input.client_dedupe_id.is_empty() || input.client_dedupe_id.len() > MAX_DEDUPE_ID_BYTES {
        return Err(validation("Interrupt dedupe id is invalid"));
    }
    if input.reason.is_empty() || input.reason.len() > MAX_REASON_BYTES {
        return Err(validation("Interrupt reason is invalid"));
    }

    let txn = conn.begin().await?;
    if let Some(row) = txn
        .query_one(statement(
            "SELECT id, event_id, target_conversation_id, client_dedupe_id, reason, state, \
                    connection_id_snapshot, error, created_at, updated_at \
             FROM collaboration_interrupt_operation \
             WHERE target_conversation_id = ? AND client_dedupe_id = ?",
            vec![
                input.target_conversation_id.into(),
                input.client_dedupe_id.clone().into(),
            ],
        ))
        .await?
    {
        let operation = parse_operation(&row)?;
        if operation.event_id != input.event_id || operation.reason != input.reason {
            return Err(validation(
                "Interrupt dedupe id was already used with different content",
            ));
        }
        txn.commit().await?;
        return Ok(InterruptPreparation {
            operation,
            deduplicated: true,
            should_cancel: false,
        });
    }

    let Some(row) = txn
        .query_one(statement(
            "SELECT d.state AS delivery_state, d.invocation_policy, q.id AS queue_item_id, \
                    q.state AS queue_item_state, s.paused_reason \
             FROM collaboration_delivery d \
             LEFT JOIN conversation_prompt_queue_item q \
               ON q.origin_event_id = d.event_id \
              AND q.conversation_id = d.target_conversation_id \
             LEFT JOIN conversation_prompt_queue_state s \
               ON s.conversation_id = d.target_conversation_id \
             WHERE d.event_id = ? AND d.target_conversation_id = ?",
            vec![
                input.event_id.clone().into(),
                input.target_conversation_id.into(),
            ],
        ))
        .await?
    else {
        return Err(validation("Collaboration delivery is missing"));
    };
    let invocation_policy: String = row.try_get("", "invocation_policy")?;
    if invocation_policy != "invoke_when_idle" {
        return Err(validation(
            "Only a queued collaboration message can stop the current task",
        ));
    }
    let delivery_state: String = row.try_get("", "delivery_state")?;
    let queue_item_id: Option<String> = row.try_get("", "queue_item_id")?;
    let queue_item_state: Option<String> = row.try_get("", "queue_item_state")?;
    let paused_reason: Option<String> = row.try_get("", "paused_reason")?;
    let operation_id = uuid::Uuid::new_v4().to_string();

    let state = match delivery_state.as_str() {
        "embedded" => CollaborationInterruptState::Completed,
        "embedding" => CollaborationInterruptState::Dispatching,
        "queued" => {
            let Some(queue_item_id) = queue_item_id.as_deref() else {
                return Err(validation("Queued collaboration message has no queue item"));
            };
            if queue_item_state.as_deref() != Some("queued") {
                return Err(validation(
                    "Collaboration message is not ready for an interrupt operation",
                ));
            }
            if paused_reason.is_some() {
                return Err(validation(
                    "Target Session queue is already paused for another reason",
                ));
            }
            txn.execute(statement(
                "UPDATE conversation_prompt_queue_item \
                 SET position = (SELECT COALESCE(MIN(position), 0) - 1 \
                                 FROM conversation_prompt_queue_item \
                                 WHERE conversation_id = ?), \
                     updated_at = CURRENT_TIMESTAMP \
                 WHERE id = ? AND conversation_id = ? AND state = 'queued'",
                vec![
                    input.target_conversation_id.into(),
                    queue_item_id.into(),
                    input.target_conversation_id.into(),
                ],
            ))
            .await?;
            if turn_active {
                txn.execute(statement(
                    "UPDATE conversation_prompt_queue_state \
                     SET paused_reason = ?, revision = revision + 1, updated_at = CURRENT_TIMESTAMP \
                     WHERE conversation_id = ? AND paused_reason IS NULL",
                    vec![
                        format!("{PAUSE_REASON_PREFIX}{operation_id}").into(),
                        input.target_conversation_id.into(),
                    ],
                ))
                .await?;
                CollaborationInterruptState::Cancelling
            } else {
                txn.execute(statement(
                    "UPDATE conversation_prompt_queue_state \
                     SET revision = revision + 1, updated_at = CURRENT_TIMESTAMP \
                     WHERE conversation_id = ?",
                    vec![input.target_conversation_id.into()],
                ))
                .await?;
                CollaborationInterruptState::Ready
            }
        }
        _ => {
            return Err(validation(
                "Collaboration message can no longer be interrupted into the target Session",
            ))
        }
    };
    let state_name = match state {
        CollaborationInterruptState::Completed => "completed",
        CollaborationInterruptState::Dispatching => "dispatching",
        CollaborationInterruptState::Cancelling => "cancelling",
        CollaborationInterruptState::Ready => "ready",
        _ => unreachable!(),
    };
    txn.execute(statement(
        "INSERT INTO collaboration_interrupt_operation \
         (id, event_id, target_conversation_id, client_dedupe_id, reason, state, \
          connection_id_snapshot, error, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, NULL, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        vec![
            operation_id.clone().into(),
            input.event_id.clone().into(),
            input.target_conversation_id.into(),
            input.client_dedupe_id.into(),
            input.reason.into(),
            state_name.into(),
            connection_id.into(),
        ],
    ))
    .await?;
    bump_collaboration_participants(&txn, input.target_conversation_id, &input.event_id).await?;
    txn.commit().await?;
    Ok(InterruptPreparation {
        operation: operation_by_id(conn, &operation_id).await?,
        deduplicated: false,
        should_cancel: state == CollaborationInterruptState::Cancelling,
    })
}

pub async fn mark_cancel_enqueued(
    conn: &DatabaseConnection,
    operation_id: &str,
) -> Result<CollaborationInterruptOperationView, DbError> {
    let txn = conn.begin().await?;
    let operation = operation_by_id(&txn, operation_id).await?;
    let next_state = match operation.state {
        CollaborationInterruptState::Cancelling => "waiting_for_terminal",
        CollaborationInterruptState::TerminalObserved => "ready",
        _ => {
            txn.commit().await?;
            return operation_by_id(conn, operation_id).await;
        }
    };
    txn.execute(statement(
        "UPDATE collaboration_interrupt_operation \
         SET state = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ? AND state = ?",
        vec![
            next_state.into(),
            operation_id.into(),
            match operation.state {
                CollaborationInterruptState::Cancelling => "cancelling",
                CollaborationInterruptState::TerminalObserved => "terminal_observed",
                _ => unreachable!(),
            }
            .into(),
        ],
    ))
    .await?;
    if next_state == "ready" {
        clear_owned_pause(&txn, operation.target_conversation_id, operation_id).await?;
    }
    bump_collaboration_participants(&txn, operation.target_conversation_id, &operation.event_id)
        .await?;
    txn.commit().await?;
    operation_by_id(conn, operation_id).await
}

pub async fn mark_cancel_failed(
    conn: &DatabaseConnection,
    operation_id: &str,
    reason: &str,
) -> Result<CollaborationInterruptOperationView, DbError> {
    let txn = conn.begin().await?;
    let operation = operation_by_id(&txn, operation_id).await?;
    txn.execute(statement(
        "UPDATE collaboration_interrupt_operation \
         SET state = 'failed', error = ?, updated_at = CURRENT_TIMESTAMP \
         WHERE id = ? AND state IN ('cancelling', 'terminal_observed')",
        vec![reason.into(), operation_id.into()],
    ))
    .await?;
    txn.execute(statement(
        "UPDATE conversation_prompt_queue_state \
         SET paused_reason = ?, revision = revision + 1, updated_at = CURRENT_TIMESTAMP \
         WHERE conversation_id = ? AND paused_reason = ?",
        vec![
            format!("{FAILED_PAUSE_REASON_PREFIX}{operation_id}").into(),
            operation.target_conversation_id.into(),
            format!("{PAUSE_REASON_PREFIX}{operation_id}").into(),
        ],
    ))
    .await?;
    bump_collaboration_participants(&txn, operation.target_conversation_id, &operation.event_id)
        .await?;
    txn.commit().await?;
    operation_by_id(conn, operation_id).await
}

#[derive(Debug)]
pub struct TerminalObservation {
    pub event_id: String,
    pub operation_id: String,
    pub became_ready: bool,
}

#[derive(Debug)]
pub struct InterruptRecovery {
    pub event_id: String,
    pub target_conversation_id: i32,
}

/// Recover operations whose in-memory command continuation disappeared. A
/// recorded terminal observation is sufficient evidence to release the saved
/// message. A stale `cancelling` row is deliberately failed/paused because the
/// process cannot know whether Cancel reached the Harness before it died.
pub async fn recover_after_process_loss(
    conn: &DatabaseConnection,
    stale_cancelling_before: DateTime<Utc>,
) -> Result<Vec<InterruptRecovery>, DbError> {
    let txn = conn.begin().await?;
    let rows = txn
        .query_all(statement(
            "SELECT id, event_id, target_conversation_id, state \
             FROM collaboration_interrupt_operation \
             WHERE state = 'terminal_observed' \
                OR (state = 'cancelling' AND updated_at <= ?) \
             ORDER BY created_at",
            vec![stale_cancelling_before.into()],
        ))
        .await?;
    let mut recovered = Vec::with_capacity(rows.len());
    for row in rows {
        let operation_id: String = row.try_get("", "id")?;
        let event_id: String = row.try_get("", "event_id")?;
        let target_conversation_id: i32 = row.try_get("", "target_conversation_id")?;
        let state: String = row.try_get("", "state")?;
        let became_ready = state == "terminal_observed";
        if became_ready {
            txn.execute(statement(
                "UPDATE collaboration_interrupt_operation \
                 SET state = 'ready', updated_at = CURRENT_TIMESTAMP \
                 WHERE id = ? AND state = 'terminal_observed'",
                vec![operation_id.clone().into()],
            ))
            .await?;
            clear_owned_pause(&txn, target_conversation_id, &operation_id).await?;
        } else {
            txn.execute(statement(
                "UPDATE collaboration_interrupt_operation \
                 SET state = 'failed', error = 'interrupt_outcome_unknown', \
                     updated_at = CURRENT_TIMESTAMP \
                 WHERE id = ? AND state = 'cancelling'",
                vec![operation_id.clone().into()],
            ))
            .await?;
            txn.execute(statement(
                "UPDATE conversation_prompt_queue_state \
                 SET paused_reason = ?, revision = revision + 1, updated_at = CURRENT_TIMESTAMP \
                 WHERE conversation_id = ? AND paused_reason = ?",
                vec![
                    format!("{FAILED_PAUSE_REASON_PREFIX}{operation_id}").into(),
                    target_conversation_id.into(),
                    format!("{PAUSE_REASON_PREFIX}{operation_id}").into(),
                ],
            ))
            .await?;
        }
        bump_collaboration_participants(&txn, target_conversation_id, &event_id).await?;
        recovered.push(InterruptRecovery {
            event_id,
            target_conversation_id,
        });
    }
    txn.commit().await?;
    Ok(recovered)
}

pub async fn waiting_target_ids(conn: &DatabaseConnection) -> Result<Vec<i32>, DbError> {
    conn.query_all(statement(
        "SELECT DISTINCT target_conversation_id \
         FROM collaboration_interrupt_operation \
         WHERE state = 'waiting_for_terminal' \
         ORDER BY target_conversation_id",
        Vec::new(),
    ))
    .await?
    .into_iter()
    .map(|row| {
        row.try_get::<i32>("", "target_conversation_id")
            .map_err(DbError::from)
    })
    .collect()
}

pub async fn observe_terminal(
    conn: &DatabaseConnection,
    target_conversation_id: i32,
) -> Result<Option<TerminalObservation>, DbError> {
    let txn = conn.begin().await?;
    let Some(row) = txn
        .query_one(statement(
            "SELECT id, event_id, state FROM collaboration_interrupt_operation \
             WHERE target_conversation_id = ? \
               AND state IN ('cancelling', 'waiting_for_terminal') \
             ORDER BY created_at LIMIT 1",
            vec![target_conversation_id.into()],
        ))
        .await?
    else {
        txn.commit().await?;
        return Ok(None);
    };
    let operation_id: String = row.try_get("", "id")?;
    let event_id: String = row.try_get("", "event_id")?;
    let state: String = row.try_get("", "state")?;
    let (next_state, became_ready) = if state == "cancelling" {
        ("terminal_observed", false)
    } else {
        ("ready", true)
    };
    txn.execute(statement(
        "UPDATE collaboration_interrupt_operation \
         SET state = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ? AND state = ?",
        vec![next_state.into(), operation_id.clone().into(), state.into()],
    ))
    .await?;
    if became_ready {
        clear_owned_pause(&txn, target_conversation_id, &operation_id).await?;
    }
    bump_collaboration_participants(&txn, target_conversation_id, &event_id).await?;
    txn.commit().await?;
    Ok(Some(TerminalObservation {
        event_id,
        operation_id,
        became_ready,
    }))
}

async fn transition_origin(
    txn: &DatabaseTransaction,
    target_conversation_id: i32,
    event_id: &str,
    from: &[&str],
    to: &str,
    error: Option<&str>,
) -> Result<bool, DbError> {
    let placeholders = std::iter::repeat_n("?", from.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "UPDATE collaboration_interrupt_operation SET state = ?, error = ?, \
         updated_at = CURRENT_TIMESTAMP WHERE event_id = ? AND target_conversation_id = ? \
         AND state IN ({placeholders})"
    );
    let mut values: Vec<sea_orm::Value> = vec![
        to.into(),
        error.map(ToOwned::to_owned).into(),
        event_id.into(),
        target_conversation_id.into(),
    ];
    values.extend(from.iter().map(|state| (*state).into()));
    let changed = txn.execute(statement(&sql, values)).await?.rows_affected() == 1;
    if changed {
        bump_collaboration_participants(txn, target_conversation_id, event_id).await?;
    }
    Ok(changed)
}

pub(crate) async fn mark_origin_dispatching(
    txn: &DatabaseTransaction,
    target_conversation_id: i32,
    event_id: &str,
) -> Result<bool, DbError> {
    transition_origin(
        txn,
        target_conversation_id,
        event_id,
        &["ready"],
        "dispatching",
        None,
    )
    .await
}

pub(crate) async fn mark_origin_ready(
    txn: &DatabaseTransaction,
    target_conversation_id: i32,
    event_id: &str,
) -> Result<bool, DbError> {
    transition_origin(
        txn,
        target_conversation_id,
        event_id,
        &["dispatching"],
        "ready",
        None,
    )
    .await
}

pub(crate) async fn mark_origin_completed(
    txn: &DatabaseTransaction,
    target_conversation_id: i32,
    event_id: &str,
) -> Result<bool, DbError> {
    transition_origin(
        txn,
        target_conversation_id,
        event_id,
        &["dispatching", "ready"],
        "completed",
        None,
    )
    .await
}

pub(crate) async fn mark_origin_failed(
    txn: &DatabaseTransaction,
    target_conversation_id: i32,
    event_id: &str,
    reason: &str,
) -> Result<bool, DbError> {
    transition_origin(
        txn,
        target_conversation_id,
        event_id,
        &["ready", "dispatching"],
        "failed",
        Some(reason),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    use crate::db::service::{collaboration_service, prompt_queue_service};
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::{
        AgentType, CollaborationDeliveryHint, CollaborationInvocationPolicy, CollaborationUrgency,
        SendCollaborationMessageInput,
    };

    async fn seeded_message(dedupe: &str) -> (crate::db::AppDatabase, i32, String) {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/codeg-interrupt").await;
        let source = seed_conversation(&db, folder_id, AgentType::Codex).await;
        let target = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let sent = collaboration_service::send(
            &db.conn,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                subject: "Test letter".into(),
                body: "Review the new evidence".to_string(),
                client_dedupe_id: dedupe.to_string(),
                invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .expect("send collaboration message");
        (db, target, sent.event_id)
    }

    fn input(event_id: &str, target: i32, dedupe: &str) -> InterruptCollaborationInput {
        InterruptCollaborationInput {
            event_id: event_id.to_string(),
            target_conversation_id: target,
            client_dedupe_id: dedupe.to_string(),
            reason: "User requested stop and send".to_string(),
        }
    }

    #[tokio::test]
    async fn terminal_before_cancel_ack_releases_only_after_ack() {
        let (db, target, event_id) = seeded_message("message-terminal-first").await;
        let prepared = prepare(
            &db.conn,
            input(&event_id, target, "interrupt-terminal-first"),
            "connection-a",
            true,
        )
        .await
        .expect("prepare interrupt");
        assert!(prepared.should_cancel);
        assert_eq!(
            prepared.operation.state,
            CollaborationInterruptState::Cancelling
        );
        let snapshot = prompt_queue_service::snapshot(&db.conn, target)
            .await
            .expect("paused snapshot");
        assert_eq!(
            snapshot.paused_reason.as_deref(),
            Some(format!("{PAUSE_REASON_PREFIX}{}", prepared.operation.id).as_str())
        );
        let resume_error = prompt_queue_service::resume_queue(&db.conn, target, snapshot.revision)
            .await
            .expect_err("an interrupt-owned pause cannot be released early");
        assert!(resume_error
            .to_string()
            .contains("waiting for the interrupted turn"));

        let observed = observe_terminal(&db.conn, target)
            .await
            .expect("observe terminal")
            .expect("active operation");
        assert!(!observed.became_ready);
        assert_eq!(
            operation_by_id(&db.conn, &prepared.operation.id)
                .await
                .expect("operation")
                .state,
            CollaborationInterruptState::TerminalObserved
        );
        assert!(prompt_queue_service::snapshot(&db.conn, target)
            .await
            .expect("still paused")
            .paused_reason
            .is_some());

        let ready = mark_cancel_enqueued(&db.conn, &prepared.operation.id)
            .await
            .expect("ack cancel");
        assert_eq!(ready.state, CollaborationInterruptState::Ready);
        assert!(prompt_queue_service::snapshot(&db.conn, target)
            .await
            .expect("released snapshot")
            .paused_reason
            .is_none());
    }

    #[tokio::test]
    async fn duplicate_request_cancels_once_and_dispatch_completes_operation() {
        let (db, target, event_id) = seeded_message("message-deduped").await;
        let request = input(&event_id, target, "interrupt-deduped");
        let first = prepare(&db.conn, request.clone(), "connection-b", true)
            .await
            .expect("first prepare");
        let duplicate = prepare(&db.conn, request, "connection-b", true)
            .await
            .expect("duplicate prepare");
        assert!(first.should_cancel);
        assert!(duplicate.deduplicated);
        assert!(!duplicate.should_cancel);
        assert_eq!(first.operation.id, duplicate.operation.id);

        let waiting = mark_cancel_enqueued(&db.conn, &first.operation.id)
            .await
            .expect("cancel enqueued");
        assert_eq!(
            waiting.state,
            CollaborationInterruptState::WaitingForTerminal
        );
        let observed = observe_terminal(&db.conn, target)
            .await
            .expect("observe terminal")
            .expect("active operation");
        assert!(observed.became_ready);

        let (claimed, _) = prompt_queue_service::claim_head(
            &db.conn,
            target,
            "interrupt-test-worker",
            Duration::seconds(30),
        )
        .await
        .expect("claim")
        .expect("queue head");
        assert_eq!(claimed.origin_event_id.as_deref(), Some(event_id.as_str()));
        assert!(prompt_queue_service::mark_dispatch_started(
            &db.conn,
            &claimed,
            Duration::seconds(30),
        )
        .await
        .expect("mark dispatch"));
        assert_eq!(
            operation_by_id(&db.conn, &first.operation.id)
                .await
                .expect("dispatching operation")
                .state,
            CollaborationInterruptState::Dispatching
        );
        prompt_queue_service::accept_claim(&db.conn, &claimed)
            .await
            .expect("accept");
        assert_eq!(
            operation_by_id(&db.conn, &first.operation.id)
                .await
                .expect("completed operation")
                .state,
            CollaborationInterruptState::Completed
        );
        let feed = collaboration_service::feed(&db.conn, target, None)
            .await
            .expect("feed");
        assert_eq!(
            feed.inbound[0].interrupt_state,
            Some(CollaborationInterruptState::Completed)
        );
    }

    #[tokio::test]
    async fn cancel_failure_keeps_message_durably_paused() {
        let (db, target, event_id) = seeded_message("message-cancel-failure").await;
        let prepared = prepare(
            &db.conn,
            input(&event_id, target, "interrupt-cancel-failure"),
            "connection-c",
            true,
        )
        .await
        .expect("prepare interrupt");
        let failed = mark_cancel_failed(&db.conn, &prepared.operation.id, "process exited")
            .await
            .expect("persist failure");
        assert_eq!(failed.state, CollaborationInterruptState::Failed);
        assert_eq!(failed.error.as_deref(), Some("process exited"));
        let snapshot = prompt_queue_service::snapshot(&db.conn, target)
            .await
            .expect("snapshot");
        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(
            snapshot.paused_reason.as_deref(),
            Some(format!("{FAILED_PAUSE_REASON_PREFIX}{}", prepared.operation.id).as_str())
        );
    }

    #[tokio::test]
    async fn process_loss_never_replays_an_ambiguous_cancel() {
        let (db, target, event_id) = seeded_message("message-process-loss").await;
        let prepared = prepare(
            &db.conn,
            input(&event_id, target, "interrupt-process-loss"),
            "connection-d",
            true,
        )
        .await
        .expect("prepare interrupt");
        db.conn
            .execute(statement(
                "UPDATE collaboration_interrupt_operation \
                 SET updated_at = '2000-01-01 00:00:00' WHERE id = ?",
                vec![prepared.operation.id.clone().into()],
            ))
            .await
            .expect("age operation");

        let recovered = recover_after_process_loss(&db.conn, Utc::now())
            .await
            .expect("recover process loss");
        assert_eq!(recovered.len(), 1);
        let operation = operation_by_id(&db.conn, &prepared.operation.id)
            .await
            .expect("operation");
        assert_eq!(operation.state, CollaborationInterruptState::Failed);
        assert_eq!(
            operation.error.as_deref(),
            Some("interrupt_outcome_unknown")
        );
        let snapshot = prompt_queue_service::snapshot(&db.conn, target)
            .await
            .expect("snapshot");
        assert_eq!(snapshot.items.len(), 1);
        assert!(snapshot
            .paused_reason
            .unwrap()
            .starts_with(FAILED_PAUSE_REASON_PREFIX));
    }
}
