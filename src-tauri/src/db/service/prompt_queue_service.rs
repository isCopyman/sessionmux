use std::collections::HashSet;

use chrono::{DateTime, Duration, Utc};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, EntityTrait, QueryResult,
    Statement, TransactionTrait,
};

use crate::db::entities::conversation;
use crate::db::error::DbError;
use crate::models::prompt_queue::{
    ClaimedPromptQueueItem, EnqueuePromptQueueItem, PromptQueueDraft, PromptQueueItem,
    PromptQueueItemState, PromptQueueSnapshot,
};

const MAX_QUEUE_ITEMS: usize = 1_000;
const MAX_DISPLAY_TEXT_BYTES: usize = 1_000_000;
const UNKNOWN_DISPATCH_REASON: &str = "dispatch_outcome_unknown";
pub const SESSION_COLLABORATION_ENABLED_KEY: &str = "session_collaboration.enabled";
pub const COLLABORATION_DISABLED_REASON: &str = "session_collaboration_disabled";

#[derive(Debug)]
pub(crate) struct CollaborationQueuePolicyChange {
    pub snapshot: PromptQueueSnapshot,
    pub origin_event_ids: Vec<String>,
}

fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, values)
}

fn validation(message: impl Into<String>) -> DbError {
    DbError::Validation(message.into())
}

fn parse_timestamp(row: &QueryResult, column: &str) -> Result<DateTime<Utc>, DbError> {
    row.try_get("", column).map_err(DbError::from)
}

fn parse_item(row: &QueryResult) -> Result<PromptQueueItem, DbError> {
    let state_raw: String = row.try_get("", "state")?;
    let state = PromptQueueItemState::parse(&state_raw)
        .ok_or_else(|| validation(format!("Unknown prompt queue state: {state_raw}")))?;
    let draft_json: Option<String> = row.try_get("", "draft_json")?;
    let draft = draft_json
        .map(|value| {
            serde_json::from_str::<PromptQueueDraft>(&value)
                .map_err(|err| validation(format!("Invalid queued prompt payload: {err}")))
        })
        .transpose()?;
    Ok(PromptQueueItem {
        id: row.try_get("", "id")?,
        conversation_id: row.try_get("", "conversation_id")?,
        position: row.try_get("", "position")?,
        draft,
        origin_event_id: row.try_get("", "origin_event_id")?,
        mode_id: row.try_get("", "mode_id")?,
        state,
        client_dedupe_id: row.try_get("", "client_dedupe_id")?,
        attempts: row.try_get("", "attempts")?,
        paused_reason: row.try_get("", "paused_reason")?,
        created_at: parse_timestamp(row, "created_at")?,
        updated_at: parse_timestamp(row, "updated_at")?,
    })
}

fn validate_draft(draft: &PromptQueueDraft) -> Result<(), DbError> {
    if draft.blocks.is_empty() {
        return Err(validation(
            "A queued prompt must contain at least one content block",
        ));
    }
    if draft.display_text.len() > MAX_DISPLAY_TEXT_BYTES {
        return Err(validation("Queued prompt display text is too large"));
    }
    Ok(())
}

fn validate_id(label: &str, value: &str) -> Result<(), DbError> {
    let value = value.trim();
    if value.is_empty() || value.len() > 200 {
        return Err(validation(format!(
            "{label} must contain between 1 and 200 bytes"
        )));
    }
    Ok(())
}

async fn ensure_conversation<C: ConnectionTrait>(
    conn: &C,
    conversation_id: i32,
) -> Result<(), DbError> {
    let exists = conversation::Entity::find_by_id(conversation_id)
        .one(conn)
        .await?
        .is_some_and(|row| row.deleted_at.is_none());
    if !exists {
        return Err(DbError::NotFound(format!("Conversation {conversation_id}")));
    }
    Ok(())
}

async fn ensure_state(txn: &DatabaseTransaction, conversation_id: i32) -> Result<(), DbError> {
    txn.execute(statement(
        "INSERT OR IGNORE INTO conversation_prompt_queue_state \
         (conversation_id, revision, paused_reason, updated_at) \
         VALUES (?, 0, NULL, CURRENT_TIMESTAMP)",
        vec![conversation_id.into()],
    ))
    .await?;
    Ok(())
}

async fn state_row<C: ConnectionTrait>(
    conn: &C,
    conversation_id: i32,
) -> Result<(i64, Option<String>), DbError> {
    let row = conn
        .query_one(statement(
            "SELECT revision, paused_reason FROM conversation_prompt_queue_state \
             WHERE conversation_id = ?",
            vec![conversation_id.into()],
        ))
        .await?;
    Ok(match row {
        Some(row) => (
            row.try_get("", "revision")?,
            row.try_get("", "paused_reason")?,
        ),
        None => (0, None),
    })
}

async fn verify_revision(
    txn: &DatabaseTransaction,
    conversation_id: i32,
    expected_revision: i64,
) -> Result<(), DbError> {
    let (revision, _) = state_row(txn, conversation_id).await?;
    if revision != expected_revision {
        return Err(validation(format!(
            "Prompt queue revision conflict: expected {expected_revision}, current {revision}"
        )));
    }
    Ok(())
}

async fn bump_revision(txn: &DatabaseTransaction, conversation_id: i32) -> Result<(), DbError> {
    txn.execute(statement(
        "UPDATE conversation_prompt_queue_state \
         SET revision = revision + 1, updated_at = CURRENT_TIMESTAMP \
         WHERE conversation_id = ?",
        vec![conversation_id.into()],
    ))
    .await?;
    Ok(())
}

async fn snapshot_on<C: ConnectionTrait>(
    conn: &C,
    conversation_id: i32,
) -> Result<PromptQueueSnapshot, DbError> {
    ensure_conversation(conn, conversation_id).await?;
    let (revision, paused_reason) = state_row(conn, conversation_id).await?;
    let rows = conn
        .query_all(statement(
            "SELECT id, conversation_id, position, draft_json, origin_event_id, mode_id, \
                    state, client_dedupe_id, attempts, paused_reason, created_at, updated_at \
             FROM conversation_prompt_queue_item WHERE conversation_id = ? \
             ORDER BY position ASC, created_at ASC, id ASC",
            vec![conversation_id.into()],
        ))
        .await?;
    let items = rows.iter().map(parse_item).collect::<Result<Vec<_>, _>>()?;
    Ok(PromptQueueSnapshot {
        conversation_id,
        revision,
        paused_reason,
        items,
    })
}

pub async fn snapshot(
    conn: &DatabaseConnection,
    conversation_id: i32,
) -> Result<PromptQueueSnapshot, DbError> {
    snapshot_on(conn, conversation_id).await
}

/// Whether the current FIFO head is a collaboration `invoke_when_idle`
/// delivery. Those items force-deliver: inject if native steering exists,
/// otherwise cancel the current turn.
pub(crate) async fn head_is_collaboration_invoke<C: ConnectionTrait>(
    conn: &C,
    conversation_id: i32,
) -> Result<bool, DbError> {
    let row = conn
        .query_one(statement(
            "SELECT d.id FROM conversation_prompt_queue_item q \
             JOIN collaboration_delivery d \
               ON d.event_id = q.origin_event_id \
              AND d.target_conversation_id = q.conversation_id \
             WHERE q.id = ( \
               SELECT id FROM conversation_prompt_queue_item \
               WHERE conversation_id = ? AND state = 'queued' \
               ORDER BY position ASC, created_at ASC, id ASC LIMIT 1 \
             ) AND q.conversation_id = ? AND q.state = 'queued' \
               AND d.invocation_policy = 'invoke_when_idle' \
               AND NOT EXISTS ( \
                 SELECT 1 FROM conversation_prompt_queue_state s \
                 WHERE s.conversation_id = ? AND s.paused_reason IS NOT NULL \
               )",
            vec![
                conversation_id.into(),
                conversation_id.into(),
                conversation_id.into(),
            ],
        ))
        .await?;
    Ok(row.is_some())
}

pub(crate) async fn collaboration_dispatch_enabled<C: ConnectionTrait>(
    conn: &C,
) -> Result<bool, DbError> {
    let raw = crate::db::service::app_metadata_service::get_value_conn(
        conn,
        SESSION_COLLABORATION_ENABLED_KEY,
    )
    .await?;
    Ok(raw
        .as_deref()
        .and_then(|value| value.parse::<bool>().ok())
        .unwrap_or(true))
}

/// Converge durable cross-Session queue items with the live collaboration
/// policy. Disabling freezes only collaboration-origin items that have not
/// crossed the dispatch boundary; ordinary user follow-ups remain runnable.
/// Enabling thaws only items frozen by this policy, preserving failures and
/// explicit confirmation pauses.
pub(crate) async fn reconcile_collaboration_dispatch_policy(
    conn: &DatabaseConnection,
    enabled: bool,
) -> Result<Vec<CollaborationQueuePolicyChange>, DbError> {
    let txn = conn.begin().await?;
    let rows = if enabled {
        txn.query_all(statement(
            "SELECT conversation_id, origin_event_id FROM conversation_prompt_queue_item \
             WHERE origin_event_id IS NOT NULL AND state = 'paused' AND paused_reason = ? \
             ORDER BY conversation_id ASC, position ASC, created_at ASC, id ASC",
            vec![COLLABORATION_DISABLED_REASON.into()],
        ))
        .await?
    } else {
        txn.query_all(statement(
            "SELECT conversation_id, origin_event_id FROM conversation_prompt_queue_item \
             WHERE origin_event_id IS NOT NULL AND (state = 'queued' OR \
               (state = 'claimed' AND dispatch_started_at IS NULL)) \
             ORDER BY conversation_id ASC, position ASC, created_at ASC, id ASC",
            Vec::new(),
        ))
        .await?
    };
    if rows.is_empty() {
        txn.commit().await?;
        return Ok(Vec::new());
    }

    let mut origins_by_conversation = std::collections::BTreeMap::<i32, Vec<String>>::new();
    for row in rows {
        origins_by_conversation
            .entry(row.try_get("", "conversation_id")?)
            .or_default()
            .push(row.try_get("", "origin_event_id")?);
    }

    for conversation_id in origins_by_conversation.keys().copied() {
        ensure_state(&txn, conversation_id).await?;
        let result = if enabled {
            txn.execute(statement(
                "UPDATE conversation_prompt_queue_item \
                 SET state = 'queued', paused_reason = NULL, claimed_by = NULL, \
                     claim_expires_at = NULL, dispatch_started_at = NULL, \
                     updated_at = CURRENT_TIMESTAMP \
                 WHERE conversation_id = ? AND origin_event_id IS NOT NULL \
                   AND state = 'paused' AND paused_reason = ?",
                vec![conversation_id.into(), COLLABORATION_DISABLED_REASON.into()],
            ))
            .await?
        } else {
            txn.execute(statement(
                "UPDATE conversation_prompt_queue_item \
                 SET state = 'paused', paused_reason = ?, claimed_by = NULL, \
                     claim_expires_at = NULL, dispatch_started_at = NULL, \
                     updated_at = CURRENT_TIMESTAMP \
                 WHERE conversation_id = ? AND origin_event_id IS NOT NULL AND \
                   (state = 'queued' OR (state = 'claimed' AND dispatch_started_at IS NULL))",
                vec![COLLABORATION_DISABLED_REASON.into(), conversation_id.into()],
            ))
            .await?
        };
        if result.rows_affected() > 0 {
            bump_revision(&txn, conversation_id).await?;
        }
    }
    txn.commit().await?;

    let mut changes = Vec::with_capacity(origins_by_conversation.len());
    for (conversation_id, origin_event_ids) in origins_by_conversation {
        changes.push(CollaborationQueuePolicyChange {
            snapshot: snapshot_on(conn, conversation_id).await?,
            origin_event_ids,
        });
    }
    Ok(changes)
}

/// Admission guard shared by ordinary sends and the queue worker. When a
/// durable queue exists, no ad-hoc prompt may jump its FIFO head. The worker is
/// admitted only after it has claimed that exact stable message id.
pub(crate) async fn send_is_admitted(
    conn: &DatabaseConnection,
    conversation_id: i32,
    client_message_id: Option<&str>,
) -> Result<bool, DbError> {
    let row = conn
        .query_one(statement(
            "SELECT q.id, q.state, s.paused_reason \
             FROM conversation_prompt_queue_item q \
             LEFT JOIN conversation_prompt_queue_state s ON s.conversation_id = q.conversation_id \
             WHERE q.conversation_id = ? \
             ORDER BY q.position ASC, q.created_at ASC, q.id ASC LIMIT 1",
            vec![conversation_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(true);
    };
    let id: String = row.try_get("", "id")?;
    let state: String = row.try_get("", "state")?;
    let paused_reason: Option<String> = row.try_get("", "paused_reason")?;
    Ok(paused_reason.is_none() && state == "claimed" && client_message_id == Some(id.as_str()))
}

pub async fn enqueue(
    conn: &DatabaseConnection,
    input: EnqueuePromptQueueItem,
) -> Result<PromptQueueSnapshot, DbError> {
    validate_id("Queue item id", &input.id)?;
    validate_id("Queue dedupe id", &input.client_dedupe_id)?;
    validate_draft(&input.draft)?;
    ensure_conversation(conn, input.conversation_id).await?;
    let draft_json = serde_json::to_string(&input.draft)
        .map_err(|err| validation(format!("Could not serialize queued prompt: {err}")))?;

    let txn = conn.begin().await?;
    ensure_state(&txn, input.conversation_id).await?;

    // A client may retry after losing the response. The dedupe key makes that
    // retry a read, not a second queued prompt and not a revision bump.
    let duplicate = txn
        .query_one(statement(
            "SELECT id FROM conversation_prompt_queue_item \
             WHERE conversation_id = ? AND client_dedupe_id = ?",
            vec![
                input.conversation_id.into(),
                input.client_dedupe_id.clone().into(),
            ],
        ))
        .await?;
    if duplicate.is_some() {
        txn.commit().await?;
        return snapshot_on(conn, input.conversation_id).await;
    }

    let count: i64 = txn
        .query_one(statement(
            "SELECT COUNT(*) AS count FROM conversation_prompt_queue_item \
             WHERE conversation_id = ?",
            vec![input.conversation_id.into()],
        ))
        .await?
        .expect("COUNT always returns one row")
        .try_get("", "count")?;
    if count >= MAX_QUEUE_ITEMS as i64 {
        return Err(validation(format!(
            "A Session queue can contain at most {MAX_QUEUE_ITEMS} items"
        )));
    }

    // This write obtains SQLite's writer lock before the position read. Two
    // clients appending concurrently therefore serialize and both survive.
    bump_revision(&txn, input.conversation_id).await?;
    let position: i32 = txn
        .query_one(statement(
            "SELECT COALESCE(MAX(position), -1) + 1 AS next_position \
             FROM conversation_prompt_queue_item WHERE conversation_id = ?",
            vec![input.conversation_id.into()],
        ))
        .await?
        .expect("aggregate always returns one row")
        .try_get("", "next_position")?;
    txn.execute(statement(
        "INSERT INTO conversation_prompt_queue_item \
         (id, conversation_id, position, draft_json, origin_event_id, mode_id, state, \
          client_dedupe_id, attempts, created_at, updated_at) \
         VALUES (?, ?, ?, ?, NULL, ?, 'queued', ?, 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        vec![
            input.id.into(),
            input.conversation_id.into(),
            position.into(),
            draft_json.into(),
            input.mode_id.into(),
            input.client_dedupe_id.into(),
        ],
    ))
    .await?;
    txn.commit().await?;
    snapshot_on(conn, input.conversation_id).await
}

/// Materialize one cross-Session delivery into the same durable FIFO used by
/// ordinary follow-ups. The row stores only an immutable event reference; its
/// transcript envelope is resolved at claim time. The caller owns `txn`, so
/// event, delivery, and queue admission either all commit or none do.
pub(crate) async fn enqueue_origin_in_transaction(
    txn: &DatabaseTransaction,
    conversation_id: i32,
    item_id: &str,
    origin_event_id: &str,
    client_dedupe_id: &str,
    initial_pause_reason: Option<&str>,
) -> Result<bool, DbError> {
    validate_id("Queue item id", item_id)?;
    validate_id("Queue origin event id", origin_event_id)?;
    validate_id("Queue dedupe id", client_dedupe_id)?;
    ensure_conversation(txn, conversation_id).await?;
    ensure_state(txn, conversation_id).await?;

    let duplicate = txn
        .query_one(statement(
            "SELECT id FROM conversation_prompt_queue_item \
             WHERE conversation_id = ? AND client_dedupe_id = ?",
            vec![conversation_id.into(), client_dedupe_id.into()],
        ))
        .await?;
    if duplicate.is_some() {
        return Ok(false);
    }

    let count: i64 = txn
        .query_one(statement(
            "SELECT COUNT(*) AS count FROM conversation_prompt_queue_item \
             WHERE conversation_id = ?",
            vec![conversation_id.into()],
        ))
        .await?
        .expect("COUNT always returns one row")
        .try_get("", "count")?;
    if count >= MAX_QUEUE_ITEMS as i64 {
        return Err(validation(format!(
            "A Session queue can contain at most {MAX_QUEUE_ITEMS} items"
        )));
    }

    bump_revision(txn, conversation_id).await?;
    let position: i32 = txn
        .query_one(statement(
            "SELECT COALESCE(MAX(position), -1) + 1 AS next_position \
             FROM conversation_prompt_queue_item WHERE conversation_id = ?",
            vec![conversation_id.into()],
        ))
        .await?
        .expect("aggregate always returns one row")
        .try_get("", "next_position")?;
    let (state, paused_reason) = match initial_pause_reason {
        Some(reason) => ("paused", Some(reason)),
        None => ("queued", None),
    };
    txn.execute(statement(
        "INSERT INTO conversation_prompt_queue_item \
         (id, conversation_id, position, draft_json, origin_event_id, mode_id, state, \
          client_dedupe_id, attempts, paused_reason, created_at, updated_at) \
         VALUES (?, ?, ?, NULL, ?, NULL, ?, ?, 0, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        vec![
            item_id.into(),
            conversation_id.into(),
            position.into(),
            origin_event_id.into(),
            state.into(),
            client_dedupe_id.into(),
            paused_reason.into(),
        ],
    ))
    .await?;
    Ok(true)
}

pub async fn edit(
    conn: &DatabaseConnection,
    conversation_id: i32,
    id: &str,
    draft: PromptQueueDraft,
    expected_revision: i64,
) -> Result<PromptQueueSnapshot, DbError> {
    validate_draft(&draft)?;
    let draft_json = serde_json::to_string(&draft)
        .map_err(|err| validation(format!("Could not serialize queued prompt: {err}")))?;
    let txn = conn.begin().await?;
    ensure_state(&txn, conversation_id).await?;
    verify_revision(&txn, conversation_id, expected_revision).await?;
    let result = txn
        .execute(statement(
            "UPDATE conversation_prompt_queue_item \
             SET draft_json = ?, updated_at = CURRENT_TIMESTAMP \
             WHERE id = ? AND conversation_id = ? AND state <> 'claimed' \
               AND draft_json IS NOT NULL",
            vec![draft_json.into(), id.into(), conversation_id.into()],
        ))
        .await?;
    if result.rows_affected() != 1 {
        return Err(validation(
            "Queued prompt is missing or currently being dispatched",
        ));
    }
    bump_revision(&txn, conversation_id).await?;
    txn.commit().await?;
    snapshot_on(conn, conversation_id).await
}

pub async fn delete(
    conn: &DatabaseConnection,
    conversation_id: i32,
    id: &str,
    expected_revision: i64,
) -> Result<PromptQueueSnapshot, DbError> {
    let txn = conn.begin().await?;
    ensure_state(&txn, conversation_id).await?;
    verify_revision(&txn, conversation_id, expected_revision).await?;
    let result = txn
        .execute(statement(
            "DELETE FROM conversation_prompt_queue_item \
             WHERE id = ? AND conversation_id = ? AND state <> 'claimed' \
               AND draft_json IS NOT NULL",
            vec![id.into(), conversation_id.into()],
        ))
        .await?;
    if result.rows_affected() != 1 {
        return Err(validation(
            "Queued prompt is missing or currently being dispatched",
        ));
    }
    bump_revision(&txn, conversation_id).await?;
    txn.commit().await?;
    snapshot_on(conn, conversation_id).await
}

pub async fn reorder(
    conn: &DatabaseConnection,
    conversation_id: i32,
    ordered_ids: Vec<String>,
    expected_revision: i64,
) -> Result<PromptQueueSnapshot, DbError> {
    let txn = conn.begin().await?;
    ensure_state(&txn, conversation_id).await?;
    verify_revision(&txn, conversation_id, expected_revision).await?;
    let rows = txn
        .query_all(statement(
            "SELECT id, state, draft_json, position FROM conversation_prompt_queue_item \
             WHERE conversation_id = ? ORDER BY position ASC, created_at ASC, id ASC",
            vec![conversation_id.into()],
        ))
        .await?;
    let existing = rows
        .iter()
        .filter(|row| {
            row.try_get::<Option<String>>("", "draft_json")
                .is_ok_and(|draft| draft.is_some())
        })
        .map(|row| row.try_get::<String>("", "id"))
        .collect::<Result<Vec<_>, _>>()?;
    let ordinary_positions = rows
        .iter()
        .filter_map(|row| {
            row.try_get::<Option<String>>("", "draft_json")
                .ok()
                .flatten()
                .and_then(|_| row.try_get::<i32>("", "position").ok())
        })
        .collect::<Vec<_>>();
    if rows.iter().any(|row| {
        row.try_get::<String>("", "state")
            .is_ok_and(|state| state == "claimed")
    }) {
        return Err(validation(
            "The queue changed while a prompt was being dispatched",
        ));
    }
    let requested: HashSet<&str> = ordered_ids.iter().map(String::as_str).collect();
    let live: HashSet<&str> = existing.iter().map(String::as_str).collect();
    if ordered_ids.len() != existing.len()
        || requested.len() != ordered_ids.len()
        || requested != live
    {
        return Err(validation(
            "Queue order must contain every queued prompt exactly once",
        ));
    }
    for (position, id) in ordinary_positions.into_iter().zip(ordered_ids.iter()) {
        txn.execute(statement(
            "UPDATE conversation_prompt_queue_item SET position = ?, updated_at = CURRENT_TIMESTAMP \
             WHERE id = ? AND conversation_id = ? AND draft_json IS NOT NULL",
            vec![position.into(), id.clone().into(), conversation_id.into()],
        ))
        .await?;
    }
    bump_revision(&txn, conversation_id).await?;
    txn.commit().await?;
    snapshot_on(conn, conversation_id).await
}

pub async fn pause_queue(
    conn: &DatabaseConnection,
    conversation_id: i32,
    reason: String,
) -> Result<PromptQueueSnapshot, DbError> {
    let reason = reason.trim().to_string();
    if reason.is_empty() {
        return Err(validation("Queue pause reason cannot be empty"));
    }
    let txn = conn.begin().await?;
    ensure_conversation(&txn, conversation_id).await?;
    ensure_state(&txn, conversation_id).await?;
    txn.execute(statement(
        "UPDATE conversation_prompt_queue_state \
         SET paused_reason = ?, revision = revision + 1, updated_at = CURRENT_TIMESTAMP \
         WHERE conversation_id = ?",
        vec![reason.into(), conversation_id.into()],
    ))
    .await?;
    txn.commit().await?;
    snapshot_on(conn, conversation_id).await
}

/// Pause a Session queue only when it actually has pending work. Cancellation
/// calls this after sending the Harness cancel command: an empty queue should
/// not gain invisible state or bump its revision merely because a turn was
/// stopped.
pub async fn pause_queue_if_pending(
    conn: &DatabaseConnection,
    conversation_id: i32,
    reason: String,
) -> Result<Option<PromptQueueSnapshot>, DbError> {
    let reason = reason.trim().to_string();
    if reason.is_empty() {
        return Err(validation("Queue pause reason cannot be empty"));
    }
    let txn = conn.begin().await?;
    ensure_conversation(&txn, conversation_id).await?;
    let count: i64 = txn
        .query_one(statement(
            "SELECT COUNT(*) AS count FROM conversation_prompt_queue_item \
             WHERE conversation_id = ?",
            vec![conversation_id.into()],
        ))
        .await?
        .expect("COUNT always returns one row")
        .try_get("", "count")?;
    if count == 0 {
        txn.commit().await?;
        return Ok(None);
    }
    ensure_state(&txn, conversation_id).await?;
    txn.execute(statement(
        "UPDATE conversation_prompt_queue_state \
         SET paused_reason = ?, revision = revision + 1, updated_at = CURRENT_TIMESTAMP \
         WHERE conversation_id = ?",
        vec![reason.into(), conversation_id.into()],
    ))
    .await?;
    txn.commit().await?;
    Ok(Some(snapshot_on(conn, conversation_id).await?))
}

pub async fn resume_queue(
    conn: &DatabaseConnection,
    conversation_id: i32,
    expected_revision: i64,
) -> Result<PromptQueueSnapshot, DbError> {
    let txn = conn.begin().await?;
    ensure_state(&txn, conversation_id).await?;
    verify_revision(&txn, conversation_id, expected_revision).await?;
    let (_, paused_reason) = state_row(&txn, conversation_id).await?;
    if paused_reason.as_deref().is_some_and(|reason| {
        reason.starts_with(crate::db::service::collaboration_interrupt_service::PAUSE_REASON_PREFIX)
    }) {
        return Err(validation(
            "Queue is waiting for the interrupted turn to finish",
        ));
    }
    txn.execute(statement(
        "UPDATE conversation_prompt_queue_state \
         SET paused_reason = NULL, revision = revision + 1, updated_at = CURRENT_TIMESTAMP \
         WHERE conversation_id = ?",
        vec![conversation_id.into()],
    ))
    .await?;
    txn.commit().await?;
    snapshot_on(conn, conversation_id).await
}

pub async fn retry_item(
    conn: &DatabaseConnection,
    conversation_id: i32,
    id: &str,
    expected_revision: i64,
) -> Result<PromptQueueSnapshot, DbError> {
    let txn = conn.begin().await?;
    ensure_state(&txn, conversation_id).await?;
    verify_revision(&txn, conversation_id, expected_revision).await?;
    let row = txn
        .query_one(statement(
            "SELECT origin_event_id, paused_reason \
             FROM conversation_prompt_queue_item \
             WHERE id = ? AND conversation_id = ? AND state = 'paused'",
            vec![id.into(), conversation_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Err(validation("Queued prompt is not paused"));
    };
    let origin_event_id: Option<String> = row.try_get("", "origin_event_id")?;
    let paused_reason: Option<String> = row.try_get("", "paused_reason")?;
    let (next_id, next_dedupe_id) = if let Some(event_id) = origin_event_id.as_deref() {
        let allow_queued = paused_reason.as_deref()
            == Some(crate::db::service::collaboration_service::INACTIVE_TARGET_CONFIRMATION_REASON);
        let identity = crate::db::service::collaboration_service::prepare_origin_retry_with_queued_confirmation(
            &txn,
            conversation_id,
            event_id,
            allow_queued,
        )
        .await?;
        let Some(identity) = identity else {
            return Err(validation("Collaboration delivery is not retryable"));
        };
        (identity.queue_item_id, identity.queue_dedupe_id)
    } else {
        let retry_id = uuid::Uuid::new_v4().to_string();
        (retry_id.clone(), retry_id)
    };
    let changed = txn
        .execute(statement(
            "UPDATE conversation_prompt_queue_item \
             SET id = ?, client_dedupe_id = ?, state = 'queued', paused_reason = NULL, \
                 claimed_by = NULL, claim_expires_at = NULL, dispatch_started_at = NULL, \
                 updated_at = CURRENT_TIMESTAMP \
             WHERE id = ? AND conversation_id = ? AND state = 'paused'",
            vec![
                next_id.into(),
                next_dedupe_id.into(),
                id.into(),
                conversation_id.into(),
            ],
        ))
        .await?;
    if changed.rows_affected() != 1 {
        return Err(validation("Queued prompt changed before retry"));
    }
    txn.execute(statement(
        "UPDATE conversation_prompt_queue_state \
         SET paused_reason = NULL, updated_at = CURRENT_TIMESTAMP \
         WHERE conversation_id = ?",
        vec![conversation_id.into()],
    ))
    .await?;
    bump_revision(&txn, conversation_id).await?;
    txn.commit().await?;
    snapshot_on(conn, conversation_id).await
}

pub(crate) async fn claim_head(
    conn: &DatabaseConnection,
    conversation_id: i32,
    worker_id: &str,
    lease: Duration,
) -> Result<Option<(ClaimedPromptQueueItem, PromptQueueSnapshot)>, DbError> {
    let txn = conn.begin().await?;
    ensure_state(&txn, conversation_id).await?;
    let (_, paused_reason) = state_row(&txn, conversation_id).await?;
    if paused_reason.is_some() {
        txn.commit().await?;
        return Ok(None);
    }
    let expires_at = Utc::now() + lease;
    // Select + claim in one SQLite write statement. A second backend process
    // can race this worker, but it cannot observe the same head as claimable
    // after this statement obtains the writer lock. This is the database-side
    // equivalent of Codex Desktop's per-message acquire lock.
    let Some(row) = txn
        .query_one(statement(
            "UPDATE conversation_prompt_queue_item \
             SET state = 'claimed', claimed_by = ?, claim_expires_at = ?, \
                 dispatch_started_at = NULL, attempts = attempts + 1, \
                 updated_at = CURRENT_TIMESTAMP \
             WHERE id = ( \
                 SELECT id FROM conversation_prompt_queue_item \
                 WHERE conversation_id = ? AND state = 'queued' \
                 ORDER BY position ASC, created_at ASC, id ASC LIMIT 1 \
             ) AND conversation_id = ? AND state = 'queued' \
             RETURNING id, conversation_id, draft_json, origin_event_id, mode_id",
            vec![
                worker_id.into(),
                expires_at.into(),
                conversation_id.into(),
                conversation_id.into(),
            ],
        ))
        .await?
    else {
        txn.commit().await?;
        return Ok(None);
    };
    let id: String = row.try_get("", "id")?;
    let draft_json: Option<String> = row.try_get("", "draft_json")?;
    let origin_event_id: Option<String> = row.try_get("", "origin_event_id")?;
    let draft = match (draft_json, origin_event_id.as_deref()) {
        (Some(draft_json), None) => serde_json::from_str(&draft_json)
            .map_err(|err| validation(format!("Invalid queued prompt payload: {err}")))?,
        (None, Some(event_id)) => {
            crate::db::service::collaboration_service::prompt_draft_for_origin(
                &txn,
                conversation_id,
                event_id,
            )
            .await?
        }
        _ => {
            return Err(validation(
                "Queued prompt must contain exactly one of draft_json or origin_event_id",
            ))
        }
    };
    bump_revision(&txn, conversation_id).await?;
    let delivery_hint = match origin_event_id.as_deref() {
        Some(event_id) => Some(
            crate::db::service::collaboration_service::delivery_hint_for_origin(
                &txn,
                conversation_id,
                event_id,
            )
            .await?,
        ),
        None => None,
    };
    let claimed = ClaimedPromptQueueItem {
        id,
        conversation_id,
        draft,
        origin_event_id,
        delivery_hint,
        mode_id: row.try_get("", "mode_id")?,
        claimed_by: worker_id.to_string(),
    };
    txn.commit().await?;
    let snapshot = snapshot_on(conn, conversation_id).await?;
    Ok(Some((claimed, snapshot)))
}

/// Renew ownership and cross the irreversible dispatch boundary. The worker
/// must call this immediately before handing the prompt to the Harness. If the
/// original lease already expired or was recovered, the CAS returns `false`
/// and the caller must not send.
pub(crate) async fn mark_dispatch_started(
    conn: &DatabaseConnection,
    item: &ClaimedPromptQueueItem,
    lease: Duration,
) -> Result<bool, DbError> {
    let now = Utc::now();
    let expires_at = now + lease;
    let txn = conn.begin().await?;
    ensure_state(&txn, item.conversation_id).await?;
    let result = txn
        .execute(statement(
            "UPDATE conversation_prompt_queue_item \
             SET dispatch_started_at = CURRENT_TIMESTAMP, claim_expires_at = ?, \
                 updated_at = CURRENT_TIMESTAMP \
             WHERE id = ? AND conversation_id = ? AND state = 'claimed' \
               AND claimed_by = ? AND claim_expires_at > ? \
               AND NOT EXISTS ( \
                   SELECT 1 FROM conversation_prompt_queue_state s \
                   WHERE s.conversation_id = ? AND s.paused_reason IS NOT NULL \
               ) AND (? IS NULL OR NOT EXISTS ( \
                   SELECT 1 FROM app_metadata m \
                   WHERE m.key = ? AND m.value = 'false' AND m.deleted_at IS NULL \
               ))",
            vec![
                expires_at.into(),
                item.id.clone().into(),
                item.conversation_id.into(),
                item.claimed_by.clone().into(),
                now.into(),
                item.conversation_id.into(),
                item.origin_event_id.clone().into(),
                SESSION_COLLABORATION_ENABLED_KEY.into(),
            ],
        ))
        .await?;
    if result.rows_affected() == 1 {
        if let Some(event_id) = item.origin_event_id.as_deref() {
            let changed = crate::db::service::collaboration_service::mark_origin_embedding(
                &txn,
                item.conversation_id,
                event_id,
                &item.id,
            )
            .await?;
            if !changed {
                return Err(validation(
                    "Collaboration delivery is no longer eligible for dispatch",
                ));
            }
            crate::db::service::collaboration_interrupt_service::mark_origin_dispatching(
                &txn,
                item.conversation_id,
                event_id,
            )
            .await?;
        }
        bump_revision(&txn, item.conversation_id).await?;
    }
    txn.commit().await?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn accept_claim(
    conn: &DatabaseConnection,
    item: &ClaimedPromptQueueItem,
) -> Result<PromptQueueSnapshot, DbError> {
    let txn = conn.begin().await?;
    ensure_state(&txn, item.conversation_id).await?;
    let result = txn
        .execute(statement(
            "DELETE FROM conversation_prompt_queue_item \
             WHERE id = ? AND conversation_id = ? AND state = 'claimed' AND claimed_by = ?",
            vec![
                item.id.clone().into(),
                item.conversation_id.into(),
                item.claimed_by.clone().into(),
            ],
        ))
        .await?;
    if result.rows_affected() == 0 {
        // Idempotent success for a lost response: the first DELETE may have
        // committed and only its snapshot read failed. Retrying must not turn
        // that successful acceptance into an "unknown" pause.
        let still_exists = txn
            .query_one(statement(
                "SELECT 1 AS present FROM conversation_prompt_queue_item \
                 WHERE id = ? AND conversation_id = ?",
                vec![item.id.clone().into(), item.conversation_id.into()],
            ))
            .await?
            .is_some();
        if still_exists {
            return Err(validation("Prompt queue claim is no longer owned"));
        }
        txn.commit().await?;
        return snapshot_on(conn, item.conversation_id).await;
    }
    if let Some(event_id) = item.origin_event_id.as_deref() {
        let changed = crate::db::service::collaboration_service::mark_origin_embedded(
            &txn,
            item.conversation_id,
            event_id,
            &item.id,
        )
        .await?;
        if !changed {
            return Err(validation(
                "Collaboration delivery is no longer being embedded",
            ));
        }
        crate::db::service::collaboration_interrupt_service::mark_origin_completed(
            &txn,
            item.conversation_id,
            event_id,
        )
        .await?;
    }
    bump_revision(&txn, item.conversation_id).await?;
    txn.commit().await?;
    snapshot_on(conn, item.conversation_id).await
}

pub(crate) async fn release_claim_busy(
    conn: &DatabaseConnection,
    item: &ClaimedPromptQueueItem,
) -> Result<PromptQueueSnapshot, DbError> {
    let txn = conn.begin().await?;
    ensure_state(&txn, item.conversation_id).await?;
    let result = txn
        .execute(statement(
            "UPDATE conversation_prompt_queue_item \
         SET state = 'queued', claimed_by = NULL, claim_expires_at = NULL, \
             dispatch_started_at = NULL, updated_at = CURRENT_TIMESTAMP \
         WHERE id = ? AND conversation_id = ? AND state = 'claimed' AND claimed_by = ?",
            vec![
                item.id.clone().into(),
                item.conversation_id.into(),
                item.claimed_by.clone().into(),
            ],
        ))
        .await?;
    if result.rows_affected() != 1 {
        return Err(validation("Prompt queue claim is no longer owned"));
    }
    if let Some(event_id) = item.origin_event_id.as_deref() {
        // A busy rejection happens before the Harness accepts a prompt, so an
        // embedding reservation is safe to return to the delivery queue.
        let changed = crate::db::service::collaboration_service::mark_origin_queued(
            &txn,
            item.conversation_id,
            event_id,
        )
        .await?;
        if !changed {
            return Err(validation(
                "Collaboration delivery is no longer being embedded",
            ));
        }
        crate::db::service::collaboration_interrupt_service::mark_origin_ready(
            &txn,
            item.conversation_id,
            event_id,
        )
        .await?;
    }
    bump_revision(&txn, item.conversation_id).await?;
    txn.commit().await?;
    snapshot_on(conn, item.conversation_id).await
}

/// The Harness accepted the send, but Codeg could not durably acknowledge the
/// queue item. Never return it to FIFO automatically: pause for a human check,
/// because replaying a coding prompt can repeat filesystem side effects.
pub(crate) async fn pause_dispatch_unknown(
    conn: &DatabaseConnection,
    item: &ClaimedPromptQueueItem,
) -> Result<PromptQueueSnapshot, DbError> {
    let txn = conn.begin().await?;
    ensure_state(&txn, item.conversation_id).await?;
    let result = txn
        .execute(statement(
            "UPDATE conversation_prompt_queue_item \
             SET state = 'paused', claimed_by = NULL, claim_expires_at = NULL, \
                 paused_reason = ?, updated_at = CURRENT_TIMESTAMP \
             WHERE id = ? AND conversation_id = ? AND dispatch_started_at IS NOT NULL",
            vec![
                UNKNOWN_DISPATCH_REASON.into(),
                item.id.clone().into(),
                item.conversation_id.into(),
            ],
        ))
        .await?;
    if result.rows_affected() > 0 {
        if let Some(event_id) = item.origin_event_id.as_deref() {
            crate::db::service::collaboration_service::mark_origin_failed(
                &txn,
                item.conversation_id,
                event_id,
                UNKNOWN_DISPATCH_REASON,
            )
            .await?;
            crate::db::service::collaboration_interrupt_service::mark_origin_failed(
                &txn,
                item.conversation_id,
                event_id,
                UNKNOWN_DISPATCH_REASON,
            )
            .await?;
        }
        txn.execute(statement(
            "UPDATE conversation_prompt_queue_state \
             SET paused_reason = ?, updated_at = CURRENT_TIMESTAMP WHERE conversation_id = ?",
            vec![UNKNOWN_DISPATCH_REASON.into(), item.conversation_id.into()],
        ))
        .await?;
        bump_revision(&txn, item.conversation_id).await?;
    }
    txn.commit().await?;
    snapshot_on(conn, item.conversation_id).await
}

pub(crate) async fn fail_claim(
    conn: &DatabaseConnection,
    item: &ClaimedPromptQueueItem,
    reason: &str,
) -> Result<PromptQueueSnapshot, DbError> {
    let reason = reason.trim();
    let txn = conn.begin().await?;
    ensure_state(&txn, item.conversation_id).await?;
    let result = txn
        .execute(statement(
            "UPDATE conversation_prompt_queue_item \
             SET state = 'paused', claimed_by = NULL, claim_expires_at = NULL, \
                 paused_reason = ?, updated_at = CURRENT_TIMESTAMP \
             WHERE id = ? AND conversation_id = ? AND state = 'claimed' AND claimed_by = ?",
            vec![
                reason.into(),
                item.id.clone().into(),
                item.conversation_id.into(),
                item.claimed_by.clone().into(),
            ],
        ))
        .await?;
    if result.rows_affected() != 1 {
        return Err(validation("Prompt queue claim is no longer owned"));
    }
    if let Some(event_id) = item.origin_event_id.as_deref() {
        crate::db::service::collaboration_service::mark_origin_failed(
            &txn,
            item.conversation_id,
            event_id,
            reason,
        )
        .await?;
        crate::db::service::collaboration_interrupt_service::mark_origin_failed(
            &txn,
            item.conversation_id,
            event_id,
            reason,
        )
        .await?;
    }
    txn.execute(statement(
        "UPDATE conversation_prompt_queue_state \
         SET paused_reason = ?, updated_at = CURRENT_TIMESTAMP WHERE conversation_id = ?",
        vec![reason.into(), item.conversation_id.into()],
    ))
    .await?;
    bump_revision(&txn, item.conversation_id).await?;
    txn.commit().await?;
    snapshot_on(conn, item.conversation_id).await
}

pub(crate) async fn recover_expired_claims(
    conn: &DatabaseConnection,
) -> Result<Vec<PromptQueueSnapshot>, DbError> {
    let rows = conn
        .query_all(statement(
            "SELECT DISTINCT conversation_id FROM conversation_prompt_queue_item \
             WHERE state = 'claimed' AND claim_expires_at <= ?",
            vec![Utc::now().into()],
        ))
        .await?;
    let conversation_ids = rows
        .iter()
        .map(|row| row.try_get::<i32>("", "conversation_id"))
        .collect::<Result<Vec<_>, _>>()?;
    let mut snapshots = Vec::with_capacity(conversation_ids.len());
    for conversation_id in conversation_ids {
        let txn = conn.begin().await?;
        ensure_state(&txn, conversation_id).await?;
        let now = Utc::now();
        let uncertain_origins = txn
            .query_all(statement(
                "SELECT origin_event_id FROM conversation_prompt_queue_item \
                 WHERE conversation_id = ? AND state = 'claimed' \
                   AND claim_expires_at <= ? AND dispatch_started_at IS NOT NULL \
                   AND origin_event_id IS NOT NULL",
                vec![conversation_id.into(), now.into()],
            ))
            .await?
            .into_iter()
            .map(|row| row.try_get::<String>("", "origin_event_id"))
            .collect::<Result<Vec<_>, _>>()?;
        let safe_to_retry = txn
            .execute(statement(
                "UPDATE conversation_prompt_queue_item \
                 SET state = 'queued', claimed_by = NULL, claim_expires_at = NULL, \
                     updated_at = CURRENT_TIMESTAMP \
                 WHERE conversation_id = ? AND state = 'claimed' \
                   AND claim_expires_at <= ? AND dispatch_started_at IS NULL",
                vec![conversation_id.into(), now.into()],
            ))
            .await?;
        let uncertain = txn
            .execute(statement(
                "UPDATE conversation_prompt_queue_item \
                 SET state = 'paused', claimed_by = NULL, claim_expires_at = NULL, \
                     paused_reason = ?, updated_at = CURRENT_TIMESTAMP \
                 WHERE conversation_id = ? AND state = 'claimed' \
                   AND claim_expires_at <= ? AND dispatch_started_at IS NOT NULL",
                vec![
                    UNKNOWN_DISPATCH_REASON.into(),
                    conversation_id.into(),
                    now.into(),
                ],
            ))
            .await?;
        for event_id in &uncertain_origins {
            crate::db::service::collaboration_service::mark_origin_failed(
                &txn,
                conversation_id,
                event_id,
                UNKNOWN_DISPATCH_REASON,
            )
            .await?;
            crate::db::service::collaboration_interrupt_service::mark_origin_failed(
                &txn,
                conversation_id,
                event_id,
                UNKNOWN_DISPATCH_REASON,
            )
            .await?;
        }
        if uncertain.rows_affected() > 0 {
            txn.execute(statement(
                "UPDATE conversation_prompt_queue_state \
                 SET paused_reason = ?, updated_at = CURRENT_TIMESTAMP WHERE conversation_id = ?",
                vec![UNKNOWN_DISPATCH_REASON.into(), conversation_id.into()],
            ))
            .await?;
        }
        let changed = safe_to_retry.rows_affected() + uncertain.rows_affected();
        if changed > 0 {
            bump_revision(&txn, conversation_id).await?;
        }
        txn.commit().await?;
        if changed > 0 {
            snapshots.push(snapshot_on(conn, conversation_id).await?);
        }
    }
    Ok(snapshots)
}

pub(crate) async fn pending_conversation_ids(
    conn: &DatabaseConnection,
) -> Result<Vec<i32>, DbError> {
    let rows = conn
        .query_all(statement(
            "SELECT DISTINCT q.conversation_id \
             FROM conversation_prompt_queue_item q \
             LEFT JOIN conversation_prompt_queue_state s ON s.conversation_id = q.conversation_id \
             WHERE q.state = 'queued' AND s.paused_reason IS NULL \
             ORDER BY q.conversation_id ASC",
            Vec::new(),
        ))
        .await?;
    rows.iter()
        .map(|row| {
            row.try_get::<i32>("", "conversation_id")
                .map_err(DbError::from)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::types::PromptInputBlock;
    use crate::db::service::collaboration_service;
    use crate::db::test_helpers::{
        fresh_disk_db, fresh_in_memory_db, seed_conversation, seed_folder,
    };
    use crate::models::{
        AgentType, CollaborationDeliveryHint, CollaborationDeliveryState,
        CollaborationInvocationPolicy, CollaborationUrgency, SendCollaborationMessageInput,
    };

    fn draft(text: &str) -> PromptQueueDraft {
        PromptQueueDraft {
            blocks: vec![PromptInputBlock::Text {
                text: text.to_string(),
            }],
            display_text: text.to_string(),
        }
    }

    fn input(conversation_id: i32, id: &str, text: &str) -> EnqueuePromptQueueItem {
        EnqueuePromptQueueItem {
            conversation_id,
            id: id.to_string(),
            client_dedupe_id: id.to_string(),
            draft: draft(text),
            mode_id: None,
        }
    }

    async fn seeded_memory() -> (crate::db::AppDatabase, i32) {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/codeg-prompt-queue").await;
        let conversation_id = seed_conversation(&db, folder_id, AgentType::Codex).await;
        (db, conversation_id)
    }

    #[tokio::test]
    async fn queue_survives_database_reopen_and_deduplicates_retries() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = fresh_disk_db(dir.path()).await;
        let folder_id = seed_folder(&db, "/tmp/codeg-prompt-queue-disk").await;
        let conversation_id = seed_conversation(&db, folder_id, AgentType::Codex).await;

        let first = enqueue(&db.conn, input(conversation_id, "msg-1", "persist me"))
            .await
            .expect("enqueue");
        let first_revision = first.revision;
        let duplicate = enqueue(&db.conn, input(conversation_id, "msg-1", "ignored retry"))
            .await
            .expect("dedupe retry");
        assert_eq!(duplicate.items.len(), 1);
        assert_eq!(duplicate.revision, first_revision);
        assert_eq!(
            duplicate.items[0].draft.as_ref().unwrap().display_text,
            "persist me"
        );

        drop(db);
        let reopened = fresh_disk_db(dir.path()).await;
        let restored = snapshot(&reopened.conn, conversation_id)
            .await
            .expect("restored snapshot");
        assert_eq!(restored.items.len(), 1);
        assert_eq!(restored.items[0].id, "msg-1");
        assert_eq!(restored.revision, first_revision);
    }

    #[tokio::test]
    async fn editing_reordering_and_deleting_require_current_revision() {
        let (db, conversation_id) = seeded_memory().await;
        let one = enqueue(&db.conn, input(conversation_id, "one", "first"))
            .await
            .expect("one");
        let two = enqueue(&db.conn, input(conversation_id, "two", "second"))
            .await
            .expect("two");

        let conflict = edit(
            &db.conn,
            conversation_id,
            "one",
            draft("stale"),
            one.revision,
        )
        .await;
        assert!(
            conflict.is_err(),
            "stale client must not overwrite newer state"
        );

        let reordered = reorder(
            &db.conn,
            conversation_id,
            vec!["two".into(), "one".into()],
            two.revision,
        )
        .await
        .expect("reorder");
        assert_eq!(reordered.items[0].id, "two");

        let edited = edit(
            &db.conn,
            conversation_id,
            "one",
            draft("edited"),
            reordered.revision,
        )
        .await
        .expect("edit");
        assert_eq!(
            edited.items[1].draft.as_ref().unwrap().display_text,
            "edited"
        );

        let deleted = delete(&db.conn, conversation_id, "two", edited.revision)
            .await
            .expect("delete");
        assert_eq!(deleted.items.len(), 1);
        assert_eq!(deleted.items[0].id, "one");
    }

    #[tokio::test]
    async fn ordinary_queue_controls_cannot_mutate_hidden_collaboration_items() {
        let (db, target) = seeded_memory().await;
        let target_row = conversation::Entity::find_by_id(target)
            .one(&db.conn)
            .await
            .unwrap()
            .unwrap();
        let source = seed_conversation(&db, target_row.folder_id, AgentType::ClaudeCode).await;
        let sent = collaboration_service::send(
            &db.conn,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                body: "immutable external input".to_string(),
                client_dedupe_id: "protected-origin".to_string(),
                invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .expect("send collaboration");
        let origin_id = sent.deliveries[0].id.clone();
        let mixed = enqueue(&db.conn, input(target, "ordinary", "editable"))
            .await
            .expect("ordinary enqueue");

        assert!(edit(
            &db.conn,
            target,
            &origin_id,
            draft("tampered"),
            mixed.revision
        )
        .await
        .is_err());
        let current = snapshot(&db.conn, target).await.unwrap();
        assert!(delete(&db.conn, target, &origin_id, current.revision)
            .await
            .is_err());
        let current = snapshot(&db.conn, target).await.unwrap();
        let reordered = reorder(
            &db.conn,
            target,
            vec!["ordinary".to_string()],
            current.revision,
        )
        .await
        .expect("reorder visible drafts only");
        assert_eq!(reordered.items.len(), 2);
        assert_eq!(reordered.items[0].id, origin_id);
        assert_eq!(reordered.items[1].id, "ordinary");
    }

    #[tokio::test]
    async fn empty_drafts_and_invalid_reorders_are_rejected() {
        let (db, conversation_id) = seeded_memory().await;
        let mut empty = input(conversation_id, "empty", "");
        empty.draft.blocks.clear();
        assert!(enqueue(&db.conn, empty).await.is_err());

        let snapshot = enqueue(&db.conn, input(conversation_id, "one", "first"))
            .await
            .expect("enqueue");
        assert!(reorder(
            &db.conn,
            conversation_id,
            vec!["one".into(), "one".into()],
            snapshot.revision,
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn a_head_has_one_owner_and_busy_release_preserves_identity() {
        let (db, conversation_id) = seeded_memory().await;
        enqueue(&db.conn, input(conversation_id, "head", "first"))
            .await
            .expect("enqueue");

        let (claim, claimed_snapshot) =
            claim_head(&db.conn, conversation_id, "worker-a", Duration::seconds(30))
                .await
                .expect("claim")
                .expect("head");
        assert_eq!(claim.id, "head");
        assert_eq!(
            claimed_snapshot.items[0].state,
            PromptQueueItemState::Claimed
        );
        assert!(
            claim_head(&db.conn, conversation_id, "worker-b", Duration::seconds(30),)
                .await
                .expect("second claim")
                .is_none(),
            "the claimed head cannot be acquired twice"
        );

        let released = release_claim_busy(&db.conn, &claim)
            .await
            .expect("release busy");
        assert_eq!(released.items[0].id, "head");
        assert_eq!(released.items[0].state, PromptQueueItemState::Queued);
        assert_eq!(released.items[0].attempts, 1);
    }

    #[tokio::test]
    async fn durable_head_prevents_a_direct_send_from_jumping_fifo() {
        let (db, conversation_id) = seeded_memory().await;
        enqueue(&db.conn, input(conversation_id, "head", "first"))
            .await
            .expect("enqueue");
        assert!(!send_is_admitted(&db.conn, conversation_id, Some("direct"))
            .await
            .expect("admission"));

        let (claim, _) = claim_head(&db.conn, conversation_id, "worker", Duration::seconds(30))
            .await
            .expect("claim")
            .expect("head");
        assert!(send_is_admitted(&db.conn, conversation_id, Some("head"))
            .await
            .expect("claimed admission"));
        assert!(!send_is_admitted(&db.conn, conversation_id, Some("direct"))
            .await
            .expect("direct admission"));

        accept_claim(&db.conn, &claim).await.expect("accept");
        assert!(send_is_admitted(&db.conn, conversation_id, Some("direct"))
            .await
            .expect("empty admission"));
    }

    #[tokio::test]
    async fn deterministic_failure_pauses_until_explicit_retry() {
        let (db, conversation_id) = seeded_memory().await;
        enqueue(&db.conn, input(conversation_id, "head", "first"))
            .await
            .expect("enqueue");
        let (claim, _) = claim_head(&db.conn, conversation_id, "worker", Duration::seconds(30))
            .await
            .expect("claim")
            .expect("head");

        let paused = fail_claim(&db.conn, &claim, "mode rejected")
            .await
            .expect("pause");
        assert_eq!(paused.paused_reason.as_deref(), Some("mode rejected"));
        assert_eq!(paused.items[0].state, PromptQueueItemState::Paused);
        assert!(
            claim_head(&db.conn, conversation_id, "other", Duration::seconds(30),)
                .await
                .expect("paused claim")
                .is_none()
        );

        let retried = retry_item(&db.conn, conversation_id, "head", paused.revision)
            .await
            .expect("retry");
        assert!(retried.paused_reason.is_none());
        assert_eq!(retried.items[0].state, PromptQueueItemState::Queued);
        assert_ne!(retried.items[0].id, "head");
        assert_eq!(
            retried.items[0].client_dedupe_id, retried.items[0].id,
            "a deliberate retry is a new user-message attempt"
        );
    }

    #[tokio::test]
    async fn expired_claim_recovers_without_changing_message_identity() {
        let (db, conversation_id) = seeded_memory().await;
        enqueue(&db.conn, input(conversation_id, "head", "first"))
            .await
            .expect("enqueue");
        claim_head(
            &db.conn,
            conversation_id,
            "dead-worker",
            Duration::seconds(-1),
        )
        .await
        .expect("claim")
        .expect("head");

        let recovered = recover_expired_claims(&db.conn).await.expect("recover");
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].items[0].id, "head");
        assert_eq!(recovered[0].items[0].state, PromptQueueItemState::Queued);
        assert_eq!(recovered[0].items[0].attempts, 1);
    }

    #[tokio::test]
    async fn expired_dispatch_is_paused_as_unknown_instead_of_replayed() {
        let (db, conversation_id) = seeded_memory().await;
        enqueue(&db.conn, input(conversation_id, "head", "possibly sent"))
            .await
            .expect("enqueue");
        let (claim, _) = claim_head(
            &db.conn,
            conversation_id,
            "dead-worker",
            Duration::seconds(30),
        )
        .await
        .expect("claim")
        .expect("head");
        assert!(
            mark_dispatch_started(&db.conn, &claim, Duration::seconds(-1))
                .await
                .expect("mark dispatch")
        );

        let recovered = recover_expired_claims(&db.conn).await.expect("recover");
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].items[0].id, "head");
        assert_eq!(recovered[0].items[0].state, PromptQueueItemState::Paused);
        assert_eq!(
            recovered[0].paused_reason.as_deref(),
            Some("dispatch_outcome_unknown")
        );
        assert!(
            claim_head(
                &db.conn,
                conversation_id,
                "new-worker",
                Duration::seconds(30),
            )
            .await
            .expect("claim after recovery")
            .is_none(),
            "an unknown dispatch must never replay automatically"
        );
    }

    #[tokio::test]
    async fn expired_collaboration_dispatch_marks_delivery_failed_without_replay() {
        let (db, target) = seeded_memory().await;
        let target_row = conversation::Entity::find_by_id(target)
            .one(&db.conn)
            .await
            .unwrap()
            .unwrap();
        let source = seed_conversation(&db, target_row.folder_id, AgentType::ClaudeCode).await;
        let sent = collaboration_service::send(
            &db.conn,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                body: "possibly accepted".to_string(),
                client_dedupe_id: "unknown-collaboration-dispatch".to_string(),
                invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .expect("send collaboration");
        let (claim, _) = claim_head(&db.conn, target, "dead-worker", Duration::seconds(30))
            .await
            .expect("claim")
            .expect("head");
        assert_eq!(
            claim.origin_event_id.as_deref(),
            Some(sent.event_id.as_str())
        );
        assert!(
            mark_dispatch_started(&db.conn, &claim, Duration::seconds(-1))
                .await
                .expect("mark dispatch")
        );

        let recovered = recover_expired_claims(&db.conn).await.expect("recover");
        assert_eq!(recovered[0].items[0].state, PromptQueueItemState::Paused);
        let delivery = &collaboration_service::feed(&db.conn, target, None)
            .await
            .unwrap()
            .inbound[0];
        assert_eq!(delivery.state, CollaborationDeliveryState::Failed);
        assert_eq!(delivery.error.as_deref(), Some(UNKNOWN_DISPATCH_REASON));
        assert!(
            claim_head(&db.conn, target, "new-worker", Duration::seconds(30))
                .await
                .expect("claim after recovery")
                .is_none(),
            "an unknown cross-Session dispatch must never replay automatically"
        );
    }

    #[tokio::test]
    async fn collaboration_retry_gets_a_new_message_identity() {
        let (db, target) = seeded_memory().await;
        let target_row = conversation::Entity::find_by_id(target)
            .one(&db.conn)
            .await
            .unwrap()
            .unwrap();
        let source = seed_conversation(&db, target_row.folder_id, AgentType::ClaudeCode).await;
        let sent = collaboration_service::send(
            &db.conn,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                body: "retry with a new turn identity".to_string(),
                client_dedupe_id: "collaboration-retry-identity".to_string(),
                invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .expect("send collaboration");
        let delivery_id = sent.deliveries[0].id.clone();
        let (claim, _) = claim_head(&db.conn, target, "worker", Duration::seconds(30))
            .await
            .expect("claim")
            .expect("head");
        assert_eq!(claim.id, delivery_id);
        assert!(
            mark_dispatch_started(&db.conn, &claim, Duration::seconds(30))
                .await
                .expect("mark dispatch")
        );
        let paused = fail_claim(&db.conn, &claim, "harness rejected")
            .await
            .expect("pause");

        let retried = retry_item(&db.conn, target, &delivery_id, paused.revision)
            .await
            .expect("retry");
        assert_eq!(retried.items[0].id, format!("{delivery_id}#2"));
        assert_eq!(
            retried.items[0].client_dedupe_id,
            format!("{delivery_id}:2")
        );
        let (retry_claim, _) = claim_head(&db.conn, target, "retry-worker", Duration::seconds(30))
            .await
            .expect("claim retry")
            .expect("retry head");
        assert_eq!(retry_claim.id, format!("{delivery_id}#2"));
    }

    #[tokio::test]
    async fn inactive_collaboration_starts_only_after_explicit_confirmation() {
        let (db, target) = seeded_memory().await;
        let target_row = conversation::Entity::find_by_id(target)
            .one(&db.conn)
            .await
            .unwrap()
            .unwrap();
        let source = seed_conversation(&db, target_row.folder_id, AgentType::ClaudeCode).await;
        let sent = collaboration_service::send_with_initially_inactive_targets(
            &db.conn,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                body: "wait for an explicit start".to_string(),
                client_dedupe_id: "inactive-collaboration-confirmation".to_string(),
                invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
            &std::collections::HashSet::from([target]),
        )
        .await
        .expect("deliver paused collaboration");
        let delivery_id = sent.deliveries[0].id.clone();
        let paused = snapshot(&db.conn, target).await.expect("paused snapshot");
        assert_eq!(paused.items[0].state, PromptQueueItemState::Paused);
        assert!(
            claim_head(&db.conn, target, "worker", Duration::seconds(30))
                .await
                .expect("paused claim")
                .is_none()
        );

        let confirmed = retry_item(&db.conn, target, &delivery_id, paused.revision)
            .await
            .expect("explicit confirmation");
        let confirmed_id = format!("{delivery_id}#1");
        assert_eq!(confirmed.items[0].id, confirmed_id);
        assert_eq!(confirmed.items[0].state, PromptQueueItemState::Queued);
        let delivery = &collaboration_service::feed(&db.conn, target, None)
            .await
            .expect("delivery feed")
            .inbound[0];
        assert_eq!(delivery.state, CollaborationDeliveryState::Queued);
        assert_eq!(delivery.queue_state, Some(PromptQueueItemState::Queued));
        assert_eq!(
            delivery.queue_item_id.as_deref(),
            Some(confirmed_id.as_str())
        );
    }

    #[tokio::test]
    async fn collaboration_policy_freezes_only_origin_items_and_thaws_them() {
        let (db, target) = seeded_memory().await;
        let target_row = conversation::Entity::find_by_id(target)
            .one(&db.conn)
            .await
            .unwrap()
            .unwrap();
        let source = seed_conversation(&db, target_row.folder_id, AgentType::ClaudeCode).await;
        let sent = collaboration_service::send(
            &db.conn,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                body: "freeze this cross-session request".to_string(),
                client_dedupe_id: "collaboration-policy-freeze".to_string(),
                invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .expect("send collaboration");
        enqueue(
            &db.conn,
            input(
                target,
                "ordinary-follow-up",
                "keep the user's own queue moving",
            ),
        )
        .await
        .expect("enqueue ordinary follow-up");

        let frozen = reconcile_collaboration_dispatch_policy(&db.conn, false)
            .await
            .expect("freeze collaboration");
        assert_eq!(frozen.len(), 1);
        assert_eq!(frozen[0].origin_event_ids, vec![sent.event_id.clone()]);
        let snapshot = &frozen[0].snapshot;
        assert!(snapshot.paused_reason.is_none());
        let origin = snapshot
            .items
            .iter()
            .find(|item| item.origin_event_id.as_deref() == Some(sent.event_id.as_str()))
            .expect("origin item");
        assert_eq!(origin.state, PromptQueueItemState::Paused);
        assert_eq!(
            origin.paused_reason.as_deref(),
            Some(COLLABORATION_DISABLED_REASON)
        );

        let (ordinary, _) = claim_head(&db.conn, target, "worker", Duration::seconds(30))
            .await
            .expect("claim ordinary item")
            .expect("ordinary item remains runnable");
        assert_eq!(ordinary.id, "ordinary-follow-up");
        release_claim_busy(&db.conn, &ordinary)
            .await
            .expect("release ordinary item");

        let thawed = reconcile_collaboration_dispatch_policy(&db.conn, true)
            .await
            .expect("thaw collaboration");
        assert_eq!(thawed.len(), 1);
        let origin = thawed[0]
            .snapshot
            .items
            .iter()
            .find(|item| item.origin_event_id.as_deref() == Some(sent.event_id.as_str()))
            .expect("thawed origin item");
        assert_eq!(origin.state, PromptQueueItemState::Queued);
        assert!(origin.paused_reason.is_none());
    }

    #[tokio::test]
    async fn collaboration_dispatch_boundary_obeys_persisted_policy() {
        let (db, target) = seeded_memory().await;
        let target_row = conversation::Entity::find_by_id(target)
            .one(&db.conn)
            .await
            .unwrap()
            .unwrap();
        let source = seed_conversation(&db, target_row.folder_id, AgentType::ClaudeCode).await;
        collaboration_service::send(
            &db.conn,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                body: "must stop before the harness".to_string(),
                client_dedupe_id: "collaboration-policy-boundary".to_string(),
                invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .expect("send collaboration");
        let (claim, _) = claim_head(&db.conn, target, "worker", Duration::seconds(30))
            .await
            .expect("claim")
            .expect("origin head");

        crate::db::service::app_metadata_service::upsert_value(
            &db.conn,
            SESSION_COLLABORATION_ENABLED_KEY,
            "false",
        )
        .await
        .expect("disable collaboration");
        assert!(
            !mark_dispatch_started(&db.conn, &claim, Duration::seconds(30))
                .await
                .expect("dispatch boundary"),
            "a disabled collaboration must not cross into the Harness"
        );

        let frozen = reconcile_collaboration_dispatch_policy(&db.conn, false)
            .await
            .expect("freeze claimed origin");
        assert_eq!(
            frozen[0].snapshot.items[0].state,
            PromptQueueItemState::Paused
        );
        let delivery = &collaboration_service::feed(&db.conn, target, None)
            .await
            .expect("delivery feed")
            .inbound[0];
        assert_eq!(delivery.state, CollaborationDeliveryState::Queued);
        assert_eq!(delivery.queue_state, Some(PromptQueueItemState::Paused));
        assert_eq!(
            delivery.queue_paused_reason.as_deref(),
            Some(COLLABORATION_DISABLED_REASON)
        );
    }

    #[tokio::test]
    async fn dispatch_cas_refuses_an_already_expired_claim() {
        let (db, conversation_id) = seeded_memory().await;
        enqueue(&db.conn, input(conversation_id, "head", "do not send"))
            .await
            .expect("enqueue");
        let (claim, _) = claim_head(
            &db.conn,
            conversation_id,
            "slow-worker",
            Duration::seconds(-1),
        )
        .await
        .expect("claim")
        .expect("head");
        assert!(
            !mark_dispatch_started(&db.conn, &claim, Duration::seconds(30))
                .await
                .expect("dispatch CAS"),
            "a worker that lost its lease must abandon the send"
        );
    }

    #[tokio::test]
    async fn dispatch_cas_refuses_a_claim_paused_before_the_send_boundary() {
        let (db, conversation_id) = seeded_memory().await;
        enqueue(&db.conn, input(conversation_id, "head", "do not send"))
            .await
            .expect("enqueue");
        let (claim, _) = claim_head(&db.conn, conversation_id, "worker", Duration::seconds(30))
            .await
            .expect("claim")
            .expect("head");
        pause_queue(&db.conn, conversation_id, "cancelled_current_turn".into())
            .await
            .expect("pause");

        assert!(
            !mark_dispatch_started(&db.conn, &claim, Duration::seconds(30))
                .await
                .expect("dispatch CAS"),
            "a cancellation that wins before dispatch must prevent the send"
        );
    }

    #[tokio::test]
    async fn final_admission_refuses_a_dispatch_paused_after_marking() {
        let (db, conversation_id) = seeded_memory().await;
        enqueue(&db.conn, input(conversation_id, "head", "do not send"))
            .await
            .expect("enqueue");
        let (claim, _) = claim_head(&db.conn, conversation_id, "worker", Duration::seconds(30))
            .await
            .expect("claim")
            .expect("head");
        assert!(
            mark_dispatch_started(&db.conn, &claim, Duration::seconds(30))
                .await
                .expect("mark")
        );
        pause_queue(&db.conn, conversation_id, "cancelled_current_turn".into())
            .await
            .expect("pause");

        assert!(
            !send_is_admitted(&db.conn, conversation_id, Some(&claim.id))
                .await
                .expect("admission"),
            "the send gate must re-check cancellation immediately before the Harness"
        );
        let released = release_claim_busy(&db.conn, &claim)
            .await
            .expect("release after rejected send");
        assert_eq!(released.items[0].state, PromptQueueItemState::Queued);
        assert_eq!(
            released.paused_reason.as_deref(),
            Some("cancelled_current_turn")
        );
    }

    #[tokio::test]
    async fn accepting_the_same_claim_twice_is_idempotent() {
        let (db, conversation_id) = seeded_memory().await;
        enqueue(&db.conn, input(conversation_id, "head", "once"))
            .await
            .expect("enqueue");
        let (claim, _) = claim_head(&db.conn, conversation_id, "worker", Duration::seconds(30))
            .await
            .expect("claim")
            .expect("head");
        assert!(
            mark_dispatch_started(&db.conn, &claim, Duration::seconds(30))
                .await
                .expect("mark")
        );
        let first = accept_claim(&db.conn, &claim).await.expect("first accept");
        let retry = accept_claim(&db.conn, &claim).await.expect("retry accept");
        assert!(first.items.is_empty());
        assert!(retry.items.is_empty());
        assert_eq!(retry.revision, first.revision);
    }

    #[tokio::test]
    async fn deleting_the_session_cascades_its_queue() {
        use sea_orm::{ConnectionTrait, DbBackend, Statement};

        let (db, conversation_id) = seeded_memory().await;
        enqueue(&db.conn, input(conversation_id, "head", "first"))
            .await
            .expect("enqueue");
        db.conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "DELETE FROM conversation WHERE id = ?",
                vec![conversation_id.into()],
            ))
            .await
            .expect("delete conversation");
        let count: i64 = db
            .conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS count FROM conversation_prompt_queue_item",
            ))
            .await
            .expect("count query")
            .expect("count row")
            .try_get("", "count")
            .expect("count");
        assert_eq!(count, 0);
    }
}
