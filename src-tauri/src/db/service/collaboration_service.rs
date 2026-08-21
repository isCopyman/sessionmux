use std::collections::{BTreeSet, HashSet};

use chrono::{DateTime, Utc};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, QueryResult, Statement,
    TransactionTrait,
};

use crate::acp::types::PromptInputBlock;
use crate::db::error::DbError;
use crate::db::service::collaboration_mention;
use crate::db::service::collaboration_room_service;
use crate::db::service::prompt_queue_service;
use crate::models::{
    CollaborationAgentReceiptKind, CollaborationAttentionState, CollaborationAuthorKind,
    CollaborationDeliveryHint, CollaborationDeliveryState, CollaborationDeliveryView,
    CollaborationFeed, CollaborationInterruptState, CollaborationInvocationPolicy,
    CollaborationObligationState, CollaborationSendResult, CollaborationSessionSnapshot,
    CollaborationTimelineProjection, CollaborationUnreadOverview, CollaborationUnreadSession,
    CollaborationUrgency, PostRoomMessageInput, PromptQueueDraft, PromptQueueItemState,
    RoomPostResult, SendCollaborationMessageInput,
};

const MAX_BODY_BYTES: usize = 1_000_000;
const MAX_TARGETS: usize = 16;
const MAX_DEDUPE_ID_BYTES: usize = 200;
const DEFAULT_FEED_LIMIT: u32 = 100;
const MAX_FEED_LIMIT: u32 = 500;
const MAX_STORE_ONLY_DELIVERIES_PER_TURN: usize = 16;
/// First delivery copies a bounded prefix of the body into the prompt.
/// The rest stays in the mailbox / Room ledger; `read_message` /
/// `read_room_post` return it. Legal events may be up to `MAX_BODY_BYTES`.
const MAX_FIRST_DELIVERY_BODY_CHARS: usize = 8_000;
const MAX_PARENT_SNIPPET_CHARS: usize = 200;
const DELIVERY_ENVELOPE_SELECT: &str =
    "SELECT d.id AS delivery_id, e.id AS event_id, e.reply_to_event_id, \
                    CASE WHEN d.obligation_state = 'awaiting_reply' THEN 1 ELSE 0 END \
                        AS effective_expects_reply, \
                    e.source_conversation_id, e.source_title_snapshot, \
                    e.source_agent_type_snapshot, e.source_folder_path_snapshot, \
                    e.subject, e.body, e.visibility, e.room_id, \
                    parent.source_conversation_id AS parent_source_conversation_id, \
                    parent.source_title_snapshot AS parent_source_title, \
                    COALESCE(parent.author_kind, 'session') AS parent_author_kind, \
                    parent.body AS parent_body \
             FROM collaboration_delivery d \
             JOIN collaboration_event e ON e.id = d.event_id \
             LEFT JOIN collaboration_event parent ON parent.id = e.reply_to_event_id";
// One legal event body may already be MAX_BODY_BYTES. First delivery now
// truncates, so a 16-item store_only batch stays far below this ceiling.
const MAX_STORE_ONLY_ENVELOPE_BYTES_PER_TURN: usize = MAX_BODY_BYTES + 128_000;
pub const INACTIVE_TARGET_CONFIRMATION_REASON: &str =
    "collaboration_target_inactive_confirmation_required";
/// An Agent may create replies through this depth, but the event at the limit
/// cannot ask another Agent for a reply. Human UI sends remain unrestricted.
pub const MAX_AGENT_REPLY_CHAIN_DEPTH: i32 = 4;
/// Mailbox / inbox / session-timeline projections must never see Room events.
const DIRECT_MAIL_SQL: &str = "AND COALESCE(e.visibility, 'direct') = 'direct'";

/// Versioned, transcript-safe envelope persisted by every Harness when Codeg
/// invokes a Session on behalf of another Session. The UUID-scoped closing
/// marker makes an accidental collision with the untrusted body impractical;
/// malformed or future versions remain readable as ordinary text.
const ENVELOPE_VERSION: u8 = 1;
const ENVELOPE_PREFIX: &str = "<<<CODEG_SESSION_MESSAGE_V1:";
const ENVELOPE_END_PREFIX: &str = "<<<END_CODEG_SESSION_MESSAGE_V1:";

#[derive(Debug, Clone)]
pub(crate) struct ClaimedStoreOnlyBatch {
    pub blocks: Vec<PromptInputBlock>,
    pub event_ids: Vec<String>,
    pub affected_conversation_ids: Vec<i32>,
    pub turn_ref: String,
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

fn parse_optional_timestamp(
    row: &QueryResult,
    column: &str,
) -> Result<Option<DateTime<Utc>>, DbError> {
    row.try_get("", column).map_err(DbError::from)
}

#[derive(Debug, Clone)]
struct LiveSession {
    id: i32,
    title: Option<String>,
    agent_type: String,
    folder_path: Option<String>,
    archived: bool,
}

impl LiveSession {
    fn snapshot(&self) -> CollaborationSessionSnapshot {
        CollaborationSessionSnapshot {
            conversation_id: self.id,
            title: self.title.clone(),
            agent_type: Some(self.agent_type.clone()),
            folder_path: self.folder_path.clone(),
            backend: "current".to_string(),
        }
    }
}

async fn live_session<C: ConnectionTrait>(
    conn: &C,
    conversation_id: i32,
) -> Result<Option<LiveSession>, DbError> {
    let row = conn
        .query_one(statement(
            "SELECT c.id, c.title, c.agent_type, \
             (c.archived_at IS NOT NULL) AS archived, f.path AS folder_path \
             FROM conversation c LEFT JOIN folder f ON f.id = c.folder_id \
             WHERE c.id = ? AND c.deleted_at IS NULL",
            vec![conversation_id.into()],
        ))
        .await?;
    row.map(|row| {
        Ok(LiveSession {
            id: row.try_get("", "id")?,
            title: row.try_get("", "title")?,
            agent_type: row.try_get("", "agent_type")?,
            folder_path: row.try_get("", "folder_path")?,
            archived: row.try_get::<i64>("", "archived")? != 0,
        })
    })
    .transpose()
}

async fn require_live_session<C: ConnectionTrait>(
    conn: &C,
    conversation_id: i32,
) -> Result<LiveSession, DbError> {
    live_session(conn, conversation_id)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("Conversation {conversation_id}")))
}

async fn ensure_state(txn: &DatabaseTransaction, conversation_id: i32) -> Result<(), DbError> {
    txn.execute(statement(
        "INSERT OR IGNORE INTO conversation_collaboration_state \
         (conversation_id, revision, updated_at) VALUES (?, 0, CURRENT_TIMESTAMP)",
        vec![conversation_id.into()],
    ))
    .await?;
    Ok(())
}

async fn ensure_state_if_live(
    txn: &DatabaseTransaction,
    conversation_id: i32,
) -> Result<bool, DbError> {
    if live_session(txn, conversation_id).await?.is_none() {
        return Ok(false);
    }
    ensure_state(txn, conversation_id).await?;
    Ok(true)
}

async fn bump_revision(txn: &DatabaseTransaction, conversation_id: i32) -> Result<(), DbError> {
    txn.execute(statement(
        "UPDATE conversation_collaboration_state \
         SET revision = revision + 1, updated_at = CURRENT_TIMESTAMP \
         WHERE conversation_id = ?",
        vec![conversation_id.into()],
    ))
    .await?;
    Ok(())
}

async fn revision<C: ConnectionTrait>(conn: &C, conversation_id: i32) -> Result<i64, DbError> {
    let row = conn
        .query_one(statement(
            "SELECT revision FROM conversation_collaboration_state WHERE conversation_id = ?",
            vec![conversation_id.into()],
        ))
        .await?;
    Ok(match row {
        Some(row) => row.try_get("", "revision")?,
        None => 0,
    })
}

const DELIVERY_SELECT: &str = "SELECT d.id, d.event_id, d.target_conversation_id, \
            d.target_title_snapshot, d.target_agent_type_snapshot, \
             d.target_folder_path_snapshot, d.invocation_policy, d.delivery_hint, \
             d.state, d.attention_state, d.opened_at, d.agent_received_at, \
             d.agent_receipt_kind, d.agent_receipt_ref, d.obligation_state, \
             d.obligation_created_at, d.obligation_resolved_at, \
             d.ui_seen_at, d.embedded_turn_ref, d.attempts, d.error, \
            q.id AS queue_item_id, q.state AS queue_state, \
            q.paused_reason AS queue_paused_reason, i.id AS interrupt_operation_id, \
            i.state AS interrupt_state, i.error AS interrupt_error, \
            d.created_at, d.updated_at, e.source_conversation_id, \
            e.source_title_snapshot, e.source_agent_type_snapshot, \
            e.source_folder_path_snapshot, e.source_backend_snapshot, \
            e.subject, e.body, \
            e.reply_to_event_id, e.expects_reply, \
            EXISTS (SELECT 1 FROM collaboration_event reply \
              WHERE reply.reply_to_event_id = e.id \
                AND reply.source_conversation_id = d.target_conversation_id) \
              AS reply_received, e.urgency \
     FROM collaboration_delivery d \
     JOIN collaboration_event e ON e.id = d.event_id \
     LEFT JOIN conversation_prompt_queue_item q \
       ON q.origin_event_id = d.event_id \
       AND q.conversation_id = d.target_conversation_id \
     LEFT JOIN collaboration_interrupt_operation i \
       ON i.event_id = d.event_id \
      AND i.target_conversation_id = d.target_conversation_id ";

fn parse_delivery(row: &QueryResult) -> Result<CollaborationDeliveryView, DbError> {
    let invocation_raw: String = row.try_get("", "invocation_policy")?;
    let invocation_policy = CollaborationInvocationPolicy::parse(&invocation_raw)
        .ok_or_else(|| validation(format!("Unknown invocation policy: {invocation_raw}")))?;
    let hint_raw: String = row.try_get("", "delivery_hint")?;
    let delivery_hint = CollaborationDeliveryHint::parse(&hint_raw)
        .ok_or_else(|| validation(format!("Unknown delivery hint: {hint_raw}")))?;
    let state_raw: String = row.try_get("", "state")?;
    let state = CollaborationDeliveryState::parse(&state_raw)
        .ok_or_else(|| validation(format!("Unknown delivery state: {state_raw}")))?;
    let queue_state = row
        .try_get::<Option<String>>("", "queue_state")?
        .map(|value| {
            PromptQueueItemState::parse(&value)
                .ok_or_else(|| validation(format!("Unknown prompt queue state: {value}")))
        })
        .transpose()?;
    let urgency_raw: String = row.try_get("", "urgency")?;
    let urgency = CollaborationUrgency::parse(&urgency_raw)
        .ok_or_else(|| validation(format!("Unknown urgency: {urgency_raw}")))?;
    let expects_reply: i64 = row.try_get("", "expects_reply")?;
    let reply_received: i64 = row.try_get("", "reply_received")?;
    let attention_raw: String = row.try_get("", "attention_state")?;
    let attention_state = CollaborationAttentionState::parse(&attention_raw)
        .ok_or_else(|| validation(format!("Unknown attention state: {attention_raw}")))?;
    let agent_receipt_kind = row
        .try_get::<Option<String>>("", "agent_receipt_kind")?
        .map(|value| {
            CollaborationAgentReceiptKind::parse(&value)
                .ok_or_else(|| validation(format!("Unknown Agent receipt kind: {value}")))
        })
        .transpose()?;
    let obligation_raw: String = row.try_get("", "obligation_state")?;
    let obligation_state = CollaborationObligationState::parse(&obligation_raw)
        .ok_or_else(|| validation(format!("Unknown obligation state: {obligation_raw}")))?;
    let interrupt_state = row
        .try_get::<Option<String>>("", "interrupt_state")?
        .map(|value| {
            CollaborationInterruptState::parse(&value).ok_or_else(|| {
                validation(format!("Unknown collaboration interrupt state: {value}"))
            })
        })
        .transpose()?;

    Ok(CollaborationDeliveryView {
        id: row.try_get("", "id")?,
        event_id: row.try_get("", "event_id")?,
        source: CollaborationSessionSnapshot {
            conversation_id: row.try_get("", "source_conversation_id")?,
            title: row.try_get("", "source_title_snapshot")?,
            agent_type: Some(row.try_get("", "source_agent_type_snapshot")?),
            folder_path: row.try_get("", "source_folder_path_snapshot")?,
            backend: row.try_get("", "source_backend_snapshot")?,
        },
        target: CollaborationSessionSnapshot {
            conversation_id: row.try_get("", "target_conversation_id")?,
            title: row.try_get("", "target_title_snapshot")?,
            agent_type: row.try_get("", "target_agent_type_snapshot")?,
            folder_path: row.try_get("", "target_folder_path_snapshot")?,
            backend: "current".to_string(),
        },
        subject: row
            .try_get::<Option<String>>("", "subject")?
            .unwrap_or_default(),
        body: row.try_get("", "body")?,
        reply_to_event_id: row.try_get("", "reply_to_event_id")?,
        expects_reply: expects_reply != 0,
        reply_received: reply_received != 0,
        urgency,
        invocation_policy,
        delivery_hint,
        state,
        queue_item_id: row.try_get("", "queue_item_id")?,
        queue_state,
        queue_paused_reason: row.try_get("", "queue_paused_reason")?,
        attention_state,
        opened_at: parse_optional_timestamp(row, "opened_at")?,
        agent_received_at: parse_optional_timestamp(row, "agent_received_at")?,
        agent_receipt_kind,
        agent_receipt_ref: row.try_get("", "agent_receipt_ref")?,
        obligation_state,
        obligation_created_at: parse_optional_timestamp(row, "obligation_created_at")?,
        obligation_resolved_at: parse_optional_timestamp(row, "obligation_resolved_at")?,
        ui_seen_at: parse_optional_timestamp(row, "ui_seen_at")?,
        embedded_turn_ref: row.try_get("", "embedded_turn_ref")?,
        attempts: row.try_get("", "attempts")?,
        error: row.try_get("", "error")?,
        interrupt_operation_id: row.try_get("", "interrupt_operation_id")?,
        interrupt_state,
        interrupt_error: row.try_get("", "interrupt_error")?,
        created_at: parse_timestamp(row, "created_at")?,
        updated_at: parse_timestamp(row, "updated_at")?,
    })
}

async fn deliveries_for_event<C: ConnectionTrait>(
    conn: &C,
    event_id: &str,
) -> Result<Vec<CollaborationDeliveryView>, DbError> {
    let rows = conn
        .query_all(statement(
            &format!(
                "{DELIVERY_SELECT} WHERE d.event_id = ? \
                 ORDER BY d.target_conversation_id ASC, d.id ASC"
            ),
            vec![event_id.into()],
        ))
        .await?;
    rows.iter().map(parse_delivery).collect()
}

async fn event_id_for_dedupe<C: ConnectionTrait>(
    conn: &C,
    source_conversation_id: i32,
    client_dedupe_id: &str,
) -> Result<Option<String>, DbError> {
    let row = conn
        .query_one(statement(
            "SELECT id FROM collaboration_event \
             WHERE source_conversation_id = ? AND client_dedupe_id = ?",
            vec![source_conversation_id.into(), client_dedupe_id.into()],
        ))
        .await?;
    row.map(|row| row.try_get("", "id").map_err(DbError::from))
        .transpose()
}

async fn validate_dedupe_payload<C: ConnectionTrait>(
    conn: &C,
    event_id: &str,
    input: &SendCollaborationMessageInput,
    effective_expects_reply: bool,
) -> Result<(), DbError> {
    let row = conn
        .query_one(statement(
            "SELECT subject, body, reply_to_event_id, expects_reply, urgency \
             FROM collaboration_event WHERE id = ?",
            vec![event_id.into()],
        ))
        .await?
        .ok_or_else(|| DbError::NotFound(format!("Collaboration event {event_id}")))?;
    let subject: String = row
        .try_get::<Option<String>>("", "subject")?
        .unwrap_or_default();
    let body: String = row.try_get("", "body")?;
    let reply_to: Option<String> = row.try_get("", "reply_to_event_id")?;
    let expects_reply: i64 = row.try_get("", "expects_reply")?;
    let urgency: String = row.try_get("", "urgency")?;
    let expected_subject =
        crate::acp::session_collaboration::normalize_letter_title(&input.subject)
            .unwrap_or_else(|_| input.subject.trim().to_string());
    if subject != expected_subject
        || body != input.body
        || reply_to != input.reply_to_event_id
        || (expects_reply != 0) != effective_expects_reply
        || urgency != input.urgency.as_str()
    {
        return Err(validation(
            "Collaboration dedupe id was reused with a different message payload",
        ));
    }
    Ok(())
}

/// A reply or follow-up is a directed edge on an existing thread. The caller
/// must either have received the parent, or have authored it (a supplement
/// after their own last letter). Merely knowing an event id is not enough to
/// attach an unrelated message, and a reply cannot silently fan out to third
/// parties while presenting itself as the answer to one event.
async fn validate_reply_relation<C: ConnectionTrait>(
    conn: &C,
    source_conversation_id: i32,
    target_ids: &[i32],
    reply_to_event_id: &str,
) -> Result<(), DbError> {
    let received = conn
        .query_one(statement(
            "SELECT e.source_conversation_id \
             FROM collaboration_event e \
             JOIN collaboration_delivery d ON d.event_id = e.id \
             WHERE e.id = ? AND d.target_conversation_id = ?",
            vec![reply_to_event_id.into(), source_conversation_id.into()],
        ))
        .await?;
    if let Some(row) = received {
        let original_source: i32 = row.try_get("", "source_conversation_id")?;
        if target_ids != [original_source] {
            return Err(validation(format!(
                "A reply to collaboration event {reply_to_event_id} must target only its source Session {original_source}"
            )));
        }
        return Ok(());
    }

    let authored = conn
        .query_one(statement(
            "SELECT 1 AS ok FROM collaboration_event \
             WHERE id = ? AND source_conversation_id = ?",
            vec![reply_to_event_id.into(), source_conversation_id.into()],
        ))
        .await?;
    if authored.is_some() {
        let parent_targets = delivery_target_ids(conn, reply_to_event_id).await?;
        if target_ids != parent_targets.as_slice() {
            return Err(validation(format!(
                "A follow-up to collaboration event {reply_to_event_id} must target the same Session(s) as that letter"
            )));
        }
        return Ok(());
    }

    Err(validation(format!(
        "Session {source_conversation_id} cannot reply to collaboration event {reply_to_event_id}"
    )))
}

async fn delivery_target_ids<C: ConnectionTrait>(
    conn: &C,
    event_id: &str,
) -> Result<Vec<i32>, DbError> {
    let rows = conn
        .query_all(statement(
            "SELECT target_conversation_id FROM collaboration_delivery \
             WHERE event_id = ? ORDER BY target_conversation_id",
            vec![event_id.into()],
        ))
        .await?;
    let mut ids = Vec::with_capacity(rows.len());
    for row in rows {
        ids.push(row.try_get("", "target_conversation_id")?);
    }
    Ok(ids)
}

/// Derive a child's immutable depth from its parent relation. Root events are
/// depth zero; no mutable counter is maintained alongside the event graph.
pub(crate) async fn child_chain_depth<C: ConnectionTrait>(
    conn: &C,
    reply_to_event_id: Option<&str>,
) -> Result<i32, DbError> {
    let Some(reply_to_event_id) = reply_to_event_id else {
        return Ok(0);
    };
    let row = conn
        .query_one(statement(
            "SELECT chain_depth FROM collaboration_event WHERE id = ?",
            vec![reply_to_event_id.into()],
        ))
        .await?
        .ok_or_else(|| DbError::NotFound(format!("Collaboration event {reply_to_event_id}")))?;
    let parent_depth: i32 = row.try_get("", "chain_depth")?;
    parent_depth
        .checked_add(1)
        .ok_or_else(|| validation("Collaboration reply chain depth overflowed"))
}

fn validate_input(input: &SendCollaborationMessageInput) -> Result<Vec<i32>, DbError> {
    crate::acp::session_collaboration::normalize_letter_title(&input.subject)
        .map_err(validation)?;
    if input.body.trim().is_empty() {
        return Err(validation("Collaboration message body cannot be empty"));
    }
    if input.body.len() > MAX_BODY_BYTES {
        return Err(validation("Collaboration message body is too large"));
    }
    let dedupe_id = input.client_dedupe_id.trim();
    if dedupe_id.is_empty() || dedupe_id.len() > MAX_DEDUPE_ID_BYTES {
        return Err(validation(format!(
            "client_dedupe_id must contain between 1 and {MAX_DEDUPE_ID_BYTES} bytes"
        )));
    }
    if input.delivery_hint == CollaborationDeliveryHint::SteerIfSupported
        && input.invocation_policy != CollaborationInvocationPolicy::InvokeWhenIdle
    {
        return Err(validation("steer_if_supported requires invoke_when_idle"));
    }

    let targets: BTreeSet<i32> = input.target_conversation_ids.iter().copied().collect();
    if targets.is_empty() {
        return Err(validation(
            "A collaboration message requires at least one target Session",
        ));
    }
    if targets.len() > MAX_TARGETS {
        return Err(validation(format!(
            "A collaboration message supports at most {MAX_TARGETS} targets"
        )));
    }
    if targets.contains(&input.source_conversation_id) {
        return Err(validation("A Session cannot send a message to itself"));
    }
    Ok(targets.into_iter().collect())
}

fn truncate_first_delivery_body(body: &str) -> (String, bool) {
    truncate_chars(body, MAX_FIRST_DELIVERY_BODY_CHARS)
}

fn truncate_chars(body: &str, max_chars: usize) -> (String, bool) {
    let mut chars = body.chars();
    let taken: String = chars.by_ref().take(max_chars).collect();
    (taken, chars.next().is_some())
}

fn parent_quote_from_row(row: &QueryResult) -> Result<Option<(String, i32, String)>, DbError> {
    let Some(parent_id) = row.try_get::<Option<i32>>("", "parent_source_conversation_id")? else {
        return Ok(None);
    };
    let parent_title: Option<String> = row.try_get("", "parent_source_title")?;
    let parent_kind: Option<String> = row.try_get("", "parent_author_kind")?;
    let parent_body: Option<String> = row.try_get("", "parent_body")?;
    let human = parent_kind.as_deref() == Some("human");
    let label = parent_title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            if human {
                "the operator".to_string()
            } else {
                "Untitled Session".to_string()
            }
        });
    let (snippet, _) = truncate_chars(
        parent_body.as_deref().unwrap_or(""),
        MAX_PARENT_SNIPPET_CHARS,
    );
    let label = if human && label != "the operator" {
        format!("{label} (human)")
    } else {
        label
    };
    Ok(Some((label, parent_id, snippet)))
}

/// First delivery includes title + a bounded body. Consume is still
/// `read_message` / `read_room`; remaining Room body is `read_room_post`.
/// Overdue nags stay a short digest without repeating the body.
fn prompt_draft_from_delivery_row(row: &QueryResult) -> Result<PromptQueueDraft, DbError> {
    let event_id: String = row.try_get("", "event_id")?;
    let delivery_id: String = row.try_get("", "delivery_id")?;
    let source_title: Option<String> = row.try_get("", "source_title_snapshot")?;
    let source_conversation_id: i32 = row.try_get("", "source_conversation_id")?;
    let source_agent_type: String = row.try_get("", "source_agent_type_snapshot")?;
    let source_folder_path: Option<String> = row.try_get("", "source_folder_path_snapshot")?;
    let reply_to_event_id: Option<String> = row.try_get("", "reply_to_event_id")?;
    let expects_reply: i64 = row.try_get("", "effective_expects_reply")?;
    let subject: String = row
        .try_get::<Option<String>>("", "subject")?
        .unwrap_or_default();
    let body: String = row
        .try_get::<Option<String>>("", "body")?
        .unwrap_or_default();
    let letter_title = crate::acp::session_collaboration::letter_title(&subject, &body);
    let source_label = source_title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or("Untitled Session");
    let visibility: String = row
        .try_get::<Option<String>>("", "visibility")?
        .unwrap_or_else(|| "direct".to_string());
    let room_id: Option<String> = row.try_get("", "room_id")?;
    let is_room = visibility == "room" && room_id.is_some();
    let channel = if is_room { "room" } else { "mailbox" };
    let (body_for_prompt, truncated) = truncate_first_delivery_body(&body);
    let parent_quote = parent_quote_from_row(row)?;
    let mut metadata = serde_json::json!({
        "version": ENVELOPE_VERSION,
        "channel": channel,
        "kind": if is_room { "room_mention" } else { "letter" },
        "eventId": event_id,
        "deliveryId": delivery_id,
        "sourceConversationId": source_conversation_id,
        "sourceTitle": source_title,
        "sourceAgentType": source_agent_type,
        "sourceFolderPath": source_folder_path,
        "letterTitle": letter_title,
        "expectsReply": expects_reply != 0,
        "replyToEventId": reply_to_event_id,
        "visibility": visibility,
        "roomId": room_id,
        "bodyTruncated": truncated,
    });
    if let Some((parent_label, parent_id, parent_snippet)) = parent_quote.as_ref() {
        metadata["parentSourceConversationId"] = serde_json::json!(parent_id);
        metadata["parentSourceTitle"] = serde_json::json!(parent_label);
        metadata["parentSnippet"] = serde_json::json!(parent_snippet);
    }
    let metadata = serde_json::to_string(&metadata)
        .map_err(|err| validation(format!("Could not serialize collaboration envelope: {err}")))?;
    let reply_hint = if expects_reply != 0 {
        " This letter expects a reply after you read it."
    } else {
        ""
    };
    let quoted = match parent_quote.as_ref() {
        Some((label, parent_id, snippet)) if is_room => format!(
            "Quoted post by {label} (#{parent_id}): {snippet}\nQuoting does not wake that author. To wake them, also pass mention_session_ids.\n"
        ),
        Some((label, parent_id, snippet)) => format!(
            "Quoted letter by {label} (#{parent_id}): {snippet}\nreply_to_event_id quotes the parent; it does not wake that Session by itself.\n"
        ),
        None => String::new(),
    };
    let host_text = if is_room {
        let room = room_id.as_deref().unwrap_or("");
        let consume = if truncated {
            format!(
                "Body truncated after {MAX_FIRST_DELIVERY_BODY_CHARS} characters. Call read_room_post with event_id={event_id} offset={MAX_FIRST_DELIVERY_BODY_CHARS} for the rest. Call read_room with room_id={room} for surrounding posts."
            )
        } else {
            format!(
                "Call read_room with room_id={room} for surrounding posts (event_id={event_id})."
            )
        };
        format!(
            "channel={channel}\n\
This is a Codeg Room mention in {room}. It is not a private letter from Session {source_conversation_id}.\n\
Mention from {source_label} (#{source_conversation_id}).{reply_hint}\n\
{quoted}{consume} Reply with post_room using the same room_id and reply_to_event_id={event_id}. reply_to_event_id quotes the parent; it does not wake that author. To wake them, also pass mention_session_ids. Later supplements must also set reply_to_event_id or they start a new thread. Do not send_message a private letter unless asked."
        )
    } else {
        let consume = if truncated {
            format!(
                "Body truncated after {MAX_FIRST_DELIVERY_BODY_CHARS} characters. Call read_message with event_id={event_id} for the rest and to mark this letter read."
            )
        } else {
            format!(
                "Call read_message with event_id={event_id} to mark this letter read. Listing inbox does not mark it read."
            )
        };
        format!(
            "channel={channel}\n\
This is a Codeg mailbox letter from {source_label} (#{source_conversation_id}). It is not a Room post.\n\
Title: 《{letter_title}》.{reply_hint}\n\
{quoted}{consume} If a reply is needed, send_message to sourceConversationId and set reply_to_event_id={event_id}. Later supplements to the same thread must also set reply_to_event_id; omitting it starts a new root."
        )
    };
    let text = format!(
        "{ENVELOPE_PREFIX}{event_id}>>>\n{metadata}\n\
{host_text}\n\
--- message ---\n\
{body_for_prompt}\n\
{ENVELOPE_END_PREFIX}{event_id}>>>"
    );
    Ok(PromptQueueDraft {
        blocks: vec![PromptInputBlock::Text { text }],
        display_text: if is_room {
            format!("Codeg room: {letter_title}")
        } else {
            format!("Codeg mailbox: {letter_title}")
        },
    })
}

pub(crate) async fn prompt_draft_for_origin<C: ConnectionTrait>(
    conn: &C,
    target_conversation_id: i32,
    event_id: &str,
) -> Result<PromptQueueDraft, DbError> {
    let row = conn
        .query_one(statement(
            &format!(
                "{DELIVERY_ENVELOPE_SELECT} \
             WHERE d.event_id = ? AND d.target_conversation_id = ? \
               AND d.state NOT IN ('dismissed', 'failed')"
            ),
            vec![event_id.into(), target_conversation_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            validation(format!(
                "Collaboration event {event_id} has no delivery for Session {target_conversation_id}"
            ))
        })?;
    prompt_draft_from_delivery_row(&row)
}

/// Atomically reserve the oldest `store_only` deliveries for a Session's next
/// ordinary turn. The caller already owns the Session prompt lock, so this DB
/// transition is the durable boundary between multiple views/ingresses. A
/// prompt-queue item whose own origin is a collaboration event is deliberately
/// excluded: one invoked request must remain one unambiguous reply obligation.
pub(crate) async fn claim_pending_store_only_for_turn(
    conn: &DatabaseConnection,
    target_conversation_id: i32,
    turn_ref: &str,
) -> Result<Option<ClaimedStoreOnlyBatch>, DbError> {
    if turn_ref.trim().is_empty() || turn_ref.len() > MAX_DEDUPE_ID_BYTES {
        return Err(validation(format!(
            "Collaboration turn reference must contain between 1 and {MAX_DEDUPE_ID_BYTES} bytes"
        )));
    }

    let txn = conn.begin().await?;
    let invoked_origin: i64 = txn
        .query_one(statement(
            "SELECT EXISTS (SELECT 1 FROM conversation_prompt_queue_item \
             WHERE conversation_id = ? AND id = ? AND origin_event_id IS NOT NULL) AS found",
            vec![target_conversation_id.into(), turn_ref.into()],
        ))
        .await?
        .ok_or_else(|| validation("Could not inspect the current prompt queue item"))?
        .try_get("", "found")?;
    if invoked_origin != 0 {
        txn.commit().await?;
        return Ok(None);
    }

    let rows = txn
        .query_all(statement(
            &format!(
                "{DELIVERY_ENVELOPE_SELECT} \
                 WHERE d.target_conversation_id = ? \
                   AND d.invocation_policy = 'store_only' AND d.state = 'pending' \
                 ORDER BY d.created_at ASC, d.rowid ASC \
                 LIMIT {MAX_STORE_ONLY_DELIVERIES_PER_TURN}"
            ),
            vec![target_conversation_id.into()],
        ))
        .await?;

    let mut event_ids = Vec::new();
    let mut blocks = Vec::new();
    let mut participants = BTreeSet::from([target_conversation_id]);
    let mut envelope_bytes = 0usize;
    for row in rows {
        let draft = prompt_draft_from_delivery_row(&row)?;
        let next_bytes = draft
            .blocks
            .iter()
            .map(|block| match block {
                PromptInputBlock::Text { text } => text.len(),
                _ => 0,
            })
            .sum::<usize>();
        if !event_ids.is_empty()
            && envelope_bytes.saturating_add(next_bytes) > MAX_STORE_ONLY_ENVELOPE_BYTES_PER_TURN
        {
            break;
        }

        let event_id: String = row.try_get("", "event_id")?;
        let source_conversation_id: i32 = row.try_get("", "source_conversation_id")?;
        let changed = txn
            .execute(statement(
                "UPDATE collaboration_delivery \
                 SET state = 'embedding', embedded_turn_ref = ?, attempts = attempts + 1, \
                     error = NULL, updated_at = CURRENT_TIMESTAMP \
                 WHERE event_id = ? AND target_conversation_id = ? \
                   AND invocation_policy = 'store_only' AND state = 'pending'",
                vec![
                    turn_ref.into(),
                    event_id.clone().into(),
                    target_conversation_id.into(),
                ],
            ))
            .await?
            .rows_affected()
            == 1;
        if !changed {
            continue;
        }
        envelope_bytes = envelope_bytes.saturating_add(next_bytes);
        event_ids.push(event_id);
        blocks.extend(draft.blocks);
        participants.insert(source_conversation_id);
    }

    if event_ids.is_empty() {
        txn.commit().await?;
        return Ok(None);
    }
    let affected_conversation_ids = bump_live_participants(&txn, participants).await?;
    txn.commit().await?;
    Ok(Some(ClaimedStoreOnlyBatch {
        blocks,
        event_ids,
        affected_conversation_ids,
        turn_ref: turn_ref.to_string(),
    }))
}

async fn transition_store_only_batch(
    conn: &DatabaseConnection,
    target_conversation_id: i32,
    batch: &ClaimedStoreOnlyBatch,
    next_state: &str,
) -> Result<Vec<i32>, DbError> {
    if next_state != "embedded" && next_state != "pending" {
        return Err(validation("Invalid store-only batch transition"));
    }
    let txn = conn.begin().await?;
    let mut participants = BTreeSet::from([target_conversation_id]);
    let mut changed_any = false;
    for event_id in &batch.event_ids {
        let source = source_for_origin(&txn, target_conversation_id, event_id).await?;
        let clear_turn_ref = next_state == "pending";
        let sql = if clear_turn_ref {
            "UPDATE collaboration_delivery \
             SET state = 'pending', embedded_turn_ref = NULL, error = NULL, \
                 updated_at = CURRENT_TIMESTAMP \
             WHERE event_id = ? AND target_conversation_id = ? \
               AND invocation_policy = 'store_only' AND state = 'embedding' \
               AND embedded_turn_ref = ?"
        } else {
            "UPDATE collaboration_delivery \
             SET state = 'embedded', error = NULL, \
                 updated_at = CURRENT_TIMESTAMP \
             WHERE event_id = ? AND target_conversation_id = ? \
               AND invocation_policy = 'store_only' AND state = 'embedding' \
               AND embedded_turn_ref = ?"
        };
        let values = vec![
            event_id.clone().into(),
            target_conversation_id.into(),
            batch.turn_ref.clone().into(),
        ];
        let changed = txn.execute(statement(sql, values)).await?.rows_affected() == 1;
        if changed {
            changed_any = true;
            participants.insert(source);
        }
    }
    let affected = if changed_any {
        bump_live_participants(&txn, participants).await?
    } else {
        Vec::new()
    };
    txn.commit().await?;
    Ok(affected)
}

pub(crate) async fn mark_store_only_batch_embedded(
    conn: &DatabaseConnection,
    target_conversation_id: i32,
    batch: &ClaimedStoreOnlyBatch,
) -> Result<Vec<i32>, DbError> {
    transition_store_only_batch(conn, target_conversation_id, batch, "embedded").await
}

/// A known pre-dispatch failure is safe to retry. A process crash after the
/// command may have reached the Harness leaves `embedding` untouched instead;
/// Codeg must not silently duplicate a message whose outcome is unknown.
pub(crate) async fn release_store_only_batch(
    conn: &DatabaseConnection,
    target_conversation_id: i32,
    batch: &ClaimedStoreOnlyBatch,
) -> Result<Vec<i32>, DbError> {
    transition_store_only_batch(conn, target_conversation_id, batch, "pending").await
}

pub(crate) async fn delivery_hint_for_origin<C: ConnectionTrait>(
    conn: &C,
    target_conversation_id: i32,
    event_id: &str,
) -> Result<CollaborationDeliveryHint, DbError> {
    // No invocation-policy filter: a reminder may legitimately queue an
    // origin item for a `store_only` delivery (read-but-unanswered nag), and
    // rejecting it here would poison the claimed queue head forever.
    let row = conn
        .query_one(statement(
            "SELECT delivery_hint FROM collaboration_delivery \
             WHERE event_id = ? AND target_conversation_id = ?",
            vec![event_id.into(), target_conversation_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            validation(format!(
                "Queued collaboration event {event_id} has no delivery hint for Session {target_conversation_id}"
            ))
        })?;
    let raw: String = row.try_get("", "delivery_hint")?;
    CollaborationDeliveryHint::parse(&raw)
        .ok_or_else(|| validation(format!("Invalid collaboration delivery hint: {raw}")))
}

async fn source_for_origin<C: ConnectionTrait>(
    conn: &C,
    target_conversation_id: i32,
    event_id: &str,
) -> Result<i32, DbError> {
    conn.query_one(statement(
        "SELECT e.source_conversation_id \
         FROM collaboration_delivery d JOIN collaboration_event e ON e.id = d.event_id \
         WHERE d.event_id = ? AND d.target_conversation_id = ?",
        vec![event_id.into(), target_conversation_id.into()],
    ))
    .await?
    .ok_or_else(|| validation(format!("Collaboration delivery {event_id} is missing")))?
    .try_get("", "source_conversation_id")
    .map_err(DbError::from)
}

async fn bump_origin_participants(
    txn: &DatabaseTransaction,
    target_conversation_id: i32,
    event_id: &str,
) -> Result<Vec<i32>, DbError> {
    let source = source_for_origin(txn, target_conversation_id, event_id).await?;
    bump_live_participants(txn, [source, target_conversation_id]).await
}

pub(crate) async fn origin_participants<C: ConnectionTrait>(
    conn: &C,
    target_conversation_id: i32,
    event_id: &str,
) -> Result<Vec<i32>, DbError> {
    let source = source_for_origin(conn, target_conversation_id, event_id).await?;
    let mut participants = Vec::with_capacity(2);
    for id in [source, target_conversation_id] {
        if live_session(conn, id).await?.is_some() {
            participants.push(id);
        }
    }
    participants.sort_unstable();
    participants.dedup();
    Ok(participants)
}

pub(crate) async fn mark_origin_embedding(
    txn: &DatabaseTransaction,
    target_conversation_id: i32,
    event_id: &str,
    turn_ref: &str,
) -> Result<bool, DbError> {
    // `queued` is the normal hand-off state, but two resting states are also
    // legal dispatch sources: `pending` (a store_only letter re-delivered by
    // the reminder before the promotion in `enqueue_origin` existed) and
    // `embedded` (an already-injected letter being nagged again). Dismissed
    // and failed deliveries stay ineligible.
    let changed = txn
        .execute(statement(
            "UPDATE collaboration_delivery \
             SET state = 'embedding', embedded_turn_ref = ?, attempts = attempts + 1, \
                 error = NULL, updated_at = CURRENT_TIMESTAMP \
             WHERE event_id = ? AND target_conversation_id = ? \
               AND state IN ('queued', 'pending', 'embedded')",
            vec![
                turn_ref.into(),
                event_id.into(),
                target_conversation_id.into(),
            ],
        ))
        .await?
        .rows_affected()
        == 1;
    if changed {
        bump_origin_participants(txn, target_conversation_id, event_id).await?;
    }
    Ok(changed)
}

/// A reminder re-delivers a letter by queueing a fresh origin item. Deliveries
/// rest in `pending` (store_only) or `embedded` (already injected once);
/// promote them back to `queued` so the origin dispatch protocol
/// (queued → embedding → embedded) holds end to end. Refuse deliveries that
/// cannot be rendered at claim time (dismissed/failed/missing) and ones whose
/// dispatch is literally in flight (`embedding`): enqueueing those would park
/// an unclaimable item at the queue head.
pub(crate) async fn prepare_origin_redelivery(
    txn: &DatabaseTransaction,
    target_conversation_id: i32,
    event_id: &str,
) -> Result<bool, DbError> {
    let Some(row) = txn
        .query_one(statement(
            "SELECT state FROM collaboration_delivery \
             WHERE event_id = ? AND target_conversation_id = ?",
            vec![event_id.into(), target_conversation_id.into()],
        ))
        .await?
    else {
        return Ok(false);
    };
    let state: String = row.try_get("", "state")?;
    match state.as_str() {
        "queued" => Ok(true),
        "pending" | "embedded" => {
            txn.execute(statement(
                "UPDATE collaboration_delivery \
                 SET state = 'queued', error = NULL, updated_at = CURRENT_TIMESTAMP \
                 WHERE event_id = ? AND target_conversation_id = ? \
                   AND state IN ('pending', 'embedded')",
                vec![event_id.into(), target_conversation_id.into()],
            ))
            .await?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub(crate) async fn mark_origin_queued(
    txn: &DatabaseTransaction,
    target_conversation_id: i32,
    event_id: &str,
) -> Result<bool, DbError> {
    let changed = txn
        .execute(statement(
            "UPDATE collaboration_delivery \
             SET state = 'queued', embedded_turn_ref = NULL, error = NULL, \
                 updated_at = CURRENT_TIMESTAMP \
             WHERE event_id = ? AND target_conversation_id = ? AND state = 'embedding'",
            vec![event_id.into(), target_conversation_id.into()],
        ))
        .await?
        .rows_affected()
        == 1;
    if changed {
        bump_origin_participants(txn, target_conversation_id, event_id).await?;
    }
    Ok(changed)
}

pub(crate) async fn mark_origin_embedded(
    txn: &DatabaseTransaction,
    target_conversation_id: i32,
    event_id: &str,
    turn_ref: &str,
) -> Result<bool, DbError> {
    let changed = txn
        .execute(statement(
            "UPDATE collaboration_delivery \
             SET state = 'embedded', embedded_turn_ref = ?, error = NULL, \
                  updated_at = CURRENT_TIMESTAMP \
             WHERE event_id = ? AND target_conversation_id = ? AND state = 'embedding'",
            vec![
                turn_ref.into(),
                event_id.into(),
                target_conversation_id.into(),
            ],
        ))
        .await?
        .rows_affected()
        == 1;
    if changed {
        bump_origin_participants(txn, target_conversation_id, event_id).await?;
    }
    Ok(changed)
}

pub(crate) async fn mark_origin_failed(
    txn: &DatabaseTransaction,
    target_conversation_id: i32,
    event_id: &str,
    reason: &str,
) -> Result<bool, DbError> {
    let changed = txn
        .execute(statement(
            "UPDATE collaboration_delivery \
             SET state = 'failed', error = ?, updated_at = CURRENT_TIMESTAMP \
             WHERE event_id = ? AND target_conversation_id = ? \
               AND state IN ('queued', 'embedding')",
            vec![
                reason.into(),
                event_id.into(),
                target_conversation_id.into(),
            ],
        ))
        .await?
        .rows_affected()
        == 1;
    if changed {
        bump_origin_participants(txn, target_conversation_id, event_id).await?;
    }
    Ok(changed)
}

/// Persist the normal final answer of a turn whose *main user message* was one
/// exact collaboration delivery. `completed_message_id` is the stable id that
/// the prompt queue handed to the Harness; matching it against
/// `embedded_turn_ref` prevents ordinary user turns and native steering from
/// being mistaken for collaboration replies.
///
/// An Agent may already have answered explicitly with `send_message` during
/// the turn. The `NOT EXISTS` guard makes this a fallback, not a duplicate
/// second reply. The deterministic dedupe id also makes a replayed terminal
/// event harmless.
pub(crate) async fn auto_reply_for_completed_turn(
    conn: &DatabaseConnection,
    target_conversation_id: i32,
    completed_message_id: &str,
    assistant_text: &str,
) -> Result<Option<CollaborationSendResult>, DbError> {
    if completed_message_id.trim().is_empty() || assistant_text.trim().is_empty() {
        return Ok(None);
    }
    let Some(row) = conn
        .query_one(statement(
            "SELECT e.id AS event_id, e.source_conversation_id, e.visibility, e.room_id \
             FROM collaboration_delivery d \
             JOIN collaboration_event e ON e.id = d.event_id \
             WHERE d.target_conversation_id = ? \
               AND d.embedded_turn_ref = ? \
               AND d.state = 'embedded' \
               AND d.invocation_policy = 'invoke_when_idle' \
               AND e.expects_reply = 1 \
               AND d.agent_received_at IS NOT NULL \
               AND d.obligation_state = 'awaiting_reply' \
               AND NOT EXISTS ( \
                   SELECT 1 FROM collaboration_event reply \
                   WHERE reply.reply_to_event_id = e.id \
                     AND reply.source_conversation_id = d.target_conversation_id \
               ) \
               AND (SELECT COUNT(*) FROM collaboration_delivery same \
                     WHERE same.target_conversation_id = d.target_conversation_id \
                       AND same.embedded_turn_ref = d.embedded_turn_ref) = 1 \
             ORDER BY d.updated_at DESC, d.id DESC LIMIT 1",
            vec![target_conversation_id.into(), completed_message_id.into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    let event_id: String = row.try_get("", "event_id")?;
    let original_source_conversation_id: i32 = row.try_get("", "source_conversation_id")?;
    let visibility: String = row
        .try_get::<Option<String>>("", "visibility")?
        .unwrap_or_else(|| "direct".to_string());
    let room_id: Option<String> = row.try_get("", "room_id")?;
    let body =
        bounded_auto_reply_body(target_conversation_id, completed_message_id, assistant_text);
    if visibility == "room" {
        let Some(room_id) = room_id else {
            return Ok(None);
        };
        let posted = post_room(
            conn,
            PostRoomMessageInput {
                room_id,
                source_conversation_id: target_conversation_id,
                target_conversation_ids: vec![],
                mention_all: false,
                body,
                client_dedupe_id: format!(
                    "auto-reply:{event_id}:{target_conversation_id}:{completed_message_id}"
                ),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: Some(event_id),
                mention_human: false,
                author_kind: Default::default(),
            },
        )
        .await?;
        return Ok(Some(CollaborationSendResult {
            event_id: posted.event_id,
            deliveries: posted.deliveries,
            affected_conversation_ids: posted.affected_conversation_ids,
            deduplicated: posted.deduplicated,
        }));
    }
    send_with_initially_inactive_targets_guarded(
        conn,
        SendCollaborationMessageInput {
            source_conversation_id: target_conversation_id,
            target_conversation_ids: vec![original_source_conversation_id],
            subject: "Auto reply".into(),
            body,
            // The completed queue message id contains the execution attempt.
            // A real retry is a second turn and must be allowed to publish its
            // own answer rather than deduplicating against the earlier one.
            client_dedupe_id: format!(
                "auto-reply:{event_id}:{target_conversation_id}:{completed_message_id}"
            ),
            invocation_policy: CollaborationInvocationPolicy::StoreOnly,
            delivery_hint: CollaborationDeliveryHint::Default,
            expects_reply: false,
            urgency: CollaborationUrgency::Normal,
            reply_to_event_id: Some(event_id.clone()),
        },
        &HashSet::new(),
        Some(ReplyInsertGuard {
            reply_to_event_id: event_id,
            reply_source_conversation_id: target_conversation_id,
        }),
    )
    .await
}

fn bounded_auto_reply_body(
    source_conversation_id: i32,
    completed_message_id: &str,
    assistant_text: &str,
) -> String {
    if assistant_text.len() <= MAX_BODY_BYTES {
        return assistant_text.to_string();
    }
    let suffix = format!(
        "\n\n[Reply truncated at Codeg's message limit. Open the full source Session](codeg://session/{source_conversation_id}) (turn `{completed_message_id}`)."
    );
    let available = MAX_BODY_BYTES.saturating_sub(suffix.len());
    let mut boundary = available.min(assistant_text.len());
    while boundary > 0 && !assistant_text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    let mut body = assistant_text[..boundary].to_string();
    body.push_str(&suffix);
    body
}

#[derive(Debug, Clone)]
struct ReplyInsertGuard {
    reply_to_event_id: String,
    reply_source_conversation_id: i32,
}

pub(crate) struct OriginRetryIdentity {
    pub queue_item_id: String,
    pub queue_dedupe_id: String,
}

/// Prepare one new execution attempt for a delivery. Each attempt gets a new
/// queue/message id so viewers cannot mistake a real retry for a replay of the
/// prior user turn. `allow_queued` is reserved for an initially-paused inactive
/// target; ordinary retries must come from the failed delivery state.
pub(crate) async fn prepare_origin_retry_with_queued_confirmation(
    txn: &DatabaseTransaction,
    target_conversation_id: i32,
    event_id: &str,
    allow_queued: bool,
) -> Result<Option<OriginRetryIdentity>, DbError> {
    let Some(row) = txn
        .query_one(statement(
            "SELECT id, state, attempts FROM collaboration_delivery \
             WHERE event_id = ? AND target_conversation_id = ?",
            vec![event_id.into(), target_conversation_id.into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    let delivery_id: String = row.try_get("", "id")?;
    let state: String = row.try_get("", "state")?;
    let attempts: i32 = row.try_get("", "attempts")?;
    match state.as_str() {
        "failed" => {
            let changed = txn
                .execute(statement(
                    "UPDATE collaboration_delivery \
                     SET state = 'queued', embedded_turn_ref = NULL, error = NULL, \
                         updated_at = CURRENT_TIMESTAMP \
                     WHERE id = ? AND state = 'failed'",
                    vec![delivery_id.clone().into()],
                ))
                .await?
                .rows_affected()
                == 1;
            if !changed {
                return Ok(None);
            }
        }
        "queued" if allow_queued => {}
        _ => return Ok(None),
    }
    let next_attempt = attempts
        .checked_add(1)
        .ok_or_else(|| validation("Collaboration delivery attempt counter overflowed"))?;
    bump_origin_participants(txn, target_conversation_id, event_id).await?;
    Ok(Some(OriginRetryIdentity {
        queue_item_id: format!("{delivery_id}#{next_attempt}"),
        queue_dedupe_id: format!("{delivery_id}:{next_attempt}"),
    }))
}

/// Persist one immutable event and all per-target deliveries atomically.
///
/// A retry with the same `(source_conversation_id, client_dedupe_id)` returns
/// the original event and original delivery set. In particular, adding targets
/// to a retry never silently expands fan-out. A changed body or metadata is a
/// caller error because it would make the idempotency key ambiguous.
///
/// A letter addressed to an archived Session is rejected before the event is
/// persisted, naming every archived target, so the sender learns immediately
/// instead of the mail rotting unread. Restore the Session to accept mail.
///
/// `#[cfg(test)]`: the only production call site
/// (`commands::collaboration::persist_collaboration_message`) calls
/// `send_with_initially_inactive_targets` directly, this wrapper's
/// zero-inactive-targets case included. Every caller is a `#[cfg(test)]`
/// module using it as a convenience fixture builder, so the wrapper only
/// exists in test builds (dead code otherwise).
#[cfg(test)]
pub(crate) async fn send(
    conn: &DatabaseConnection,
    input: SendCollaborationMessageInput,
) -> Result<CollaborationSendResult, DbError> {
    send_with_initially_inactive_targets(conn, input, &HashSet::new()).await
}

/// Persist a collaboration event while preventing an inactive target from
/// unexpectedly spending tokens the next time the user merely opens it.
/// Ordinary same-Session follow-ups retain their existing resume behavior;
/// only cross-Session invocation rows named here start paused.
pub(crate) async fn send_with_initially_inactive_targets(
    conn: &DatabaseConnection,
    input: SendCollaborationMessageInput,
    inactive_target_ids: &HashSet<i32>,
) -> Result<CollaborationSendResult, DbError> {
    send_with_initially_inactive_targets_guarded(conn, input, inactive_target_ids, None)
        .await?
        .ok_or_else(|| validation("Unguarded collaboration send was not persisted"))
}

async fn send_with_initially_inactive_targets_guarded(
    conn: &DatabaseConnection,
    input: SendCollaborationMessageInput,
    inactive_target_ids: &HashSet<i32>,
    reply_guard: Option<ReplyInsertGuard>,
) -> Result<Option<CollaborationSendResult>, DbError> {
    let target_ids = validate_input(&input)?;
    let txn = conn.begin().await?;
    let source = require_live_session(&txn, input.source_conversation_id).await?;

    if let Some(reply_to) = input.reply_to_event_id.as_deref() {
        validate_direct_reply(&txn, reply_to).await?;
        validate_reply_relation(&txn, input.source_conversation_id, &target_ids, reply_to).await?;
    }
    let chain_depth = child_chain_depth(&txn, input.reply_to_event_id.as_deref()).await?;
    // Mailbox letters are always Agent-authored, so the chain-depth fuse
    // applies to every send: at the limit the letter still lands, but it can
    // no longer ask for a reply. Human UI sends go through post_room, which
    // applies the fuse to Agent authors only.
    let expects_reply = input.expects_reply && chain_depth < MAX_AGENT_REPLY_CHAIN_DEPTH;

    if let Some(event_id) =
        event_id_for_dedupe(&txn, input.source_conversation_id, &input.client_dedupe_id).await?
    {
        validate_dedupe_payload(&txn, &event_id, &input, expects_reply).await?;
        let deliveries = deliveries_for_event(&txn, &event_id).await?;
        let mut affected = BTreeSet::from([input.source_conversation_id]);
        affected.extend(deliveries.iter().map(|item| item.target.conversation_id));
        txn.commit().await?;
        return Ok(Some(CollaborationSendResult {
            event_id,
            deliveries,
            affected_conversation_ids: affected.into_iter().collect(),
            deduplicated: true,
        }));
    }

    // Archived Sessions refuse new mail. Reject the whole send before the
    // event exists so the caller learns immediately which targets are
    // archived instead of the letter rotting in a mailbox nobody watches.
    // Dedupe replays above still return the original fan-out.
    let mut archived_targets: Vec<String> = Vec::new();
    for target_id in &target_ids {
        if let Some(target) = live_session(&txn, *target_id).await? {
            if target.archived {
                archived_targets.push(match target.title.as_deref() {
                    Some(title) => format!("Session {target_id} ({title:?})"),
                    None => format!("Session {target_id}"),
                });
            }
        }
    }
    if !archived_targets.is_empty() {
        return Err(validation(format!(
            "Archived Sessions do not accept new mail: {}. Unarchive before sending.",
            archived_targets.join(", ")
        )));
    }

    let event_id = uuid::Uuid::new_v4().to_string();
    let subject = crate::acp::session_collaboration::normalize_letter_title(&input.subject)
        .map_err(validation)?;
    let inserted = if let Some(guard) = reply_guard.as_ref() {
        txn.execute(statement(
            "INSERT OR IGNORE INTO collaboration_event \
             (id, source_conversation_id, source_title_snapshot, source_agent_type_snapshot, \
              source_folder_path_snapshot, source_backend_snapshot, subject, body, reply_to_event_id, \
              expects_reply, urgency, client_dedupe_id, chain_depth, created_at) \
             SELECT ?, ?, ?, ?, ?, 'current', ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP \
             WHERE NOT EXISTS ( \
                 SELECT 1 FROM collaboration_event reply \
                 WHERE reply.reply_to_event_id = ? \
                   AND reply.source_conversation_id = ? \
             )",
            vec![
                event_id.clone().into(),
                source.id.into(),
                source.title.clone().into(),
                source.agent_type.clone().into(),
                source.folder_path.clone().into(),
                subject.clone().into(),
                input.body.clone().into(),
                input.reply_to_event_id.clone().into(),
                (expects_reply as i32).into(),
                input.urgency.as_str().into(),
                input.client_dedupe_id.clone().into(),
                chain_depth.into(),
                guard.reply_to_event_id.clone().into(),
                guard.reply_source_conversation_id.into(),
            ],
        ))
        .await?
    } else {
        txn.execute(statement(
            "INSERT OR IGNORE INTO collaboration_event \
         (id, source_conversation_id, source_title_snapshot, source_agent_type_snapshot, \
          source_folder_path_snapshot, source_backend_snapshot, subject, body, reply_to_event_id, \
          expects_reply, urgency, client_dedupe_id, chain_depth, created_at) \
         VALUES (?, ?, ?, ?, ?, 'current', ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)",
            vec![
                event_id.clone().into(),
                source.id.into(),
                source.title.clone().into(),
                source.agent_type.clone().into(),
                source.folder_path.clone().into(),
                subject.clone().into(),
                input.body.clone().into(),
                input.reply_to_event_id.clone().into(),
                (expects_reply as i32).into(),
                input.urgency.as_str().into(),
                input.client_dedupe_id.clone().into(),
                chain_depth.into(),
            ],
        ))
        .await?
    };
    // Close the double-click / two-window race between the optimistic dedupe
    // lookup above and the write. The unique key is the arbiter; a loser reads
    // and returns the already-committed fan-out instead of creating a second
    // event or silently appending its possibly different target list.
    if inserted.rows_affected() == 0 {
        if let Some(existing_id) =
            event_id_for_dedupe(&txn, input.source_conversation_id, &input.client_dedupe_id).await?
        {
            validate_dedupe_payload(&txn, &existing_id, &input, expects_reply).await?;
            let deliveries = deliveries_for_event(&txn, &existing_id).await?;
            let mut affected = BTreeSet::from([input.source_conversation_id]);
            affected.extend(deliveries.iter().map(|item| item.target.conversation_id));
            txn.commit().await?;
            return Ok(Some(CollaborationSendResult {
                event_id: existing_id,
                deliveries,
                affected_conversation_ids: affected.into_iter().collect(),
                deduplicated: true,
            }));
        }
        if reply_guard.is_some() {
            txn.commit().await?;
            return Ok(None);
        }
        return Err(validation(
            "Collaboration dedupe race did not resolve to an event",
        ));
    }

    // A reply clears only the obligation owned by this exact recipient of the
    // parent event. Fan-out siblings remain independently awaiting a reply.
    if let Some(reply_to_event_id) = input.reply_to_event_id.as_deref() {
        txn.execute(statement(
            "UPDATE collaboration_delivery \
             SET obligation_state = 'resolved', \
                 obligation_resolved_at = COALESCE(obligation_resolved_at, CURRENT_TIMESTAMP), \
                 updated_at = CURRENT_TIMESTAMP \
             WHERE event_id = ? AND target_conversation_id = ? \
               AND obligation_state = 'awaiting_reply'",
            vec![reply_to_event_id.into(), source.id.into()],
        ))
        .await?;
    }

    let mut affected = BTreeSet::from([source.id]);
    ensure_state(&txn, source.id).await?;
    bump_revision(&txn, source.id).await?;

    for target_id in target_ids {
        let target = live_session(&txn, target_id).await?;
        let (snapshot, state, error) = match target.as_ref() {
            Some(target) => (
                target.snapshot(),
                match input.invocation_policy {
                    CollaborationInvocationPolicy::StoreOnly => "pending",
                    CollaborationInvocationPolicy::InvokeWhenIdle => "queued",
                },
                None,
            ),
            None => (
                CollaborationSessionSnapshot {
                    conversation_id: target_id,
                    title: None,
                    agent_type: None,
                    folder_path: None,
                    backend: "current".to_string(),
                },
                "failed",
                Some("target_not_found".to_string()),
            ),
        };
        let delivery_id = uuid::Uuid::new_v4().to_string();
        let obligation_state = if expects_reply && target.is_some() {
            CollaborationObligationState::AwaitingReply
        } else {
            CollaborationObligationState::None
        };
        txn.execute(statement(
            "INSERT INTO collaboration_delivery \
              (id, event_id, target_conversation_id, target_title_snapshot, \
               target_agent_type_snapshot, target_folder_path_snapshot, invocation_policy, \
               delivery_hint, state, obligation_state, obligation_created_at, \
               attempts, error, created_at, updated_at) \
              VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, \
                      CASE WHEN ? = 1 THEN CURRENT_TIMESTAMP ELSE NULL END, \
                      0, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            vec![
                delivery_id.clone().into(),
                event_id.clone().into(),
                target_id.into(),
                snapshot.title.into(),
                snapshot.agent_type.into(),
                snapshot.folder_path.into(),
                input.invocation_policy.as_str().into(),
                input.delivery_hint.as_str().into(),
                state.into(),
                obligation_state.as_str().into(),
                (expects_reply as i32).into(),
                error.into(),
            ],
        ))
        .await?;
        if target.is_some() {
            if input.invocation_policy == CollaborationInvocationPolicy::InvokeWhenIdle {
                let pause_reason = inactive_target_ids
                    .contains(&target_id)
                    .then_some(INACTIVE_TARGET_CONFIRMATION_REASON);
                prompt_queue_service::enqueue_origin_in_transaction(
                    &txn,
                    target_id,
                    &delivery_id,
                    &event_id,
                    &delivery_id,
                    pause_reason,
                    crate::models::prompt_queue::PromptQueueSource::Collaboration,
                )
                .await?;
            }
            ensure_state(&txn, target_id).await?;
            bump_revision(&txn, target_id).await?;
            affected.insert(target_id);
        }
    }

    let deliveries = deliveries_for_event(&txn, &event_id).await?;
    txn.commit().await?;
    Ok(Some(CollaborationSendResult {
        event_id,
        deliveries,
        affected_conversation_ids: affected.into_iter().collect(),
        deduplicated: false,
    }))
}

/// A mailbox reply must quote a mailbox event, symmetric with
/// `validate_room_reply`. Answering a Room post with a private letter would
/// clear the Room obligation while the Room timeline never sees the answer;
/// the whole membership loses the thread.
async fn validate_direct_reply<C: ConnectionTrait>(
    conn: &C,
    reply_to_event_id: &str,
) -> Result<(), DbError> {
    let row = conn
        .query_one(statement(
            "SELECT COALESCE(visibility, 'direct') AS visibility \
             FROM collaboration_event WHERE id = ?",
            vec![reply_to_event_id.into()],
        ))
        .await?
        .ok_or_else(|| DbError::NotFound(format!("Collaboration event {reply_to_event_id}")))?;
    let visibility: String = row.try_get("", "visibility")?;
    if visibility != "direct" {
        return Err(validation(format!(
            "Collaboration event {reply_to_event_id} is a Room post; answer it with post_room in the same Room, not send_message"
        )));
    }
    Ok(())
}

async fn validate_room_reply<C: ConnectionTrait>(
    conn: &C,
    room_id: &str,
    reply_to_event_id: &str,
) -> Result<(), DbError> {
    let row = conn
        .query_one(statement(
            "SELECT visibility, room_id FROM collaboration_event WHERE id = ?",
            vec![reply_to_event_id.into()],
        ))
        .await?
        .ok_or_else(|| DbError::NotFound(format!("Collaboration event {reply_to_event_id}")))?;
    let visibility: String = row
        .try_get::<Option<String>>("", "visibility")?
        .unwrap_or_else(|| "direct".to_string());
    let parent_room: Option<String> = row.try_get("", "room_id")?;
    if visibility != "room" || parent_room.as_deref() != Some(room_id) {
        return Err(validation(format!(
            "Room reply {reply_to_event_id} must target an event in the same Room"
        )));
    }
    Ok(())
}

/// Obligations `conversation_id` still owes inside one Room. Same shape as
/// `collaboration_room_service::MEMBER_NEEDS_REPLY_SQL`, narrowed to a single
/// Room: `post_room` reports it back so a poster who cleared nothing learns
/// what is still on their tab without a second `read_room` round trip.
async fn open_room_reply_debt<C: ConnectionTrait>(
    conn: &C,
    room_id: &str,
    conversation_id: i32,
) -> Result<u32, DbError> {
    let row = conn
        .query_one(statement(
            "SELECT COUNT(*) AS count FROM collaboration_delivery d \
             JOIN collaboration_event e ON e.id = d.event_id \
             WHERE d.target_conversation_id = ? AND e.room_id = ? \
               AND COALESCE(e.visibility, 'direct') = 'room' \
               AND d.obligation_state = 'awaiting_reply' \
               AND d.state <> 'dismissed' AND d.state <> 'failed'",
            vec![conversation_id.into(), room_id.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let count: i64 = row.try_get("", "count")?;
    Ok(count.max(0) as u32)
}

fn validate_room_post(input: &PostRoomMessageInput) -> Result<(), DbError> {
    if input.body.trim().is_empty() {
        return Err(validation("A Room message body cannot be empty"));
    }
    if input.body.len() > MAX_BODY_BYTES {
        return Err(validation("A Room message body is too large"));
    }
    let dedupe_id = input.client_dedupe_id.trim();
    if dedupe_id.is_empty() || dedupe_id.len() > MAX_DEDUPE_ID_BYTES {
        return Err(validation(format!(
            "client_dedupe_id must contain between 1 and {MAX_DEDUPE_ID_BYTES} bytes"
        )));
    }
    if input.delivery_hint == CollaborationDeliveryHint::SteerIfSupported
        && input.invocation_policy != CollaborationInvocationPolicy::InvokeWhenIdle
    {
        return Err(validation("steer_if_supported requires invoke_when_idle"));
    }
    Ok(())
}

/// Persist a Room-visible event. Empty structured mentions is a record-only
/// post: every member can read it, nobody is invoked. Free-text `@word` is
/// ignored. Structured Session URIs / mention_session_ids still create
/// deliveries. An archived member stays mentioned on the ledger, but the
/// Delivery is marked `failed` with reason `target_archived` so the poster
/// sees who was skipped; the member is not enqueued and gets no
/// awaiting-reply obligation. Only a `session` author is dropped from its own
/// mention list; a `human` author borrows the creator's id for the ledger and
/// may `@` that creator like anybody else. Mail projections
/// never see these events. The result reports the ledger effect of this very
/// post — which obligation it cleared, and what the author still owes in the
/// Room — because posting is the only thing that clears a debt and the caller
/// should not have to re-read the Room to find out whether it worked.
pub async fn post_room(
    conn: &DatabaseConnection,
    input: PostRoomMessageInput,
) -> Result<RoomPostResult, DbError> {
    validate_room_post(&input)?;
    let txn = conn.begin().await?;
    let _workbench_id = collaboration_room_service::room_workbench_id(&txn, &input.room_id).await?;
    let created_by =
        collaboration_room_service::created_by_conversation_id(&txn, &input.room_id).await?;
    let ledger_source_id = if input.author_kind == CollaborationAuthorKind::Human {
        created_by
    } else {
        collaboration_room_service::require_member(
            &txn,
            &input.room_id,
            input.source_conversation_id,
        )
        .await?;
        input.source_conversation_id
    };
    let source = require_live_session(&txn, ledger_source_id).await?;
    let members = collaboration_room_service::member_ids(&txn, &input.room_id).await?;
    let member_set: HashSet<i32> = members.iter().copied().collect();
    let mut targets: BTreeSet<i32> = input.target_conversation_ids.iter().copied().collect();
    targets.extend(collaboration_mention::session_ids_from_structured_uris(
        &input.body,
    ));
    let mention_human = input.mention_human
        || collaboration_mention::mentions_human_from_structured_uris(&input.body);
    // Skipping the author is an authorship rule, not an id rule. A human post
    // only borrows the creator's Session id because
    // `collaboration_event.source_conversation_id` is NOT NULL — the person at
    // the keyboard is not that Session. So a human `@` of the creator, and an
    // @all from the UI, must fan out to them like any other member. A Session
    // author still cannot wake itself.
    let skip_author = match input.author_kind {
        CollaborationAuthorKind::Session => Some(ledger_source_id),
        CollaborationAuthorKind::Human => None,
    };
    if input.mention_all {
        for id in &members {
            if Some(*id) != skip_author {
                targets.insert(*id);
            }
        }
    }
    if let Some(author_id) = skip_author {
        targets.remove(&author_id);
    }
    if targets.len() > MAX_TARGETS {
        return Err(validation(format!(
            "A Room mention supports at most {MAX_TARGETS} targets"
        )));
    }
    for target in &targets {
        if !member_set.contains(target) {
            return Err(validation(format!(
                "Session {target} is not a member of Room {}",
                input.room_id
            )));
        }
    }
    if let Some(reply_to) = input.reply_to_event_id.as_deref() {
        validate_room_reply(&txn, &input.room_id, reply_to).await?;
    }
    let chain_depth = child_chain_depth(&txn, input.reply_to_event_id.as_deref()).await?;
    // The chain-depth fuse binds Agent authors only: a Room thread at the
    // limit still accepts the post but it can no longer ask for a reply.
    // Human UI posts stay unrestricted (red line).
    let expects_reply = input.expects_reply
        && (input.author_kind == CollaborationAuthorKind::Human
            || chain_depth < MAX_AGENT_REPLY_CHAIN_DEPTH);
    if let Some(event_id) =
        event_id_for_dedupe(&txn, ledger_source_id, &input.client_dedupe_id).await?
    {
        let deliveries = deliveries_for_event(&txn, &event_id).await?;
        let mut affected = BTreeSet::from([ledger_source_id]);
        affected.extend(deliveries.iter().map(|item| item.target.conversation_id));
        // A retry settles nothing of its own — the first call already ran the
        // resolve — but the live debt count still has to be truthful.
        let open_reply_debt = open_room_reply_debt(&txn, &input.room_id, source.id).await?;
        txn.commit().await?;
        return Ok(RoomPostResult {
            event_id,
            room_id: input.room_id,
            deliveries,
            affected_conversation_ids: affected.into_iter().collect(),
            deduplicated: true,
            cleared_reply_to_event_id: None,
            open_reply_debt,
        });
    }

    let event_id = uuid::Uuid::new_v4().to_string();
    // Room posts are single timeline messages: no subject. The column stays
    // for old rows and mailbox letters; timeline display derives from body.
    let subject = String::new();
    let (title_snapshot, agent_snapshot) = if input.author_kind == CollaborationAuthorKind::Human {
        (Some("You".to_string()), "human".to_string())
    } else {
        (source.title.clone(), source.agent_type.clone())
    };
    let inserted = txn
        .execute(statement(
            "INSERT OR IGNORE INTO collaboration_event \
             (id, source_conversation_id, source_title_snapshot, source_agent_type_snapshot, \
              source_folder_path_snapshot, source_backend_snapshot, subject, body, reply_to_event_id, \
              expects_reply, urgency, client_dedupe_id, chain_depth, visibility, room_id, \
              author_kind, mention_human, created_at) \
             VALUES (?, ?, ?, ?, ?, 'current', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)",
            vec![
                event_id.clone().into(),
                source.id.into(),
                title_snapshot.into(),
                agent_snapshot.into(),
                source.folder_path.clone().into(),
                subject.into(),
                input.body.clone().into(),
                input.reply_to_event_id.clone().into(),
                (expects_reply as i32).into(),
                input.urgency.as_str().into(),
                input.client_dedupe_id.clone().into(),
                chain_depth.into(),
                crate::models::CollaborationVisibility::Room.as_str().into(),
                input.room_id.clone().into(),
                input.author_kind.as_str().into(),
                (mention_human as i32).into(),
            ],
        ))
        .await?;
    if inserted.rows_affected() == 0 {
        if let Some(existing_id) =
            event_id_for_dedupe(&txn, ledger_source_id, &input.client_dedupe_id).await?
        {
            let deliveries = deliveries_for_event(&txn, &existing_id).await?;
            let mut affected = BTreeSet::from([input.source_conversation_id]);
            affected.extend(deliveries.iter().map(|item| item.target.conversation_id));
            let open_reply_debt = open_room_reply_debt(&txn, &input.room_id, source.id).await?;
            txn.commit().await?;
            return Ok(RoomPostResult {
                event_id: existing_id,
                room_id: input.room_id,
                deliveries,
                affected_conversation_ids: affected.into_iter().collect(),
                deduplicated: true,
                cleared_reply_to_event_id: None,
                open_reply_debt,
            });
        }
        return Err(validation(
            "Room message dedupe race did not resolve to an event",
        ));
    }

    // `rows_affected` is the ledger's own answer to "did I just pay a debt?".
    // Reporting it back is what keeps a replying Agent from having to call
    // read_room (which never clears debt) to find out.
    let mut cleared_reply_to_event_id = None;
    if let Some(reply_to_event_id) = input.reply_to_event_id.as_deref() {
        let resolved = txn
            .execute(statement(
                "UPDATE collaboration_delivery \
                 SET obligation_state = 'resolved', \
                     obligation_resolved_at = COALESCE(obligation_resolved_at, CURRENT_TIMESTAMP), \
                     updated_at = CURRENT_TIMESTAMP \
                 WHERE event_id = ? AND target_conversation_id = ? \
                   AND obligation_state = 'awaiting_reply'",
                vec![reply_to_event_id.into(), source.id.into()],
            ))
            .await?;
        if resolved.rows_affected() > 0 {
            cleared_reply_to_event_id = Some(reply_to_event_id.to_string());
        }
    }

    let mut affected = BTreeSet::from([source.id]);
    ensure_state(&txn, source.id).await?;
    bump_revision(&txn, source.id).await?;

    for target_id in targets {
        let target = live_session(&txn, target_id).await?;
        // Archived members stay in the Room and still get a Delivery so the
        // public timeline can show the @. They must not be woken: the
        // delivery fails with an explicit reason (naming the skipped member
        // to the poster), skips the prompt queue, and parks no obligation.
        let archived = target.as_ref().is_some_and(|session| session.archived);
        let invoke = input.invocation_policy == CollaborationInvocationPolicy::InvokeWhenIdle
            && target.as_ref().is_some_and(|session| !session.archived);
        let (snapshot, state, error) = match target.as_ref() {
            Some(target) if archived => (
                target.snapshot(),
                "failed",
                Some("target_archived".to_string()),
            ),
            Some(target) => (
                target.snapshot(),
                if invoke { "queued" } else { "pending" },
                None,
            ),
            None => (
                CollaborationSessionSnapshot {
                    conversation_id: target_id,
                    title: None,
                    agent_type: None,
                    folder_path: None,
                    backend: "current".to_string(),
                },
                "failed",
                Some("target_not_found".to_string()),
            ),
        };
        let delivery_id = uuid::Uuid::new_v4().to_string();
        let obligation_state = if expects_reply && target.is_some() && !archived {
            CollaborationObligationState::AwaitingReply
        } else {
            CollaborationObligationState::None
        };
        let invocation_policy = if archived {
            CollaborationInvocationPolicy::StoreOnly
        } else {
            input.invocation_policy
        };
        txn.execute(statement(
            "INSERT INTO collaboration_delivery \
              (id, event_id, target_conversation_id, target_title_snapshot, \
               target_agent_type_snapshot, target_folder_path_snapshot, invocation_policy, \
               delivery_hint, state, obligation_state, obligation_created_at, \
               attempts, error, created_at, updated_at) \
              VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, \
                      CASE WHEN ? = 1 THEN CURRENT_TIMESTAMP ELSE NULL END, \
                      0, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            vec![
                delivery_id.clone().into(),
                event_id.clone().into(),
                target_id.into(),
                snapshot.title.into(),
                snapshot.agent_type.into(),
                snapshot.folder_path.into(),
                invocation_policy.as_str().into(),
                input.delivery_hint.as_str().into(),
                state.into(),
                obligation_state.as_str().into(),
                (if matches!(
                    obligation_state,
                    CollaborationObligationState::AwaitingReply
                ) {
                    1
                } else {
                    0
                })
                .into(),
                error.into(),
            ],
        ))
        .await?;
        if target.is_some() {
            if invoke {
                prompt_queue_service::enqueue_origin_in_transaction(
                    &txn,
                    target_id,
                    &delivery_id,
                    &event_id,
                    &delivery_id,
                    None,
                    crate::models::prompt_queue::PromptQueueSource::Collaboration,
                )
                .await?;
            }
            ensure_state(&txn, target_id).await?;
            bump_revision(&txn, target_id).await?;
            affected.insert(target_id);
        }
    }

    txn.execute(statement(
        "UPDATE collaboration_room SET updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        vec![input.room_id.clone().into()],
    ))
    .await?;

    let deliveries = deliveries_for_event(&txn, &event_id).await?;
    // Counted after the resolve above, so it is the debt the ledger Session
    // walks away with. A Session author never adds to its own debt — it is
    // removed from the target set — but a human post that `@`-ed the Room
    // creator does park one on the borrowed Session, and the count says so.
    let open_reply_debt = open_room_reply_debt(&txn, &input.room_id, source.id).await?;
    txn.commit().await?;
    Ok(RoomPostResult {
        event_id,
        room_id: input.room_id,
        deliveries,
        affected_conversation_ids: affected.into_iter().collect(),
        deduplicated: false,
        cleared_reply_to_event_id,
        open_reply_debt,
    })
}

async fn feed_on<C: ConnectionTrait>(
    conn: &C,
    conversation_id: i32,
    limit: u32,
) -> Result<CollaborationFeed, DbError> {
    require_live_session(conn, conversation_id).await?;
    let limit = limit.clamp(1, MAX_FEED_LIMIT) as i64;
    let inbound_rows = conn
        .query_all(statement(
            &format!(
                "{DELIVERY_SELECT} WHERE d.target_conversation_id = ? {DIRECT_MAIL_SQL} \
                 ORDER BY d.created_at DESC, d.id DESC LIMIT ?"
            ),
            vec![conversation_id.into(), limit.into()],
        ))
        .await?;
    let outbound_rows = conn
        .query_all(statement(
            &format!(
                "{DELIVERY_SELECT} WHERE e.source_conversation_id = ? {DIRECT_MAIL_SQL} \
                 ORDER BY d.created_at DESC, d.id DESC LIMIT ?"
            ),
            vec![conversation_id.into(), limit.into()],
        ))
        .await?;
    let count_row = conn
        .query_one(statement(
            "SELECT COUNT(*) AS count FROM collaboration_delivery d \
             JOIN collaboration_event e ON e.id = d.event_id \
             WHERE d.target_conversation_id = ? AND d.agent_received_at IS NULL \
               AND d.state <> 'dismissed' \
               AND COALESCE(e.visibility, 'direct') = 'direct'",
            vec![conversation_id.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let unread_count: i64 = count_row.try_get("", "count")?;
    Ok(CollaborationFeed {
        conversation_id,
        revision: revision(conn, conversation_id).await?,
        unread_count: unread_count.max(0) as u32,
        inbound: inbound_rows
            .iter()
            .map(parse_delivery)
            .collect::<Result<Vec<_>, _>>()?,
        outbound: outbound_rows
            .iter()
            .map(parse_delivery)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

pub async fn feed(
    conn: &DatabaseConnection,
    conversation_id: i32,
    limit: Option<u32>,
) -> Result<CollaborationFeed, DbError> {
    feed_on(conn, conversation_id, limit.unwrap_or(DEFAULT_FEED_LIMIT)).await
}

pub async fn list_inbox(
    conn: &DatabaseConnection,
    conversation_id: i32,
    scope: crate::acp::session_collaboration::SessionMailboxScope,
    filter: crate::acp::session_collaboration::SessionInboxFilter,
    peer_session_id: Option<i32>,
    limit: u32,
) -> Result<Vec<crate::models::CollaborationDeliveryView>, DbError> {
    require_live_session(conn, conversation_id).await?;
    let limit = limit.clamp(1, crate::acp::session_collaboration::MAX_INBOX_LIMIT) as i64;
    // The delivery row always carries recipient-side facts, so one filter
    // vocabulary serves both boxes: for Sent, "unread" is the recipient's.
    let filter_sql = match filter {
        crate::acp::session_collaboration::SessionInboxFilter::Open => {
            "AND (d.agent_received_at IS NULL OR d.obligation_state = 'awaiting_reply')"
        }
        crate::acp::session_collaboration::SessionInboxFilter::Unread => {
            "AND d.agent_received_at IS NULL"
        }
        crate::acp::session_collaboration::SessionInboxFilter::AwaitingReply => {
            "AND d.obligation_state = 'awaiting_reply'"
        }
        crate::acp::session_collaboration::SessionInboxFilter::All => "",
    };
    let (anchor_sql, peer_sql) = match scope {
        crate::acp::session_collaboration::SessionMailboxScope::Inbox => (
            "d.target_conversation_id = ?",
            "AND e.source_conversation_id = ?",
        ),
        crate::acp::session_collaboration::SessionMailboxScope::Sent => (
            "e.source_conversation_id = ?",
            "AND d.target_conversation_id = ?",
        ),
    };
    let mut params: Vec<sea_orm::Value> = vec![conversation_id.into()];
    let peer_clause = match peer_session_id {
        Some(peer) => {
            params.push(peer.into());
            peer_sql
        }
        None => "",
    };
    params.push(limit.into());
    let rows = conn
        .query_all(statement(
            &format!(
                "{DELIVERY_SELECT} WHERE {anchor_sql}                  AND d.state <> 'dismissed' {DIRECT_MAIL_SQL} {peer_clause} {filter_sql}                  ORDER BY d.created_at DESC, d.id DESC LIMIT ?"
            ),
            params,
        ))
        .await?;
    rows.iter().map(parse_delivery).collect()
}

pub async fn get_inbound_message(
    conn: &DatabaseConnection,
    conversation_id: i32,
    event_id: &str,
) -> Result<crate::models::CollaborationDeliveryView, DbError> {
    require_live_session(conn, conversation_id).await?;
    let row = conn
        .query_one(statement(
            &format!(
                "{DELIVERY_SELECT} WHERE d.target_conversation_id = ? \
                 AND d.event_id = ? AND d.state <> 'dismissed' LIMIT 1"
            ),
            vec![conversation_id.into(), event_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            DbError::NotFound(format!(
                "Inbox message {event_id} was not found for Session {conversation_id}"
            ))
        })?;
    parse_delivery(&row)
}

pub async fn mark_agent_read(
    conn: &DatabaseConnection,
    conversation_id: i32,
    event_id: &str,
) -> Result<CollaborationMutationResult, DbError> {
    require_live_session(conn, conversation_id).await?;
    let txn = conn.begin().await?;
    let changed = txn
        .execute(statement(
            "UPDATE collaboration_delivery \
             SET agent_received_at = COALESCE(agent_received_at, CURRENT_TIMESTAMP), \
                 agent_receipt_kind = COALESCE(agent_receipt_kind, 'managed_acp'), \
                 agent_receipt_ref = COALESCE(agent_receipt_ref, 'inbox_read'), \
                 updated_at = CURRENT_TIMESTAMP \
             WHERE target_conversation_id = ? AND event_id = ? \
               AND state <> 'dismissed'",
            vec![conversation_id.into(), event_id.into()],
        ))
        .await?
        .rows_affected()
        > 0;
    if !changed {
        txn.commit().await?;
        return Err(DbError::NotFound(format!(
            "Inbox message {event_id} was not found for Session {conversation_id}"
        )));
    }
    let mut participants = HashSet::from([conversation_id]);
    if let Ok((source_id, _)) =
        delivery_participants_by_event(&txn, conversation_id, event_id).await
    {
        participants.insert(source_id);
    }
    let affected = bump_live_participants(&txn, participants).await?;
    txn.commit().await?;
    Ok(CollaborationMutationResult {
        feed: feed(conn, conversation_id, None).await?,
        affected_conversation_ids: affected,
    })
}

async fn delivery_participants_by_event(
    txn: &DatabaseTransaction,
    target_conversation_id: i32,
    event_id: &str,
) -> Result<(i32, i32), DbError> {
    let row = txn
        .query_one(statement(
            "SELECT e.source_conversation_id, d.target_conversation_id \
             FROM collaboration_delivery d \
             JOIN collaboration_event e ON e.id = d.event_id \
             WHERE d.event_id = ? AND d.target_conversation_id = ?",
            vec![event_id.into(), target_conversation_id.into()],
        ))
        .await?
        .ok_or_else(|| DbError::NotFound(format!("Collaboration event {event_id}")))?;
    Ok((
        row.try_get("", "source_conversation_id")?,
        row.try_get("", "target_conversation_id")?,
    ))
}

/// Project this Session's inbound and outbound letters onto its timeline.
/// Pending inbound appears immediately; outbound is the same durable body
/// the sender already committed.
pub async fn timeline_projection(
    conn: &DatabaseConnection,
    conversation_id: i32,
) -> Result<CollaborationTimelineProjection, DbError> {
    require_live_session(conn, conversation_id).await?;
    let inbound_rows = conn
        .query_all(statement(
            &format!(
                "{DELIVERY_SELECT} WHERE d.target_conversation_id = ? \
                 AND d.state <> 'dismissed' {DIRECT_MAIL_SQL} \
                 ORDER BY d.created_at ASC, d.id ASC"
            ),
            vec![conversation_id.into()],
        ))
        .await?;
    let outbound_rows = conn
        .query_all(statement(
            &format!(
                "{DELIVERY_SELECT} WHERE e.source_conversation_id = ? \
                 AND d.state <> 'dismissed' {DIRECT_MAIL_SQL} \
                 ORDER BY d.created_at ASC, d.id ASC"
            ),
            vec![conversation_id.into()],
        ))
        .await?;
    Ok(CollaborationTimelineProjection {
        conversation_id,
        revision: revision(conn, conversation_id).await?,
        inbound: inbound_rows
            .iter()
            .map(parse_delivery)
            .collect::<Result<Vec<_>, _>>()?,
        outbound: outbound_rows
            .iter()
            .map(parse_delivery)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

/// Return actionable collaboration counts for every live Session. This is a
/// dedicated projection because mailbox lifecycle belongs to collaboration,
/// not the Harness-owned conversation index.
///
/// The per-Session rows and their totals stay direct-mail only. Room debt has
/// no Session mailbox to land in, so it is summed separately into the
/// `total_room_*` fields; a caller that wants "everything the user owes" adds
/// the two, and one that drills into `sessions` still gets a list it can open.
pub async fn unread_overview(
    conn: &DatabaseConnection,
) -> Result<CollaborationUnreadOverview, DbError> {
    let rows = conn
        .query_all(statement(
            "SELECT c.id AS conversation_id, COALESCE(s.revision, 0) AS revision, \
             (SELECT COUNT(*) FROM collaboration_delivery d \
                JOIN collaboration_event e ON e.id = d.event_id \
                WHERE d.target_conversation_id = c.id \
                  AND d.agent_received_at IS NULL AND d.state <> 'dismissed' \
                  AND COALESCE(e.visibility, 'direct') = 'direct') AS unread_count, \
             (SELECT COUNT(*) FROM collaboration_delivery d \
                JOIN collaboration_event e ON e.id = d.event_id \
                WHERE d.target_conversation_id = c.id \
                  AND d.obligation_state = 'awaiting_reply' \
                  AND COALESCE(e.visibility, 'direct') = 'direct') AS needs_reply_count, \
             (SELECT COUNT(*) FROM collaboration_delivery d \
                JOIN collaboration_event e ON e.id = d.event_id \
                WHERE e.source_conversation_id = c.id \
                  AND d.obligation_state = 'awaiting_reply' \
                  AND COALESCE(e.visibility, 'direct') = 'direct') AS awaiting_reply_count, \
             (SELECT COUNT(*) FROM collaboration_delivery d \
                JOIN collaboration_event e ON e.id = d.event_id \
                WHERE d.state = 'failed' \
                  AND COALESCE(e.visibility, 'direct') = 'direct' \
                  AND (d.target_conversation_id = c.id OR e.source_conversation_id = c.id)) AS failed_count \
             FROM conversation c \
             LEFT JOIN conversation_collaboration_state s ON s.conversation_id = c.id \
             WHERE c.deleted_at IS NULL \
             ORDER BY c.id",
            vec![],
        ))
        .await?;
    let mut total_unread_count = 0_u32;
    let mut total_needs_reply_count = 0_u32;
    let mut total_awaiting_reply_count = 0_u32;
    let mut sessions = Vec::with_capacity(rows.len());
    for row in rows {
        let count = |column| -> Result<u32, DbError> {
            let raw: i64 = row.try_get("", column)?;
            Ok(u32::try_from(raw.max(0)).unwrap_or(u32::MAX))
        };
        let unread_count = count("unread_count")?;
        let needs_reply_count = count("needs_reply_count")?;
        let awaiting_reply_count = count("awaiting_reply_count")?;
        let failed_count = count("failed_count")?;
        total_unread_count = total_unread_count.saturating_add(unread_count);
        total_needs_reply_count = total_needs_reply_count.saturating_add(needs_reply_count);
        total_awaiting_reply_count =
            total_awaiting_reply_count.saturating_add(awaiting_reply_count);
        if unread_count > 0 || needs_reply_count > 0 || awaiting_reply_count > 0 || failed_count > 0
        {
            sessions.push(CollaborationUnreadSession {
                conversation_id: row.try_get("", "conversation_id")?,
                revision: row.try_get("", "revision")?,
                unread_count,
                needs_reply_count,
                awaiting_reply_count,
                failed_count,
            });
        }
    }
    let total_failed_count = conn
        .query_one(statement(
            "SELECT COUNT(*) AS count FROM collaboration_delivery d \
             JOIN collaboration_event e ON e.id = d.event_id \
             WHERE d.state = 'failed' \
               AND COALESCE(e.visibility, 'direct') = 'direct' AND EXISTS ( \
                 SELECT 1 FROM conversation c \
                 WHERE c.deleted_at IS NULL \
                   AND (c.id = d.target_conversation_id OR c.id = e.source_conversation_id) \
             )",
            vec![],
        ))
        .await?
        .ok_or_else(|| validation("Could not count failed Session messages"))?
        .try_get::<i64>("", "count")?;
    let total_failed_count = u32::try_from(total_failed_count.max(0)).unwrap_or(u32::MAX);
    let room_totals = collaboration_room_service::host_totals(conn).await?;
    Ok(CollaborationUnreadOverview {
        total_unread_count,
        total_needs_reply_count,
        total_awaiting_reply_count,
        total_failed_count,
        total_room_unread_count: room_totals.unread_count,
        total_room_needs_reply_count: room_totals.needs_reply_count,
        sessions,
    })
}

#[derive(Debug, Clone)]
pub struct ReminderTargetSnapshot {
    pub conversation_id: i32,
    pub overdue_unread: u32,
    pub overdue_reply: u32,
    pub reminder_repeat_count: u32,
    pub reminder_last_at: Option<DateTime<Utc>>,
    pub letters: Vec<crate::acp::collaboration_reminder::ReminderLetterLine>,
}

/// Sessions whose Agent mailbox is overdue for a host reminder. Archived
/// Sessions are skipped: their queue never runs, so a queued reminder would
/// pile up undeliverable. Their debts stay frozen until they are restored.
pub async fn list_overdue_reminder_targets(
    conn: &DatabaseConnection,
) -> Result<Vec<ReminderTargetSnapshot>, DbError> {
    use crate::acp::collaboration_reminder::{
        reminder_in_cooldown, MAX_REMINDER_REPEATS, REPLY_AFTER_SECS, UNREAD_AFTER_SECS,
    };

    // overdue_unread must exclude `failed`, matching overdue_reply and
    // reset_idle_reminder_cursors: a failed delivery never reached the Agent,
    // so redelivery is the queue's job, not the reminder sweep's.
    let rows = conn
        .query_all(statement(
            &format!(
                "SELECT d.target_conversation_id AS conversation_id, \
                 SUM(CASE WHEN d.invocation_policy = 'invoke_when_idle' \
                      AND d.agent_received_at IS NULL \
                      AND d.state <> 'dismissed' AND d.state <> 'failed' \
                      AND datetime(d.created_at) <= datetime('now', '-{UNREAD_AFTER_SECS} seconds') \
                      THEN 1 ELSE 0 END) AS overdue_unread, \
                 SUM(CASE WHEN d.obligation_state = 'awaiting_reply' \
                      AND d.state <> 'dismissed' AND d.state <> 'failed' \
                      AND d.agent_received_at IS NOT NULL \
                      AND datetime(d.agent_received_at) <= datetime('now', '-{REPLY_AFTER_SECS} seconds') \
                      THEN 1 ELSE 0 END) AS overdue_reply, \
                 MAX(CASE \
                      WHEN d.invocation_policy = 'invoke_when_idle' \
                       AND d.agent_received_at IS NULL AND d.state <> 'dismissed' \
                       AND datetime(d.created_at) <= datetime('now', '-{UNREAD_AFTER_SECS} seconds') \
                      THEN datetime(d.created_at, '+{UNREAD_AFTER_SECS} seconds') \
                      WHEN d.obligation_state = 'awaiting_reply' \
                       AND d.state <> 'dismissed' AND d.state <> 'failed' \
                       AND d.agent_received_at IS NOT NULL \
                       AND datetime(d.agent_received_at) <= datetime('now', '-{REPLY_AFTER_SECS} seconds') \
                      THEN datetime(d.agent_received_at, '+{REPLY_AFTER_SECS} seconds') \
                      END) AS newest_due_at, \
                 COALESCE(s.reminder_repeat_count, 0) AS reminder_repeat_count, \
                 s.reminder_last_at AS reminder_last_at \
                 FROM collaboration_delivery d \
                 JOIN collaboration_event e ON e.id = d.event_id \
                 JOIN conversation c ON c.id = d.target_conversation_id \
                  AND c.deleted_at IS NULL AND c.archived_at IS NULL \
                 LEFT JOIN conversation_collaboration_state s \
                   ON s.conversation_id = d.target_conversation_id \
                 GROUP BY d.target_conversation_id \
                 HAVING overdue_unread > 0 OR overdue_reply > 0"
            ),
            vec![],
        ))
        .await?;

    let now = Utc::now();
    let mut targets = Vec::new();
    for row in rows {
        let count = |column| -> Result<u32, DbError> {
            let raw: i64 = row.try_get("", column)?;
            Ok(u32::try_from(raw.max(0)).unwrap_or(u32::MAX))
        };
        let stored_repeat_count = count("reminder_repeat_count")?;
        let reminder_last_at = parse_optional_timestamp(&row, "reminder_last_at")?;
        let newest_due_at = parse_optional_timestamp(&row, "newest_due_at")?;
        // The repeat cap guards one debt episode, not the Session forever. A
        // member that became due after the last reminder (new mail, or read
        // mail whose reply clock just elapsed) starts a fresh episode with a
        // fresh budget; otherwise unread-phase nags would permanently starve
        // the read-awaiting-reply phase.
        let fresh_debt = match (newest_due_at, reminder_last_at) {
            (Some(due), Some(last)) => due > last,
            (Some(_), None) => true,
            (None, _) => false,
        };
        let reminder_repeat_count = if fresh_debt { 0 } else { stored_repeat_count };
        if reminder_repeat_count >= MAX_REMINDER_REPEATS {
            continue;
        }
        if reminder_last_at.is_some_and(|at| reminder_in_cooldown(at, now)) {
            continue;
        }
        let conversation_id: i32 = row.try_get("", "conversation_id")?;
        targets.push(ReminderTargetSnapshot {
            conversation_id,
            overdue_unread: count("overdue_unread")?,
            overdue_reply: count("overdue_reply")?,
            reminder_repeat_count,
            reminder_last_at,
            letters: Vec::new(),
        });
    }
    for target in &mut targets {
        target.letters = overdue_reminder_letters(conn, target.conversation_id).await?;
    }
    Ok(targets)
}

async fn overdue_reminder_letters(
    conn: &DatabaseConnection,
    conversation_id: i32,
) -> Result<Vec<crate::acp::collaboration_reminder::ReminderLetterLine>, DbError> {
    use crate::acp::collaboration_reminder::{REPLY_AFTER_SECS, UNREAD_AFTER_SECS};
    let rows = conn
        .query_all(statement(
            &format!(
                "SELECT e.id AS event_id, e.subject, e.body, \
                        e.source_conversation_id, e.source_title_snapshot, \
                        COALESCE(e.visibility, 'direct') AS visibility, \
                        e.room_id, \
                        CASE WHEN d.obligation_state = 'awaiting_reply' \
                             AND d.agent_received_at IS NOT NULL \
                             AND d.agent_received_at <= datetime('now', '-{REPLY_AFTER_SECS} seconds') \
                             THEN 1 ELSE 0 END AS awaiting_reply \
                 FROM collaboration_delivery d \
                 JOIN collaboration_event e ON e.id = d.event_id \
                 WHERE d.target_conversation_id = ? \
                   AND d.state <> 'dismissed' AND d.state <> 'failed' \
                   AND ( \
                        (d.invocation_policy = 'invoke_when_idle' \
                         AND d.agent_received_at IS NULL \
                         AND datetime(d.created_at) <= datetime('now', '-{UNREAD_AFTER_SECS} seconds')) \
                     OR (d.obligation_state = 'awaiting_reply' \
                         AND d.agent_received_at IS NOT NULL \
                         AND datetime(d.agent_received_at) <= datetime('now', '-{REPLY_AFTER_SECS} seconds')) \
                   ) \
                 ORDER BY d.created_at ASC LIMIT 8"
            ),
            vec![conversation_id.into()],
        ))
        .await?;
    let mut letters = Vec::new();
    for row in rows {
        let subject: String = row
            .try_get::<Option<String>>("", "subject")?
            .unwrap_or_default();
        let body: String = row
            .try_get::<Option<String>>("", "body")?
            .unwrap_or_default();
        let from_title: Option<String> = row.try_get("", "source_title_snapshot")?;
        let from_session_id: i32 = row.try_get("", "source_conversation_id")?;
        let awaiting: i64 = row.try_get("", "awaiting_reply")?;
        let visibility: String = row
            .try_get::<Option<String>>("", "visibility")?
            .unwrap_or_else(|| "direct".to_string());
        let is_room = visibility == "room";
        letters.push(crate::acp::collaboration_reminder::ReminderLetterLine {
            event_id: row.try_get("", "event_id")?,
            from_session_id,
            from_title: from_title
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| format!("Session {from_session_id}")),
            letter_title: if is_room {
                crate::acp::session_collaboration::inbox_preview(&body)
            } else {
                crate::acp::session_collaboration::letter_title(&subject, &body)
            },
            awaiting_reply: awaiting != 0,
            is_room,
            room_id: row.try_get("", "room_id")?,
        });
    }
    Ok(letters)
}

/// Persist that one reminder went out. `completed_repeat_count` is the
/// effective count the sweep observed on its snapshot (0 when a fresh debt
/// episode just reset the budget), so the stored counter follows the episode
/// instead of accumulating across the Session's whole life.
pub async fn record_successful_reminder(
    conn: &DatabaseConnection,
    conversation_id: i32,
    completed_repeat_count: u32,
) -> Result<(), DbError> {
    let txn = conn.begin().await?;
    ensure_state(&txn, conversation_id).await?;
    txn.execute(statement(
        "UPDATE conversation_collaboration_state \
         SET reminder_last_at = CURRENT_TIMESTAMP, \
             reminder_repeat_count = ?, \
             updated_at = CURRENT_TIMESTAMP \
         WHERE conversation_id = ?",
        vec![
            i64::from(completed_repeat_count.saturating_add(1)).into(),
            conversation_id.into(),
        ],
    ))
    .await?;
    txn.commit().await?;
    Ok(())
}

pub async fn reset_idle_reminder_cursors(conn: &DatabaseConnection) -> Result<(), DbError> {
    conn.execute(statement(
        "UPDATE conversation_collaboration_state \
         SET reminder_last_at = NULL, reminder_repeat_count = 0, \
             updated_at = CURRENT_TIMESTAMP \
         WHERE (reminder_last_at IS NOT NULL OR reminder_repeat_count > 0) \
           AND conversation_id NOT IN ( \
             SELECT DISTINCT target_conversation_id FROM collaboration_delivery \
             WHERE state <> 'dismissed' AND state <> 'failed' \
               AND ( \
                    (invocation_policy = 'invoke_when_idle' AND agent_received_at IS NULL) \
                 OR (obligation_state = 'awaiting_reply' AND agent_received_at IS NOT NULL) \
               ) \
           )",
        vec![],
    ))
    .await?;
    Ok(())
}

/// One open reply obligation, titles only. The tail note on ordinary turns
/// (compact-safe debt recovery) lists these; the idle-continuation timer only
/// needs the counts.
#[derive(Debug, Clone)]
pub(crate) struct OpenObligationLine {
    pub event_id: String,
    pub from_session_id: i32,
    pub from_title: String,
    pub letter_title: String,
    pub is_room: bool,
    pub room_id: Option<String>,
    pub agent_received: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct OpenObligationDigest {
    /// Direct letters this Session owes a reply.
    pub letters_owed: u32,
    /// Room `@` mentions this Session owes a reply.
    pub room_mentions_owed: u32,
    /// Oldest first, capped; titles derived, bodies never leave the ledger.
    pub lines: Vec<OpenObligationLine>,
}

const MAX_OBLIGATION_NOTE_LINES: usize = 8;

/// Snapshot every open reply obligation a Session carries: direct letters it
/// has not answered plus Room `@` mentions still awaiting its reply.
pub(crate) async fn open_obligation_digest(
    conn: &DatabaseConnection,
    conversation_id: i32,
) -> Result<OpenObligationDigest, DbError> {
    let counts = conn
        .query_one(statement(
            "SELECT COALESCE(SUM(CASE WHEN COALESCE(e.visibility, 'direct') = 'direct' \
                     THEN 1 ELSE 0 END), 0) AS letters, \
                    COALESCE(SUM(CASE WHEN COALESCE(e.visibility, 'direct') = 'room' \
                     THEN 1 ELSE 0 END), 0) AS room_mentions \
             FROM collaboration_delivery d \
             JOIN collaboration_event e ON e.id = d.event_id \
             WHERE d.target_conversation_id = ? \
               AND d.obligation_state = 'awaiting_reply' \
               AND d.state = 'embedded'",
            vec![conversation_id.into()],
        ))
        .await?
        .ok_or_else(|| validation("Could not count open reply obligations"))?;
    let to_u32 = |raw: i64| u32::try_from(raw.max(0)).unwrap_or(u32::MAX);
    let letters_owed = to_u32(counts.try_get("", "letters")?);
    let room_mentions_owed = to_u32(counts.try_get("", "room_mentions")?);
    let mut digest = OpenObligationDigest {
        letters_owed,
        room_mentions_owed,
        lines: Vec::new(),
    };
    if letters_owed == 0 && room_mentions_owed == 0 {
        return Ok(digest);
    }
    let rows = conn
        .query_all(statement(
            &format!(
                "SELECT e.id AS event_id, e.subject, e.body, \
                        e.source_conversation_id, e.source_title_snapshot, \
                        COALESCE(e.visibility, 'direct') AS visibility, e.room_id, \
                        d.agent_received_at IS NOT NULL AS agent_received \
                 FROM collaboration_delivery d \
                 JOIN collaboration_event e ON e.id = d.event_id \
                 WHERE d.target_conversation_id = ? \
                   AND d.obligation_state = 'awaiting_reply' \
                   AND d.state = 'embedded' \
                 ORDER BY d.created_at ASC, d.rowid ASC \
                 LIMIT {MAX_OBLIGATION_NOTE_LINES}"
            ),
            vec![conversation_id.into()],
        ))
        .await?;
    for row in rows {
        let subject: String = row
            .try_get::<Option<String>>("", "subject")?
            .unwrap_or_default();
        let body: String = row
            .try_get::<Option<String>>("", "body")?
            .unwrap_or_default();
        let from_session_id: i32 = row.try_get("", "source_conversation_id")?;
        let from_title: Option<String> = row.try_get("", "source_title_snapshot")?;
        let visibility: String = row
            .try_get::<Option<String>>("", "visibility")?
            .unwrap_or_else(|| "direct".to_string());
        let is_room = visibility == "room";
        digest.lines.push(OpenObligationLine {
            event_id: row.try_get("", "event_id")?,
            from_session_id,
            from_title: from_title
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| format!("Session {from_session_id}")),
            letter_title: if is_room {
                crate::acp::session_collaboration::inbox_preview(&body)
            } else {
                crate::acp::session_collaboration::letter_title(&subject, &body)
            },
            is_room,
            room_id: row.try_get("", "room_id")?,
            agent_received: row.try_get::<i64>("", "agent_received")? != 0,
        });
    }
    Ok(digest)
}

/// Host-authored tail note for an ordinary turn: the titles of this Session's
/// open reply obligations. A context compact drops every delivered envelope
/// from the Agent's working memory; re-stating the debts on the next ordinary
/// turn is the compact-safe recovery — no extra wake, no extra turn. Titles
/// only, never bodies. Nothing is claimed or transitioned: the note is
/// recomputed from the ledger each turn, so a lost or crashed dispatch simply
/// reappears next time.
///
/// Only debts whose delivery is `embedded` qualify: a letter still pending,
/// queued, or riding THIS very turn is the envelope's job — restating it in
/// the same prompt would say "you owe a reply" in the same breath as handing
/// over the letter.
pub(crate) async fn open_obligation_note_for_turn(
    conn: &DatabaseConnection,
    conversation_id: i32,
) -> Result<Option<PromptInputBlock>, DbError> {
    let digest = open_obligation_digest(conn, conversation_id).await?;
    let total = digest
        .letters_owed
        .saturating_add(digest.room_mentions_owed);
    if total == 0 {
        return Ok(None);
    }
    let mut entries = Vec::with_capacity(digest.lines.len());
    for line in &digest.lines {
        let flag = if line.agent_received {
            "已读未回"
        } else {
            "未读"
        };
        if line.is_room {
            entries.push(format!(
                "群点名《{}》来自 {} #{}（{}，room_id={}，event_id={}）",
                line.letter_title,
                line.from_title,
                line.from_session_id,
                flag,
                line.room_id.as_deref().unwrap_or("room"),
                line.event_id
            ));
        } else {
            entries.push(format!(
                "《{}》来自 {} #{}（{}，event_id={}）",
                line.letter_title, line.from_title, line.from_session_id, flag, line.event_id
            ));
        }
    }
    let listed = u32::try_from(digest.lines.len()).unwrap_or(u32::MAX);
    let more = if total > listed {
        format!("，另还有 {} 笔未列出", total - listed)
    } else {
        String::new()
    };
    let text = format!(
        "Codeg 宿主附注（系统事实，不是新来信）：你有 {total} 笔未结回复义务{more}：{}。\
只列标题不含正文；用 list_inbox / read_message 处理信件，用 read_room / post_room 处理群点名，回复后义务自动清账。",
        entries.join("；")
    );
    Ok(Some(PromptInputBlock::Text { text }))
}

pub struct CollaborationMutationResult {
    pub feed: CollaborationFeed,
    pub affected_conversation_ids: Vec<i32>,
}

async fn delivery_participants(
    txn: &DatabaseTransaction,
    delivery_id: &str,
    target_conversation_id: i32,
) -> Result<(i32, i32), DbError> {
    let row = txn
        .query_one(statement(
            "SELECT e.source_conversation_id, d.target_conversation_id \
             FROM collaboration_delivery d \
             JOIN collaboration_event e ON e.id = d.event_id \
             WHERE d.id = ?",
            vec![delivery_id.into()],
        ))
        .await?
        .ok_or_else(|| DbError::NotFound(format!("Collaboration delivery {delivery_id}")))?;
    let source_id: i32 = row.try_get("", "source_conversation_id")?;
    let target_id: i32 = row.try_get("", "target_conversation_id")?;
    if target_id != target_conversation_id {
        return Err(DbError::NotFound(format!(
            "Collaboration delivery {delivery_id} for Session {target_conversation_id}"
        )));
    }
    Ok((source_id, target_id))
}

async fn bump_live_participants(
    txn: &DatabaseTransaction,
    participants: impl IntoIterator<Item = i32>,
) -> Result<Vec<i32>, DbError> {
    let mut affected = BTreeSet::new();
    for conversation_id in participants {
        if ensure_state_if_live(txn, conversation_id).await? {
            bump_revision(txn, conversation_id).await?;
            affected.insert(conversation_id);
        }
    }
    Ok(affected.into_iter().collect())
}

pub async fn mark_seen(
    conn: &DatabaseConnection,
    conversation_id: i32,
    delivery_ids: Vec<String>,
) -> Result<CollaborationMutationResult, DbError> {
    require_live_session(conn, conversation_id).await?;
    let ids: BTreeSet<String> = delivery_ids
        .into_iter()
        .filter(|id| !id.trim().is_empty())
        .collect();
    if ids.is_empty() {
        return Ok(CollaborationMutationResult {
            feed: feed(conn, conversation_id, None).await?,
            affected_conversation_ids: vec![],
        });
    }

    let txn = conn.begin().await?;
    let mut participants = HashSet::from([conversation_id]);
    let mut changed = false;
    for delivery_id in ids {
        let (source_id, _) = delivery_participants(&txn, &delivery_id, conversation_id).await?;
        participants.insert(source_id);
        let result = txn
            .execute(statement(
                "UPDATE collaboration_delivery \
                 SET attention_state = 'opened', \
                     opened_at = COALESCE(opened_at, CURRENT_TIMESTAMP), \
                     ui_seen_at = COALESCE(ui_seen_at, CURRENT_TIMESTAMP), \
                     updated_at = CURRENT_TIMESTAMP \
                 WHERE id = ? AND target_conversation_id = ? \
                   AND attention_state = 'unread'",
                vec![delivery_id.into(), conversation_id.into()],
            ))
            .await?;
        changed |= result.rows_affected() > 0;
    }
    let affected = if changed {
        bump_live_participants(&txn, participants).await?
    } else {
        vec![]
    };
    txn.commit().await?;
    Ok(CollaborationMutationResult {
        feed: feed(conn, conversation_id, None).await?,
        affected_conversation_ids: affected,
    })
}

/// Archiving means the Session no longer expects replies: every open
/// obligation owed TO it resolves so the debtors' reminders stop instead of
/// demanding a letter the archive would refuse. The waiver is final —
/// unarchiving never revives these debts; ask again with a new letter.
/// Obligations the archived Session itself owes stay frozen (its queue does
/// not run and the reminder sweep skips archived debtors). Returns the
/// debtor Session ids whose ledgers changed.
pub async fn resolve_obligations_owed_to(
    conn: &DatabaseConnection,
    source_conversation_id: i32,
) -> Result<Vec<i32>, DbError> {
    let txn = conn.begin().await?;
    let rows = txn
        .query_all(statement(
            "SELECT DISTINCT d.target_conversation_id AS debtor \
             FROM collaboration_delivery d \
             JOIN collaboration_event e ON e.id = d.event_id \
             WHERE e.source_conversation_id = ? AND d.obligation_state = 'awaiting_reply'",
            vec![source_conversation_id.into()],
        ))
        .await?;
    let mut debtors = Vec::with_capacity(rows.len());
    for row in &rows {
        debtors.push(row.try_get("", "debtor")?);
    }
    if debtors.is_empty() {
        txn.commit().await?;
        return Ok(debtors);
    }
    txn.execute(statement(
        "UPDATE collaboration_delivery \
         SET obligation_state = 'resolved', \
             obligation_resolved_at = COALESCE(obligation_resolved_at, CURRENT_TIMESTAMP), \
             updated_at = CURRENT_TIMESTAMP \
         WHERE obligation_state = 'awaiting_reply' \
           AND event_id IN ( \
               SELECT id FROM collaboration_event WHERE source_conversation_id = ? \
           )",
        vec![source_conversation_id.into()],
    ))
    .await?;
    let affected = bump_live_participants(&txn, debtors).await?;
    txn.commit().await?;
    Ok(affected)
}

/// Resolve an inbound reply obligation without sending a reply. The update is
/// scoped to one target delivery, so a fan-out recipient cannot clear sibling
/// recipients' work.
pub async fn resolve_obligation(
    conn: &DatabaseConnection,
    conversation_id: i32,
    delivery_id: &str,
) -> Result<CollaborationMutationResult, DbError> {
    require_live_session(conn, conversation_id).await?;
    let txn = conn.begin().await?;
    let (source_id, _) = delivery_participants(&txn, delivery_id, conversation_id).await?;
    let result = txn
        .execute(statement(
            "UPDATE collaboration_delivery \
             SET obligation_state = 'resolved', \
                 obligation_resolved_at = COALESCE(obligation_resolved_at, CURRENT_TIMESTAMP), \
                 updated_at = CURRENT_TIMESTAMP \
             WHERE id = ? AND target_conversation_id = ? \
               AND obligation_state = 'awaiting_reply'",
            vec![delivery_id.into(), conversation_id.into()],
        ))
        .await?;
    let affected = if result.rows_affected() > 0 {
        bump_live_participants(&txn, [source_id, conversation_id]).await?
    } else {
        vec![]
    };
    txn.commit().await?;
    Ok(CollaborationMutationResult {
        feed: feed(conn, conversation_id, None).await?,
        affected_conversation_ids: affected,
    })
}

pub async fn dismiss(
    conn: &DatabaseConnection,
    conversation_id: i32,
    delivery_id: &str,
) -> Result<CollaborationMutationResult, DbError> {
    require_live_session(conn, conversation_id).await?;
    let txn = conn.begin().await?;
    let (source_id, _) = delivery_participants(&txn, delivery_id, conversation_id).await?;
    let result = txn
        .execute(statement(
            "UPDATE collaboration_delivery \
             SET state = 'dismissed', attention_state = 'opened', \
                 opened_at = COALESCE(opened_at, CURRENT_TIMESTAMP), \
                 ui_seen_at = COALESCE(ui_seen_at, CURRENT_TIMESTAMP), \
                 updated_at = CURRENT_TIMESTAMP \
             WHERE id = ? AND target_conversation_id = ? AND state = 'pending'",
            vec![delivery_id.into(), conversation_id.into()],
        ))
        .await?;
    let affected = if result.rows_affected() > 0 {
        bump_live_participants(&txn, [source_id, conversation_id]).await?
    } else {
        vec![]
    };
    txn.commit().await?;
    Ok(CollaborationMutationResult {
        feed: feed(conn, conversation_id, None).await?,
        affected_conversation_ids: affected,
    })
}

/// Put a previously dismissed `store_only` delivery back into the target
/// Session's pending context. Read state stays untouched: restoring whether a
/// message should reach the Agent is independent from pretending the human has
/// not already seen it.
pub async fn restore(
    conn: &DatabaseConnection,
    conversation_id: i32,
    delivery_id: &str,
) -> Result<CollaborationMutationResult, DbError> {
    require_live_session(conn, conversation_id).await?;
    let txn = conn.begin().await?;
    let (source_id, _) = delivery_participants(&txn, delivery_id, conversation_id).await?;
    let result = txn
        .execute(statement(
            "UPDATE collaboration_delivery \
             SET state = 'pending', embedded_turn_ref = NULL, error = NULL, \
                 updated_at = CURRENT_TIMESTAMP \
             WHERE id = ? AND target_conversation_id = ? \
               AND invocation_policy = 'store_only' AND state = 'dismissed'",
            vec![delivery_id.into(), conversation_id.into()],
        ))
        .await?;
    let affected = if result.rows_affected() > 0 {
        bump_live_participants(&txn, [source_id, conversation_id]).await?
    } else {
        vec![]
    };
    txn.commit().await?;
    Ok(CollaborationMutationResult {
        feed: feed(conn, conversation_id, None).await?,
        affected_conversation_ids: affected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::{
        fresh_disk_db, fresh_in_memory_db, seed_conversation, seed_folder,
    };
    use crate::models::AgentType;
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    fn input(
        source: i32,
        targets: Vec<i32>,
        dedupe: &str,
        body: &str,
    ) -> SendCollaborationMessageInput {
        SendCollaborationMessageInput::letter(source, targets, dedupe, "Test letter", body)
    }

    fn invoke_input(
        source: i32,
        targets: Vec<i32>,
        dedupe: &str,
        body: &str,
    ) -> SendCollaborationMessageInput {
        let mut input = input(source, targets, dedupe, body);
        input.invocation_policy = CollaborationInvocationPolicy::InvokeWhenIdle;
        input
    }

    async fn seeded_memory() -> (crate::db::AppDatabase, i32, i32, i32) {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/codeg-collaboration").await;
        let source = seed_conversation(&db, folder_id, AgentType::Codex).await;
        let target_a = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let target_b = seed_conversation(&db, folder_id, AgentType::Gemini).await;
        (db, source, target_a, target_b)
    }

    #[tokio::test]
    async fn one_event_fans_out_atomically_and_never_enters_prompt_queue() {
        let (db, source, target_a, target_b) = seeded_memory().await;
        let sent = send(
            &db.conn,
            input(source, vec![target_a, target_b], "fanout-1", "review this"),
        )
        .await
        .expect("send");
        assert_eq!(sent.deliveries.len(), 2);
        assert!(sent
            .deliveries
            .iter()
            .all(|item| item.state == CollaborationDeliveryState::Pending));
        assert_eq!(
            feed(&db.conn, target_a, None).await.unwrap().unread_count,
            1
        );
        assert_eq!(
            feed(&db.conn, target_b, None).await.unwrap().unread_count,
            1
        );
        let queued: i64 = db
            .conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS count FROM conversation_prompt_queue_item",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "count")
            .unwrap();
        assert_eq!(queued, 0, "store_only must not wake or enqueue the Harness");
    }

    #[tokio::test]
    async fn timeline_projection_requires_exact_target_embedding_and_agent_receipt() {
        let (db, source, target_a, target_b) = seeded_memory().await;
        let fanout = send(
            &db.conn,
            input(
                source,
                vec![target_a, target_b],
                "timeline-fanout",
                "project me once",
            ),
        )
        .await
        .unwrap();
        let pending = send(
            &db.conn,
            input(source, vec![target_a], "timeline-pending", "still pending"),
        )
        .await
        .unwrap();
        let partial = send(
            &db.conn,
            input(
                source,
                vec![target_a],
                "timeline-partial",
                "embedded without receipt",
            ),
        )
        .await
        .unwrap();

        let target_a_delivery = fanout
            .deliveries
            .iter()
            .find(|delivery| delivery.target.conversation_id == target_a)
            .unwrap();
        db.conn
            .execute(statement(
                "UPDATE collaboration_delivery \
                 SET state = 'embedded', embedded_turn_ref = 'turn-a', \
                     agent_received_at = CURRENT_TIMESTAMP, \
                     agent_receipt_kind = 'managed_acp', agent_receipt_ref = 'turn-a' \
                 WHERE id = ?",
                vec![target_a_delivery.id.clone().into()],
            ))
            .await
            .unwrap();
        db.conn
            .execute(statement(
                "UPDATE collaboration_delivery \
                 SET state = 'embedded', embedded_turn_ref = 'turn-partial' \
                 WHERE event_id = ? AND target_conversation_id = ?",
                vec![partial.event_id.into(), target_a.into()],
            ))
            .await
            .unwrap();

        let projected = timeline_projection(&db.conn, target_a).await.unwrap();
        assert_eq!(projected.conversation_id, target_a);
        let embedded = projected
            .inbound
            .iter()
            .find(|delivery| delivery.id == target_a_delivery.id)
            .expect("embedded delivery");
        assert_eq!(embedded.embedded_turn_ref.as_deref(), Some("turn-a"));
        assert!(embedded.agent_received_at.is_some());
        assert!(projected
            .inbound
            .iter()
            .any(|delivery| delivery.event_id == pending.event_id));

        let sibling = timeline_projection(&db.conn, target_b).await.unwrap();
        assert_eq!(sibling.inbound.len(), 1);

        let source_view = timeline_projection(&db.conn, source).await.unwrap();
        assert!(source_view.inbound.is_empty());
        assert!(
            source_view
                .outbound
                .iter()
                .any(|delivery| delivery.body == "project me once"),
            "the sender timeline must render the outbound letter body"
        );
    }

    #[tokio::test]
    async fn store_only_mail_is_claimed_once_and_embedded_in_the_next_natural_turn() {
        let (db, source, target, _) = seeded_memory().await;
        let first = send(
            &db.conn,
            input(source, vec![target], "natural-first", "first review note"),
        )
        .await
        .unwrap();
        let second = send(
            &db.conn,
            input(source, vec![target], "natural-second", "second review note"),
        )
        .await
        .unwrap();

        let claimed = claim_pending_store_only_for_turn(&db.conn, target, "optimistic-natural")
            .await
            .unwrap()
            .expect("pending mail should attach to the next ordinary turn");
        assert_eq!(
            claimed.event_ids,
            vec![first.event_id.clone(), second.event_id.clone()]
        );
        let attached = claimed
            .blocks
            .iter()
            .filter_map(|block| match block {
                PromptInputBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(attached[0].contains(&first.event_id));
        assert!(attached[1].contains(&second.event_id));
        assert!(attached
            .iter()
            .all(|text| text.contains("Call read_message")));
        assert!(attached
            .iter()
            .all(|text| text.contains("Codeg mailbox letter")));
        assert!(attached.iter().all(|text| text.contains("review note")));
        assert!(
            claim_pending_store_only_for_turn(&db.conn, target, "another-turn")
                .await
                .unwrap()
                .is_none()
        );

        mark_store_only_batch_embedded(&db.conn, target, &claimed)
            .await
            .unwrap();
        let feed = feed(&db.conn, target, None).await.unwrap();
        assert!(feed.inbound.iter().all(|delivery| {
            delivery.state == CollaborationDeliveryState::Embedded
                && delivery.embedded_turn_ref.as_deref() == Some("optimistic-natural")
                && delivery.agent_receipt_kind.is_none()
                && delivery.agent_receipt_ref.is_none()
                && delivery.agent_received_at.is_none()
                && delivery.attempts == 1
        }));
    }

    #[tokio::test]
    async fn known_prompt_failure_releases_store_only_mail_for_a_retry() {
        let (db, source, target, _) = seeded_memory().await;
        let sent = send(
            &db.conn,
            input(source, vec![target], "natural-retry", "do not lose this"),
        )
        .await
        .unwrap();
        let first = claim_pending_store_only_for_turn(&db.conn, target, "failed-turn")
            .await
            .unwrap()
            .unwrap();
        release_store_only_batch(&db.conn, target, &first)
            .await
            .unwrap();

        let retry = claim_pending_store_only_for_turn(&db.conn, target, "retry-turn")
            .await
            .unwrap()
            .expect("known pre-dispatch failure is safe to retry");
        assert_eq!(retry.event_ids, vec![sent.event_id]);
        let delivery = feed(&db.conn, target, None)
            .await
            .unwrap()
            .inbound
            .remove(0);
        assert_eq!(delivery.state, CollaborationDeliveryState::Embedding);
        assert_eq!(delivery.embedded_turn_ref.as_deref(), Some("retry-turn"));
        assert_eq!(delivery.attempts, 2);
    }

    #[tokio::test]
    async fn invoked_collaboration_turn_does_not_mix_pending_store_only_mail() {
        let (db, source, target, _) = seeded_memory().await;
        let invoked = send(
            &db.conn,
            invoke_input(source, vec![target], "invoke-alone", "answer this alone"),
        )
        .await
        .unwrap();
        let pending = send(
            &db.conn,
            input(source, vec![target], "pending-later", "save this for later"),
        )
        .await
        .unwrap();
        let queue = prompt_queue_service::snapshot(&db.conn, target)
            .await
            .unwrap();
        let invoked_item_id = queue
            .items
            .iter()
            .find(|item| item.origin_event_id.as_deref() == Some(invoked.event_id.as_str()))
            .unwrap()
            .id
            .clone();

        assert!(
            claim_pending_store_only_for_turn(&db.conn, target, &invoked_item_id)
                .await
                .unwrap()
                .is_none()
        );
        let pending_delivery = feed(&db.conn, target, None)
            .await
            .unwrap()
            .inbound
            .into_iter()
            .find(|delivery| delivery.event_id == pending.event_id)
            .unwrap();
        assert_eq!(pending_delivery.state, CollaborationDeliveryState::Pending);
        assert_eq!(pending_delivery.attempts, 0);
    }

    #[tokio::test]
    async fn natural_turn_batch_is_bounded_and_leaves_overflow_pending() {
        let (db, source, target, _) = seeded_memory().await;
        for index in 0..=MAX_STORE_ONLY_DELIVERIES_PER_TURN {
            send(
                &db.conn,
                input(
                    source,
                    vec![target],
                    &format!("bounded-{index}"),
                    &format!("message {index}"),
                ),
            )
            .await
            .unwrap();
        }

        let batch = claim_pending_store_only_for_turn(&db.conn, target, "bounded-turn")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(batch.event_ids.len(), MAX_STORE_ONLY_DELIVERIES_PER_TURN);
        let feed = feed(&db.conn, target, None).await.unwrap();
        assert_eq!(
            feed.inbound
                .iter()
                .filter(|delivery| delivery.state == CollaborationDeliveryState::Pending)
                .count(),
            1
        );
        assert_eq!(
            feed.inbound
                .iter()
                .filter(|delivery| delivery.state == CollaborationDeliveryState::Embedding)
                .count(),
            MAX_STORE_ONLY_DELIVERIES_PER_TURN
        );
    }

    #[tokio::test]
    async fn store_only_event_resolves_to_the_same_envelope_as_invoke() {
        let (db, source, target, _) = seeded_memory().await;
        let sent = send(
            &db.conn,
            input(
                source,
                vec![target],
                "store-draft",
                "wake the other Session",
            ),
        )
        .await
        .unwrap();
        let draft = prompt_draft_for_origin(&db.conn, target, &sent.event_id)
            .await
            .expect("store_only deliveries must be injectable");
        let PromptInputBlock::Text { text } = &draft.blocks[0] else {
            panic!("store_only draft must be one text envelope");
        };
        assert!(text.contains("wake the other Session"));
        assert!(text.contains(&sent.event_id));
        assert!(text.contains("Call read_message"));
    }

    #[tokio::test]
    async fn invoke_when_idle_atomically_references_each_delivery_from_the_target_queue() {
        let (db, source, target_a, target_b) = seeded_memory().await;
        let sent = send(
            &db.conn,
            invoke_input(
                source,
                vec![target_a, target_b],
                "invoke-fanout",
                "compare the evidence",
            ),
        )
        .await
        .expect("invoke send");
        assert!(sent
            .deliveries
            .iter()
            .all(|delivery| delivery.state == CollaborationDeliveryState::Queued));

        for target in [target_a, target_b] {
            let queue = prompt_queue_service::snapshot(&db.conn, target)
                .await
                .expect("target queue");
            assert_eq!(queue.items.len(), 1);
            assert!(queue.items[0].draft.is_none());
            assert_eq!(
                queue.items[0].origin_event_id.as_deref(),
                Some(sent.event_id.as_str())
            );
            let draft = prompt_draft_for_origin(&db.conn, target, &sent.event_id)
                .await
                .expect("resolved envelope");
            let PromptInputBlock::Text { text } = &draft.blocks[0] else {
                panic!("collaboration delivery must resolve to one text envelope");
            };
            assert!(text.starts_with(&format!("{ENVELOPE_PREFIX}{}>>>", sent.event_id)));
            assert!(text.contains("compare the evidence"));
            assert!(text.contains("Call read_message"));
            assert!(text.contains("\"kind\":\"letter\""));
            assert!(text.ends_with(&format!("{ENVELOPE_END_PREFIX}{}>>>", sent.event_id)));
        }

        let replay = send(
            &db.conn,
            invoke_input(
                source,
                vec![target_a, target_b],
                "invoke-fanout",
                "compare the evidence",
            ),
        )
        .await
        .expect("deduplicated invoke");
        assert!(replay.deduplicated);
        for target in [target_a, target_b] {
            assert_eq!(
                prompt_queue_service::snapshot(&db.conn, target)
                    .await
                    .unwrap()
                    .items
                    .len(),
                1,
                "a lost response retry must not enqueue a second invocation"
            );
        }
    }

    #[tokio::test]
    async fn dedupe_returns_original_delivery_set_without_expanding_targets() {
        let (db, source, target_a, target_b) = seeded_memory().await;
        let first = send(&db.conn, input(source, vec![target_a], "same", "unchanged"))
            .await
            .unwrap();
        let retry = send(
            &db.conn,
            input(source, vec![target_a, target_b], "same", "unchanged"),
        )
        .await
        .unwrap();
        assert_eq!(retry.event_id, first.event_id);
        assert_eq!(retry.deliveries.len(), 1);
        assert_eq!(retry.deliveries[0].target.conversation_id, target_a);
        assert!(send(
            &db.conn,
            input(source, vec![target_a], "same", "changed body"),
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn reply_must_come_from_a_recipient_and_target_only_original_source() {
        let (db, source, target_a, target_b) = seeded_memory().await;
        let original = send(
            &db.conn,
            input(source, vec![target_a], "question", "please review"),
        )
        .await
        .unwrap();

        let mut valid = input(target_a, vec![source], "reply-ok", "review complete");
        valid.reply_to_event_id = Some(original.event_id.clone());
        let reply = send(&db.conn, valid).await.expect("recipient may reply");
        assert_eq!(reply.deliveries[0].target.conversation_id, source);

        let mut unrelated = input(target_b, vec![source], "reply-unrelated", "spoof");
        unrelated.reply_to_event_id = Some(original.event_id.clone());
        assert!(send(&db.conn, unrelated).await.is_err());

        let mut fanout = input(
            target_a,
            vec![source, target_b],
            "reply-fanout",
            "ambiguous reply",
        );
        fanout.reply_to_event_id = Some(original.event_id);
        assert!(send(&db.conn, fanout).await.is_err());
    }

    #[tokio::test]
    async fn same_thread_allows_second_reply_and_author_follow_up() {
        let (db, source, target_a, _target_b) = seeded_memory().await;
        let original = send(
            &db.conn,
            input(source, vec![target_a], "question", "please review"),
        )
        .await
        .unwrap();

        let mut first = input(target_a, vec![source], "reply-1", "first pass");
        first.reply_to_event_id = Some(original.event_id.clone());
        let reply = send(&db.conn, first).await.expect("first reply");

        let mut supplement_on_inbound =
            input(target_a, vec![source], "reply-2", "one more finding");
        supplement_on_inbound.reply_to_event_id = Some(original.event_id.clone());
        send(&db.conn, supplement_on_inbound)
            .await
            .expect("second reply to the same inbound event");

        let mut supplement_on_own = input(target_a, vec![source], "reply-3", "typo fix");
        supplement_on_own.reply_to_event_id = Some(reply.event_id);
        send(&db.conn, supplement_on_own)
            .await
            .expect("follow-up to own last letter");
    }

    #[tokio::test]
    async fn reply_projection_is_derived_independently_for_each_target() {
        let (db, source, target_a, target_b) = seeded_memory().await;
        let mut question = input(
            source,
            vec![target_a, target_b],
            "question-with-two-targets",
            "please review",
        );
        question.expects_reply = true;
        let original = send(&db.conn, question).await.unwrap();

        let before = feed(&db.conn, source, None).await.unwrap();
        assert!(before.outbound.iter().all(|item| !item.reply_received));

        let mut answer = input(target_a, vec![source], "answer-a", "review complete");
        answer.reply_to_event_id = Some(original.event_id);
        send(&db.conn, answer).await.unwrap();

        let source_feed = feed(&db.conn, source, None).await.unwrap();
        let delivery_a = source_feed
            .outbound
            .iter()
            .find(|item| item.target.conversation_id == target_a)
            .expect("first target delivery");
        let delivery_b = source_feed
            .outbound
            .iter()
            .find(|item| item.target.conversation_id == target_b)
            .expect("second target delivery");
        assert!(delivery_a.reply_received);
        assert!(!delivery_b.reply_received);
        assert_eq!(
            delivery_a.obligation_state,
            CollaborationObligationState::Resolved
        );
        assert!(delivery_a.obligation_resolved_at.is_some());
        assert_eq!(
            delivery_b.obligation_state,
            CollaborationObligationState::AwaitingReply
        );
        assert!(delivery_b.obligation_resolved_at.is_none());

        let recipient_feed = feed(&db.conn, target_a, None).await.unwrap();
        assert!(recipient_feed.inbound[0].reply_received);
    }

    #[tokio::test]
    async fn completed_collaboration_turn_auto_replies_once_without_waking_source() {
        let (db, source, target, _) = seeded_memory().await;
        let mut question =
            invoke_input(source, vec![target], "auto-reply-question", "please answer");
        question.expects_reply = true;
        let original = send(&db.conn, question).await.unwrap();
        db.conn
            .execute(statement(
                "UPDATE collaboration_delivery \
                 SET state = 'embedded', embedded_turn_ref = 'queue-message-1' \
                 WHERE event_id = ? AND target_conversation_id = ?",
                vec![original.event_id.clone().into(), target.into()],
            ))
            .await
            .unwrap();
        mark_agent_read(&db.conn, target, &original.event_id)
            .await
            .unwrap();

        assert!(auto_reply_for_completed_turn(
            &db.conn,
            target,
            "another-message",
            "must not match"
        )
        .await
        .unwrap()
        .is_none());

        let reply = auto_reply_for_completed_turn(
            &db.conn,
            target,
            "queue-message-1",
            "The evidence supports the claim.",
        )
        .await
        .unwrap()
        .expect("normal final answer is projected back to the source");
        assert!(!reply.deduplicated);
        assert_eq!(reply.deliveries.len(), 1);
        assert_eq!(reply.deliveries[0].target.conversation_id, source);
        assert_eq!(
            reply.deliveries[0].reply_to_event_id.as_deref(),
            Some(original.event_id.as_str())
        );
        assert!(!reply.deliveries[0].expects_reply);
        assert_eq!(
            reply.deliveries[0].invocation_policy,
            CollaborationInvocationPolicy::StoreOnly
        );
        assert!(prompt_queue_service::snapshot(&db.conn, source)
            .await
            .unwrap()
            .items
            .is_empty());

        assert!(auto_reply_for_completed_turn(
            &db.conn,
            target,
            "queue-message-1",
            "a duplicate terminal event"
        )
        .await
        .unwrap()
        .is_none());
        let source_feed = feed(&db.conn, source, None).await.unwrap();
        assert_eq!(
            source_feed.unread_count, 1,
            "the answer is delivered visibly without waking the source Harness"
        );
        assert!(
            source_feed
                .outbound
                .iter()
                .find(|item| item.event_id == original.event_id)
                .expect("original outbound request")
                .reply_received
        );
        assert_eq!(
            source_feed
                .inbound
                .iter()
                .filter(|item| item.reply_to_event_id.as_deref()
                    == Some(original.event_id.as_str()))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn open_obligation_note_restates_debt_titles_until_the_reply_lands() {
        let (db, source, target, _) = seeded_memory().await;
        let mut letter = input(
            source,
            vec![target],
            "note-debt",
            "body must stay in the ledger",
        );
        letter.subject = "Fix the parser".into();
        letter.expects_reply = true;
        let sent = send(&db.conn, letter).await.unwrap();

        assert!(
            open_obligation_note_for_turn(&db.conn, target)
                .await
                .unwrap()
                .is_none(),
            "a letter that never completed a delivery is the envelope's job, not the note's"
        );
        db.conn
            .execute(statement(
                "UPDATE collaboration_delivery \
                 SET state = 'embedded', embedded_turn_ref = 'turn-note-1' \
                 WHERE target_conversation_id = ?",
                vec![target.into()],
            ))
            .await
            .unwrap();

        let note = open_obligation_note_for_turn(&db.conn, target)
            .await
            .unwrap()
            .expect("an open obligation produces a tail note");
        let PromptInputBlock::Text { text } = note else {
            panic!("the note is a text block");
        };
        assert!(text.contains("Fix the parser"));
        assert!(text.contains(&sent.event_id));
        assert!(text.contains("未读"), "embedded but never read stays 未读");
        assert!(
            !text.contains("body must stay in the ledger"),
            "titles only, never bodies"
        );
        assert!(
            open_obligation_note_for_turn(&db.conn, source)
                .await
                .unwrap()
                .is_none(),
            "the sender carries no debt"
        );

        let mut reply = input(target, vec![source], "note-reply", "done");
        reply.reply_to_event_id = Some(sent.event_id);
        send(&db.conn, reply).await.unwrap();
        assert!(
            open_obligation_note_for_turn(&db.conn, target)
                .await
                .unwrap()
                .is_none(),
            "the reply cleared the debt, so the note stands down"
        );
    }

    #[tokio::test]
    async fn open_obligation_note_marks_room_mentions_with_their_room() {
        let (db, source, target, _) = seeded_memory().await;
        let room = collaboration_room_service::create(
            &db.conn,
            crate::models::CreateCollaborationRoomInput {
                workbench_id: 1,
                title: "Plan".into(),
                member_conversation_ids: vec![source, target],
                created_by_conversation_id: source,
                collection_id: None,
                root_folder_id: None,
            },
        )
        .await
        .expect("create room");
        let posted = post_room(
            &db.conn,
            PostRoomMessageInput {
                room_id: room.id.clone(),
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                mention_all: false,
                body: "look at the plan".into(),
                client_dedupe_id: "note-room-1".into(),
                invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: true,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
                mention_human: false,
                author_kind: Default::default(),
            },
        )
        .await
        .expect("post");
        db.conn
            .execute(statement(
                "UPDATE collaboration_delivery \
                 SET state = 'embedded', embedded_turn_ref = 'turn-note-2' \
                 WHERE target_conversation_id = ?",
                vec![target.into()],
            ))
            .await
            .unwrap();
        let note = open_obligation_note_for_turn(&db.conn, target)
            .await
            .unwrap()
            .expect("an unanswered Room mention is an open obligation");
        let PromptInputBlock::Text { text } = note else {
            panic!("the note is a text block");
        };
        assert!(text.contains("群点名"));
        assert!(text.contains(&format!("room_id={}", room.id)));
        assert!(text.contains(&posted.event_id));
    }

    async fn seeded_room(
        db: &crate::db::AppDatabase,
        source: i32,
        target: i32,
    ) -> crate::models::CollaborationRoomDetail {
        collaboration_room_service::create(
            &db.conn,
            crate::models::CreateCollaborationRoomInput {
                workbench_id: 1,
                title: "Plan".into(),
                member_conversation_ids: vec![source, target],
                created_by_conversation_id: source,
                collection_id: None,
                root_folder_id: None,
            },
        )
        .await
        .expect("create room")
    }

    fn room_post(
        room_id: &str,
        source: i32,
        targets: Vec<i32>,
        dedupe: &str,
    ) -> PostRoomMessageInput {
        PostRoomMessageInput {
            room_id: room_id.to_string(),
            source_conversation_id: source,
            target_conversation_ids: targets,
            mention_all: false,
            body: "look at the plan".into(),
            client_dedupe_id: dedupe.into(),
            invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
            delivery_hint: CollaborationDeliveryHint::Default,
            expects_reply: true,
            urgency: CollaborationUrgency::Normal,
            reply_to_event_id: None,
            mention_human: false,
            author_kind: Default::default(),
        }
    }

    #[tokio::test]
    async fn mailbox_reply_to_a_room_event_is_rejected_and_keeps_the_room_debt() {
        let (db, source, target, _) = seeded_memory().await;
        let room = seeded_room(&db, source, target).await;
        let posted = post_room(
            &db.conn,
            room_post(&room.id, source, vec![target], "room-q"),
        )
        .await
        .expect("post");

        let mut letter = input(target, vec![source], "sneaky-reply", "answering privately");
        letter.reply_to_event_id = Some(posted.event_id.clone());
        let err = send(&db.conn, letter)
            .await
            .expect_err("a Room post must be answered in the Room");
        assert!(
            err.to_string().contains("post_room"),
            "the error points at the Room channel: {err}"
        );

        db.conn
            .execute(statement(
                "UPDATE collaboration_delivery \
                 SET state = 'embedded', embedded_turn_ref = 'turn-room-debt' \
                 WHERE target_conversation_id = ?",
                vec![target.into()],
            ))
            .await
            .unwrap();
        let debt = open_obligation_digest(&db.conn, target).await.unwrap();
        assert_eq!(
            debt.room_mentions_owed, 1,
            "a rejected private reply must not clear the Room obligation"
        );
    }

    #[tokio::test]
    async fn room_reply_to_a_mailbox_letter_is_rejected() {
        let (db, source, target, _) = seeded_memory().await;
        let room = seeded_room(&db, source, target).await;
        let sent = send(
            &db.conn,
            input(source, vec![target], "direct-q", "private question"),
        )
        .await
        .expect("send");

        let mut post = room_post(&room.id, target, vec![source], "room-sneaky");
        post.reply_to_event_id = Some(sent.event_id.clone());
        let err = post_room(&db.conn, post)
            .await
            .expect_err("a mailbox letter must be answered by mailbox");
        assert!(
            err.to_string().contains("same Room"),
            "symmetric with the mailbox direction: {err}"
        );
    }

    #[tokio::test]
    async fn explicit_agent_reply_suppresses_the_automatic_fallback() {
        let (db, source, target, _) = seeded_memory().await;
        let mut question = invoke_input(
            source,
            vec![target],
            "explicit-reply-question",
            "please answer",
        );
        question.expects_reply = true;
        let original = send(&db.conn, question).await.unwrap();
        db.conn
            .execute(statement(
                "UPDATE collaboration_delivery \
                 SET state = 'embedded', embedded_turn_ref = 'queue-message-2' \
                 WHERE event_id = ? AND target_conversation_id = ?",
                vec![original.event_id.clone().into(), target.into()],
            ))
            .await
            .unwrap();

        let mut explicit = input(target, vec![source], "explicit-reply", "already sent");
        explicit.reply_to_event_id = Some(original.event_id);
        send(&db.conn, explicit).await.unwrap();

        assert!(auto_reply_for_completed_turn(
            &db.conn,
            target,
            "queue-message-2",
            "must not be duplicated"
        )
        .await
        .unwrap()
        .is_none());
    }

    #[test]
    fn automatic_reply_body_is_utf8_safe_and_keeps_a_source_link_when_bounded() {
        let answer = "结论".repeat(MAX_BODY_BYTES / "结论".len() + 32);
        let body = bounded_auto_reply_body(42, "attempt-二", &answer);

        assert!(body.len() <= MAX_BODY_BYTES);
        assert!(body.starts_with("结论结论"));
        assert!(body.contains("codeg://session/42"));
        assert!(body.contains("attempt-二"));
        assert!(body.contains("Reply truncated"));
    }

    #[tokio::test]
    async fn agent_reply_chain_stops_asking_for_replies_at_the_fuse() {
        let (db, first, second, _) = seeded_memory().await;
        let mut source = first;
        let mut target = second;
        let mut reply_to_event_id = None;

        for depth in 0..=MAX_AGENT_REPLY_CHAIN_DEPTH + 1 {
            let mut next = input(
                source,
                vec![target],
                &format!("agent-depth-{depth}"),
                "continue the discussion",
            );
            next.expects_reply = true;
            next.reply_to_event_id = reply_to_event_id.clone();
            let sent = send(&db.conn, next).await.unwrap();
            if depth < MAX_AGENT_REPLY_CHAIN_DEPTH {
                assert!(
                    sent.deliveries[0].expects_reply,
                    "depth {depth} is below the fuse"
                );
                assert_eq!(
                    sent.deliveries[0].obligation_state,
                    CollaborationObligationState::AwaitingReply
                );
            } else {
                assert!(
                    !sent.deliveries[0].expects_reply,
                    "depth {depth} hits the fuse: the letter lands but cannot ask for a reply"
                );
                assert_eq!(
                    sent.deliveries[0].obligation_state,
                    CollaborationObligationState::None
                );
            }
            reply_to_event_id = Some(sent.event_id);
            std::mem::swap(&mut source, &mut target);
        }

        let row = db
            .conn
            .query_one(statement(
                "SELECT chain_depth FROM collaboration_event WHERE id = ?",
                vec![reply_to_event_id.unwrap().into()],
            ))
            .await
            .unwrap()
            .unwrap();
        let stored_depth: i32 = row.try_get("", "chain_depth").unwrap();
        assert_eq!(
            stored_depth,
            MAX_AGENT_REPLY_CHAIN_DEPTH + 1,
            "chain depth keeps being recorded past the fuse"
        );
    }

    #[tokio::test]
    async fn human_room_posts_keep_asking_for_replies_past_the_fuse() {
        let (db, a, b, _) = seeded_memory().await;
        let room = seeded_room(&db, a, b).await;
        // Grow an Agent thread up to the fuse: the last Agent post lands at
        // depth MAX and loses its reply request.
        let mut reply_to = None;
        let mut author = a;
        let mut other = b;
        for depth in 0..=MAX_AGENT_REPLY_CHAIN_DEPTH {
            let mut post = room_post(&room.id, author, vec![other], &format!("fuse-{depth}"));
            post.reply_to_event_id = reply_to.clone();
            let posted = post_room(&db.conn, post).await.expect("post");
            assert_eq!(
                posted.deliveries[0].expects_reply,
                depth < MAX_AGENT_REPLY_CHAIN_DEPTH,
                "agent post at depth {depth}"
            );
            reply_to = Some(posted.event_id);
            std::mem::swap(&mut author, &mut other);
        }
        // A human follow-up on the depth-MAX thread still asks for a reply.
        let mut human = room_post(&room.id, a, vec![b], "human-past-fuse");
        human.reply_to_event_id = reply_to;
        human.author_kind = CollaborationAuthorKind::Human;
        let posted = post_room(&db.conn, human).await.expect("human post");
        assert!(
            posted.deliveries[0].expects_reply,
            "human UI posts stay unrestricted"
        );
    }

    #[tokio::test]
    async fn concurrent_dedupe_race_creates_one_event_and_one_fanout() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = fresh_disk_db(dir.path()).await;
        let folder_id = seed_folder(&db, "/tmp/codeg-collaboration-race").await;
        let source = seed_conversation(&db, folder_id, AgentType::Codex).await;
        let target = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let request = input(source, vec![target], "double-click", "only once");

        let (first, second) =
            tokio::join!(send(&db.conn, request.clone()), send(&db.conn, request));
        let first = first.expect("first send");
        let second = second.expect("deduplicated concurrent send");
        assert_eq!(first.event_id, second.event_id);
        assert_eq!(first.deliveries.len(), 1);
        assert_eq!(second.deliveries.len(), 1);

        let count: i64 = db
            .conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS count FROM collaboration_event",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "count")
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn missing_target_is_a_failed_delivery_without_harming_live_targets() {
        let (db, source, target_a, _) = seeded_memory().await;
        let mut request = input(source, vec![target_a, 999_999], "partial", "hello");
        request.expects_reply = true;
        let sent = send(&db.conn, request).await.unwrap();
        assert_eq!(sent.deliveries.len(), 2);
        let live = sent
            .deliveries
            .iter()
            .find(|item| item.target.conversation_id == target_a)
            .unwrap();
        let missing = sent
            .deliveries
            .iter()
            .find(|item| item.target.conversation_id == 999_999)
            .unwrap();
        assert_eq!(live.state, CollaborationDeliveryState::Pending);
        assert_eq!(missing.state, CollaborationDeliveryState::Failed);
        assert_eq!(missing.error.as_deref(), Some("target_not_found"));
        assert_eq!(
            missing.obligation_state,
            CollaborationObligationState::None,
            "an unreachable target cannot owe a reply"
        );
    }

    #[tokio::test]
    async fn archived_target_refuses_new_mail_and_the_sender_sees_why() {
        let (db, source, target_a, target_b) = seeded_memory().await;
        crate::db::service::conversation_service::update_archive(&db.conn, target_a, true)
            .await
            .expect("archive");

        let err = send(
            &db.conn,
            input(source, vec![target_a], "archived-1", "anybody home?"),
        )
        .await
        .expect_err("an archived Session refuses new mail");
        let message = err.to_string();
        assert!(message.contains("Archived"), "names the reason: {message}");
        assert!(
            message.contains(&target_a.to_string()),
            "names the archived Session: {message}"
        );

        // A mixed fan-out fails as one send rather than half-landing.
        let err = send(
            &db.conn,
            input(source, vec![target_a, target_b], "archived-2", "team note"),
        )
        .await
        .expect_err("one archived target refuses the whole letter");
        assert!(err.to_string().contains("Archived"));
        assert!(
            feed(&db.conn, target_b, None)
                .await
                .unwrap()
                .inbound
                .is_empty(),
            "nothing was persisted for the live target either"
        );

        // An existing thread cannot be continued while the peer is archived.
        let sent = send(
            &db.conn,
            input(source, vec![target_b], "thread-q", "question"),
        )
        .await
        .expect("send");
        crate::db::service::conversation_service::update_archive(&db.conn, source, true)
            .await
            .expect("archive source");
        let mut reply = input(target_b, vec![source], "thread-a", "answer");
        reply.reply_to_event_id = Some(sent.event_id.clone());
        let err = send(&db.conn, reply)
            .await
            .expect_err("replies to an archived Session are refused too");
        assert!(err.to_string().contains("Archived"));
    }

    #[tokio::test]
    async fn unarchived_target_accepts_mail_again() {
        let (db, source, target, _) = seeded_memory().await;
        crate::db::service::conversation_service::update_archive(&db.conn, target, true)
            .await
            .expect("archive");
        send(
            &db.conn,
            input(source, vec![target], "while-archived", "held"),
        )
        .await
        .expect_err("archived Sessions refuse mail");
        crate::db::service::conversation_service::update_archive(&db.conn, target, false)
            .await
            .expect("restore");
        let sent = send(
            &db.conn,
            input(source, vec![target], "after-restore", "back?"),
        )
        .await
        .expect("mail flows again after unarchive");
        assert_eq!(sent.deliveries.len(), 1);
        assert_eq!(
            sent.deliveries[0].state,
            CollaborationDeliveryState::Pending
        );
    }

    #[tokio::test]
    async fn archiving_the_creditor_waives_the_reply_owed_to_it() {
        let (db, source, target, _) = seeded_memory().await;
        let mut letter = input(source, vec![target], "waive-q", "answer me");
        letter.expects_reply = true;
        let sent = send(&db.conn, letter).await.unwrap();
        // The digest only counts debts whose letter actually reached a turn.
        db.conn
            .execute(statement(
                "UPDATE collaboration_delivery \
                 SET state = 'embedded', embedded_turn_ref = 'turn-waive', \
                     agent_received_at = CURRENT_TIMESTAMP \
                 WHERE event_id = ?",
                vec![sent.event_id.clone().into()],
            ))
            .await
            .unwrap();
        let before = open_obligation_digest(&db.conn, target).await.unwrap();
        assert_eq!(before.letters_owed, 1, "the debtor owes the reply");

        let waived = resolve_obligations_owed_to(&db.conn, source)
            .await
            .expect("waive");
        assert_eq!(waived, vec![target]);
        let after = open_obligation_digest(&db.conn, target).await.unwrap();
        assert_eq!(after.letters_owed, 0, "the digest no longer lists the debt");

        // Unarchiving never revives a waived debt.
        crate::db::service::conversation_service::update_archive(&db.conn, source, false)
            .await
            .expect("restore");
        let restored = open_obligation_digest(&db.conn, target).await.unwrap();
        assert_eq!(restored.letters_owed, 0);
    }

    #[tokio::test]
    async fn archived_debtor_is_skipped_by_the_reminder_sweep() {
        let (db, source, target, _) = seeded_memory().await;
        let sent = send(
            &db.conn,
            invoke_input(source, vec![target], "due-archived", "hello"),
        )
        .await
        .unwrap();
        db.conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE collaboration_delivery \
                 SET created_at = datetime('now', '-6 minutes') \
                 WHERE event_id = ?",
                vec![sent.event_id.clone().into()],
            ))
            .await
            .unwrap();
        assert_eq!(
            list_overdue_reminder_targets(&db.conn).await.unwrap().len(),
            1,
            "overdue unread mail is due for a reminder"
        );

        crate::db::service::conversation_service::update_archive(&db.conn, target, true)
            .await
            .expect("archive debtor");
        assert!(
            list_overdue_reminder_targets(&db.conn)
                .await
                .unwrap()
                .is_empty(),
            "an archived debtor's queue never runs, so no reminder is queued"
        );

        crate::db::service::conversation_service::update_archive(&db.conn, target, false)
            .await
            .expect("restore debtor");
        assert_eq!(
            list_overdue_reminder_targets(&db.conn).await.unwrap().len(),
            1,
            "the frozen debt becomes due again after restore"
        );
    }

    #[tokio::test]
    async fn read_is_monotonic_and_separate_from_delivery_state() {
        let (db, source, target, _) = seeded_memory().await;
        let sent = send(&db.conn, input(source, vec![target], "read", "hello"))
            .await
            .unwrap();
        let delivery_id = sent.deliveries[0].id.clone();
        let before = feed(&db.conn, target, None).await.unwrap();
        let marked = mark_seen(&db.conn, target, vec![delivery_id.clone()])
            .await
            .unwrap();
        assert_eq!(
            marked.feed.unread_count, 1,
            "a human opening the feed must not consume Agent unread"
        );
        assert_eq!(
            marked.feed.inbound[0].state,
            CollaborationDeliveryState::Pending
        );
        assert_eq!(
            marked.feed.inbound[0].attention_state,
            CollaborationAttentionState::Opened
        );
        assert!(marked.feed.inbound[0].opened_at.is_some());
        assert!(marked.feed.inbound[0].ui_seen_at.is_some());
        assert!(marked.feed.inbound[0].agent_received_at.is_none());
        assert_eq!(
            marked.feed.inbound[0].obligation_state,
            CollaborationObligationState::None
        );
        let repeated = mark_seen(&db.conn, target, vec![delivery_id])
            .await
            .unwrap();
        assert_eq!(repeated.feed.revision, marked.feed.revision);
        assert!(marked.feed.revision > before.revision);
    }

    #[tokio::test]
    async fn unread_overview_tracks_live_sessions_without_loading_every_feed() {
        let (db, source, target_a, target_b) = seeded_memory().await;
        let sent = send(
            &db.conn,
            input(
                source,
                vec![target_a, target_b, 999_999],
                "overview",
                "review this",
            ),
        )
        .await
        .unwrap();

        let overview = unread_overview(&db.conn).await.unwrap();
        assert_eq!(overview.total_unread_count, 2);
        assert_eq!(overview.total_needs_reply_count, 0);
        assert_eq!(overview.total_awaiting_reply_count, 0);
        assert_eq!(overview.total_failed_count, 1);
        let status = |conversation_id| {
            overview
                .sessions
                .iter()
                .find(|session| session.conversation_id == conversation_id)
                .expect("actionable Session")
        };
        assert_eq!(status(source).failed_count, 1);
        assert_eq!(status(target_a).unread_count, 1);
        assert_eq!(status(target_b).unread_count, 1);

        let delivery_a = sent
            .deliveries
            .iter()
            .find(|delivery| delivery.target.conversation_id == target_a)
            .unwrap();
        db.conn
            .execute(statement(
                "UPDATE collaboration_delivery SET state = 'failed', error = 'transport' \
                 WHERE id = ?",
                vec![delivery_a.id.clone().into()],
            ))
            .await
            .unwrap();
        let with_live_failure = unread_overview(&db.conn).await.unwrap();
        assert_eq!(
            with_live_failure.total_failed_count, 2,
            "one live-to-live failure is one delivery even though both Session rows surface it"
        );
        let live_failure_status = |conversation_id| {
            with_live_failure
                .sessions
                .iter()
                .find(|session| session.conversation_id == conversation_id)
                .expect("actionable Session")
        };
        assert_eq!(live_failure_status(source).failed_count, 2);
        assert_eq!(live_failure_status(target_a).failed_count, 1);

        mark_seen(&db.conn, target_a, vec![delivery_a.id.clone()])
            .await
            .unwrap();
        let after = unread_overview(&db.conn).await.unwrap();
        assert_eq!(
            after.total_unread_count, 2,
            "human mark_seen must not clear Agent unread on either target"
        );
        assert_eq!(after.total_failed_count, 2);
        assert!(after
            .sessions
            .iter()
            .any(|session| session.conversation_id == target_b && session.unread_count == 1));
        assert!(!after.sessions.iter().any(|session| {
            session.conversation_id == target_a
                && session.unread_count > 0
                && session.needs_reply_count == 0
                && session.awaiting_reply_count == 0
                && session.failed_count == 0
        }));
    }

    #[tokio::test]
    async fn unread_overview_counts_room_debt_beside_direct_mail_not_inside_it() {
        let (db, source, target_a, target_b) = seeded_memory().await;
        let room = seeded_room(&db, source, target_a).await;
        post_room(
            &db.conn,
            room_post(&room.id, source, vec![target_a], "overview-room-ask"),
        )
        .await
        .expect("room ask");
        let mut letter = input(
            source,
            vec![target_b],
            "overview-mail-ask",
            "please confirm",
        );
        letter.expects_reply = true;
        send(&db.conn, letter).await.expect("mail ask");

        let overview = unread_overview(&db.conn).await.unwrap();
        assert_eq!(
            overview.total_needs_reply_count, 1,
            "the direct-mail total must stay direct-mail only"
        );
        assert_eq!(overview.total_room_needs_reply_count, 1);
        assert_eq!(overview.total_room_unread_count, 1);
        assert_eq!(
            overview
                .sessions
                .iter()
                .map(|session| session.needs_reply_count)
                .sum::<u32>(),
            1,
            "a Room obligation has no Session mailbox to land in"
        );
        let addressee_mail_debt = overview
            .sessions
            .iter()
            .find(|session| session.conversation_id == target_a)
            .map_or(0, |session| session.needs_reply_count);
        assert_eq!(
            addressee_mail_debt, 0,
            "the Room's addressee must not show mail debt it does not have"
        );

        // What the sidebar badge adds up is what the Rooms page can show the
        // user to clear it.
        let rooms = collaboration_room_service::list_for_workbench(&db.conn, 1)
            .await
            .unwrap();
        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].needs_reply_count, 1);
        assert_eq!(
            rooms[0].needs_reply_count,
            overview.total_room_needs_reply_count
        );
    }

    #[tokio::test]
    async fn no_reply_needed_resolves_only_the_owned_fanout_delivery() {
        let (db, source, target_a, target_b) = seeded_memory().await;
        let mut request = input(
            source,
            vec![target_a, target_b],
            "manual-fanout-resolution",
            "reply only if needed",
        );
        request.expects_reply = true;
        let sent = send(&db.conn, request).await.unwrap();
        let delivery_a = sent
            .deliveries
            .iter()
            .find(|delivery| delivery.target.conversation_id == target_a)
            .unwrap();

        let resolved = resolve_obligation(&db.conn, target_a, &delivery_a.id)
            .await
            .unwrap();
        assert_eq!(
            resolved.feed.inbound[0].obligation_state,
            CollaborationObligationState::Resolved
        );
        let source_feed = feed(&db.conn, source, None).await.unwrap();
        assert_eq!(
            source_feed
                .outbound
                .iter()
                .find(|delivery| delivery.target.conversation_id == target_a)
                .unwrap()
                .obligation_state,
            CollaborationObligationState::Resolved
        );
        assert_eq!(
            source_feed
                .outbound
                .iter()
                .find(|delivery| delivery.target.conversation_id == target_b)
                .unwrap()
                .obligation_state,
            CollaborationObligationState::AwaitingReply
        );

        let repeated = resolve_obligation(&db.conn, target_a, &delivery_a.id)
            .await
            .unwrap();
        assert!(repeated.affected_conversation_ids.is_empty());
        assert!(resolve_obligation(&db.conn, target_b, &delivery_a.id)
            .await
            .is_err());

        let claimed = claim_pending_store_only_for_turn(&db.conn, target_a, "waived-turn")
            .await
            .unwrap()
            .expect("the message remains available as context");
        let envelope = claimed
            .blocks
            .iter()
            .find_map(|block| match block {
                PromptInputBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .expect("text envelope");
        assert!(envelope.contains("\"expectsReply\":false"));
    }

    #[tokio::test]
    async fn agent_inbox_filters_and_read_do_not_clear_reply_debt() {
        use crate::acp::session_collaboration::{SessionInboxFilter, SessionMailboxScope};
        let (db, source, target, other) = seeded_memory().await;
        let mut request = input(source, vec![target], "inbox-letter", "please review");
        request.expects_reply = true;
        let sent = send(&db.conn, request).await.unwrap();
        let fyi = send(
            &db.conn,
            input(source, vec![target], "inbox-fyi", "just so you know"),
        )
        .await
        .unwrap();

        let open = list_inbox(
            &db.conn,
            target,
            SessionMailboxScope::Inbox,
            SessionInboxFilter::Open,
            None,
            20,
        )
        .await
        .unwrap();
        assert_eq!(open.len(), 2);
        assert!(open.iter().all(|item| item.agent_received_at.is_none()));

        mark_agent_read(&db.conn, target, &fyi.event_id)
            .await
            .unwrap();
        let unread = list_inbox(
            &db.conn,
            target,
            SessionMailboxScope::Inbox,
            SessionInboxFilter::Unread,
            None,
            20,
        )
        .await
        .unwrap();
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].event_id, sent.event_id);

        mark_agent_read(&db.conn, target, &sent.event_id)
            .await
            .unwrap();
        let awaiting = list_inbox(
            &db.conn,
            target,
            SessionMailboxScope::Inbox,
            SessionInboxFilter::AwaitingReply,
            None,
            20,
        )
        .await
        .unwrap();
        assert_eq!(awaiting.len(), 1);
        assert_eq!(awaiting[0].event_id, sent.event_id);
        assert!(awaiting[0].agent_received_at.is_some());

        let letter = get_inbound_message(&db.conn, target, &sent.event_id)
            .await
            .unwrap();
        assert_eq!(letter.body, "please review");
        assert!(get_inbound_message(&db.conn, other, &sent.event_id)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn sent_box_lists_outbound_debt_and_peer_filter_narrows() {
        use crate::acp::session_collaboration::{SessionInboxFilter, SessionMailboxScope};
        let (db, source, target, other) = seeded_memory().await;
        let mut ask = input(source, vec![target], "sent-ask", "please review");
        ask.expects_reply = true;
        let asked = send(&db.conn, ask).await.unwrap();
        send(
            &db.conn,
            input(source, vec![other], "sent-fyi", "just so you know"),
        )
        .await
        .unwrap();

        // Sent box + awaiting_reply: only the letter whose recipient owes us.
        let awaiting = list_inbox(
            &db.conn,
            source,
            SessionMailboxScope::Sent,
            SessionInboxFilter::AwaitingReply,
            None,
            20,
        )
        .await
        .unwrap();
        assert_eq!(awaiting.len(), 1);
        assert_eq!(awaiting[0].event_id, asked.event_id);
        assert_eq!(awaiting[0].target.conversation_id, target);

        // The peer filter narrows the sent box to one exchange.
        let with_other = list_inbox(
            &db.conn,
            source,
            SessionMailboxScope::Sent,
            SessionInboxFilter::All,
            Some(other),
            20,
        )
        .await
        .unwrap();
        assert_eq!(with_other.len(), 1);
        assert_eq!(with_other[0].target.conversation_id, other);

        // Inbox side: target got mail from source only, so a peer filter on
        // `other` must come back empty.
        let none_from_other = list_inbox(
            &db.conn,
            target,
            SessionMailboxScope::Inbox,
            SessionInboxFilter::All,
            Some(other),
            20,
        )
        .await
        .unwrap();
        assert!(none_from_other.is_empty());
    }

    #[tokio::test]
    async fn no_reply_needed_prevents_turn_completion_from_auto_replying() {
        let (db, source, target, _) = seeded_memory().await;
        let mut request =
            invoke_input(source, vec![target], "waived-auto-reply", "reply if needed");
        request.expects_reply = true;
        let sent = send(&db.conn, request).await.unwrap();
        let delivery_id = sent.deliveries[0].id.clone();
        db.conn
            .execute(statement(
                "UPDATE collaboration_delivery \
                 SET state = 'embedded', embedded_turn_ref = 'waived-message' \
                 WHERE id = ?",
                vec![delivery_id.clone().into()],
            ))
            .await
            .unwrap();
        resolve_obligation(&db.conn, target, &delivery_id)
            .await
            .unwrap();

        assert!(auto_reply_for_completed_turn(
            &db.conn,
            target,
            "waived-message",
            "This must stay in the local Session only."
        )
        .await
        .unwrap()
        .is_none());
        assert!(feed(&db.conn, source, None)
            .await
            .unwrap()
            .inbound
            .is_empty());
    }

    #[tokio::test]
    async fn dismiss_updates_sender_and_target_views_without_deleting_event() {
        let (db, source, target, _) = seeded_memory().await;
        let sent = send(&db.conn, input(source, vec![target], "dismiss", "hello"))
            .await
            .unwrap();
        let result = dismiss(&db.conn, target, &sent.deliveries[0].id)
            .await
            .unwrap();
        assert_eq!(result.feed.unread_count, 0);
        assert_eq!(
            result.feed.inbound[0].state,
            CollaborationDeliveryState::Dismissed
        );
        let source_feed = feed(&db.conn, source, None).await.unwrap();
        assert_eq!(
            source_feed.outbound[0].state,
            CollaborationDeliveryState::Dismissed
        );
    }

    #[tokio::test]
    async fn origin_redelivery_promotes_resting_states_and_refuses_dismissed() {
        let (db, source, target_a, _) = seeded_memory().await;

        // Dismissed letters must never be re-queued: their draft cannot be
        // rendered at claim time, so the queued item would be poison.
        let dismissed = send(
            &db.conn,
            input(source, vec![target_a], "redeliver-dead", "gone"),
        )
        .await
        .unwrap();
        dismiss(&db.conn, target_a, &dismissed.deliveries[0].id)
            .await
            .unwrap();
        let txn = db.conn.begin().await.unwrap();
        assert!(
            !prepare_origin_redelivery(&txn, target_a, &dismissed.event_id)
                .await
                .unwrap(),
            "a dismissed delivery is not re-deliverable"
        );
        txn.commit().await.unwrap();

        // An already-embedded letter may be nagged again: it returns to
        // `queued` so the dispatch protocol applies to the second injection.
        let embedded = send(
            &db.conn,
            invoke_input(source, vec![target_a], "redeliver-embedded", "answer me"),
        )
        .await
        .unwrap();
        let txn = db.conn.begin().await.unwrap();
        assert!(
            mark_origin_embedding(&txn, target_a, &embedded.event_id, "turn-1")
                .await
                .unwrap()
        );
        assert!(
            mark_origin_embedded(&txn, target_a, &embedded.event_id, "turn-1")
                .await
                .unwrap()
        );
        txn.commit().await.unwrap();

        let txn = db.conn.begin().await.unwrap();
        assert!(
            prepare_origin_redelivery(&txn, target_a, &embedded.event_id)
                .await
                .unwrap()
        );
        txn.commit().await.unwrap();
        let refreshed = feed(&db.conn, target_a, None).await.unwrap();
        let row = refreshed
            .inbound
            .iter()
            .find(|item| item.event_id == embedded.event_id)
            .expect("the embedded letter stays visible");
        assert_eq!(row.state, CollaborationDeliveryState::Queued);
    }

    #[tokio::test]
    async fn dismissed_store_only_mail_can_return_to_future_context() {
        let (db, source, target, _) = seeded_memory().await;
        let sent = send(
            &db.conn,
            input(
                source,
                vec![target],
                "restore-dismissed",
                "include me later",
            ),
        )
        .await
        .unwrap();
        let delivery_id = &sent.deliveries[0].id;

        dismiss(&db.conn, target, delivery_id).await.unwrap();
        let restored = restore(&db.conn, target, delivery_id).await.unwrap();
        assert_eq!(
            restored.feed.unread_count, 1,
            "restored mail is still unread until the Agent receives it"
        );
        assert_eq!(
            restored.feed.inbound[0].state,
            CollaborationDeliveryState::Pending
        );
        assert!(restored.feed.inbound[0].ui_seen_at.is_some());

        let claimed = claim_pending_store_only_for_turn(&db.conn, target, "next-human-turn")
            .await
            .unwrap()
            .expect("restored delivery should be eligible for the next natural turn");
        assert_eq!(claimed.event_ids, vec![sent.event_id]);
        assert!(claimed.blocks.iter().any(|block| matches!(
            block,
            PromptInputBlock::Text { text } if text.contains("Call read_message")
                && text.contains("include me later")
        )));
    }

    #[tokio::test]
    async fn deleting_author_does_not_erase_received_mail_or_frozen_identity() {
        let (db, source, target, _) = seeded_memory().await;
        db.conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE conversation SET title = 'Original author' WHERE id = ?",
                vec![source.into()],
            ))
            .await
            .unwrap();
        send(&db.conn, input(source, vec![target], "durable", "keep me"))
            .await
            .unwrap();
        db.conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "DELETE FROM conversation WHERE id = ?",
                vec![source.into()],
            ))
            .await
            .expect("hard delete source");
        let target_feed = feed(&db.conn, target, None).await.unwrap();
        assert_eq!(target_feed.inbound.len(), 1);
        assert_eq!(
            target_feed.inbound[0].source.title.as_deref(),
            Some("Original author")
        );
        assert_eq!(target_feed.inbound[0].source.conversation_id, source);
    }

    #[tokio::test]
    async fn rename_does_not_change_delivery_identity_or_reply_routing() {
        let (db, source, target, _) = seeded_memory().await;
        db.conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE conversation SET title = 'Before A' WHERE id = ?",
                vec![source.into()],
            ))
            .await
            .unwrap();
        db.conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE conversation SET title = 'Before B' WHERE id = ?",
                vec![target.into()],
            ))
            .await
            .unwrap();

        let mut request = input(source, vec![target], "rename-stable-id", "please review");
        request.expects_reply = true;
        let sent = send(&db.conn, request).await.unwrap();
        assert_eq!(sent.deliveries[0].target.conversation_id, target);
        assert_eq!(sent.deliveries[0].target.title.as_deref(), Some("Before B"));

        db.conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE conversation SET title = 'After A' WHERE id = ?",
                vec![source.into()],
            ))
            .await
            .unwrap();
        db.conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE conversation SET title = 'After B' WHERE id = ?",
                vec![target.into()],
            ))
            .await
            .unwrap();

        let mut reply = input(target, vec![source], "rename-stable-reply", "done");
        reply.reply_to_event_id = Some(sent.event_id.clone());
        let answered = send(&db.conn, reply).await.unwrap();
        assert_eq!(answered.deliveries[0].target.conversation_id, source);

        let source_feed = feed(&db.conn, source, None).await.unwrap();
        let outbound = source_feed
            .outbound
            .iter()
            .find(|item| item.event_id == sent.event_id)
            .expect("original outbound delivery");
        assert_eq!(outbound.target.conversation_id, target);
        assert_eq!(outbound.target.title.as_deref(), Some("Before B"));
        assert!(outbound.reply_received);
        assert_eq!(
            outbound.obligation_state,
            CollaborationObligationState::Resolved
        );

        let target_feed = feed(&db.conn, target, None).await.unwrap();
        assert_eq!(
            target_feed.inbound[0].source.title.as_deref(),
            Some("Before A")
        );
        assert_eq!(target_feed.inbound[0].source.conversation_id, source);
    }

    #[tokio::test]
    async fn event_and_unread_survive_database_reopen() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = fresh_disk_db(dir.path()).await;
        let folder_id = seed_folder(&db, "/tmp/codeg-collaboration-disk").await;
        let source = seed_conversation(&db, folder_id, AgentType::Codex).await;
        let target = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        send(&db.conn, input(source, vec![target], "disk", "persist me"))
            .await
            .unwrap();
        drop(db);

        let reopened = fresh_disk_db(dir.path()).await;
        let restored = feed(&reopened.conn, target, None).await.unwrap();
        assert_eq!(restored.unread_count, 1);
        assert_eq!(restored.inbound[0].body, "persist me");
    }

    #[tokio::test]
    async fn rejects_self_send_and_store_only_steer_hint() {
        let (db, source, target, _) = seeded_memory().await;
        assert!(send(&db.conn, input(source, vec![source], "self", "no"))
            .await
            .is_err());
        let mut steer = input(source, vec![target], "steer", "later");
        steer.delivery_hint = CollaborationDeliveryHint::SteerIfSupported;
        assert!(send(&db.conn, steer).await.is_err());

        let mut invoke = invoke_input(source, vec![target], "steer-invoke", "now if safe");
        invoke.delivery_hint = CollaborationDeliveryHint::SteerIfSupported;
        let sent = send(&db.conn, invoke).await.expect("steer hint is durable");
        assert_eq!(
            sent.deliveries[0].delivery_hint,
            CollaborationDeliveryHint::SteerIfSupported
        );
    }

    #[tokio::test]
    async fn cross_harness_mail_freezes_source_and_target_agent_types() {
        let (db, source, claude, gemini) = seeded_memory().await;
        let sent = send(
            &db.conn,
            input(
                source,
                vec![claude, gemini],
                "cross-harness",
                "same question for two harnesses",
            ),
        )
        .await
        .unwrap();
        assert_eq!(sent.deliveries.len(), 2);

        let claude_in = feed(&db.conn, claude, None)
            .await
            .unwrap()
            .inbound
            .remove(0);
        let gemini_in = feed(&db.conn, gemini, None)
            .await
            .unwrap()
            .inbound
            .remove(0);
        assert_eq!(claude_in.source.agent_type.as_deref(), Some("codex"));
        assert_eq!(claude_in.target.agent_type.as_deref(), Some("claude_code"));
        assert_eq!(gemini_in.source.agent_type.as_deref(), Some("codex"));
        assert_eq!(gemini_in.target.agent_type.as_deref(), Some("gemini"));
        assert_eq!(claude_in.event_id, gemini_in.event_id);
        assert_eq!(claude_in.body, gemini_in.body);

        db.conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE conversation SET agent_type = 'grok' WHERE id = ?",
                vec![source.into()],
            ))
            .await
            .unwrap();
        let still_codex = feed(&db.conn, claude, None)
            .await
            .unwrap()
            .inbound
            .remove(0);
        assert_eq!(still_codex.source.agent_type.as_deref(), Some("codex"));
    }

    #[tokio::test]
    async fn resume_claim_repeats_the_same_collaboration_envelope() {
        let (db, source, target, _) = seeded_memory().await;
        let sent = send(
            &db.conn,
            input(source, vec![target], "resume-envelope", "keep this wording"),
        )
        .await
        .unwrap();
        let first = claim_pending_store_only_for_turn(&db.conn, target, "first-attempt")
            .await
            .unwrap()
            .unwrap();
        let PromptInputBlock::Text { text: first_text } = &first.blocks[0] else {
            panic!("expected a text envelope");
        };
        assert!(first_text.contains("\"sourceAgentType\":\"codex\""));
        assert!(first_text.contains("keep this wording"));
        assert!(first_text.contains("Call read_message"));
        assert!(first_text.contains("\"kind\":\"letter\""));
        assert!(first_text.contains(&sent.event_id));
        release_store_only_batch(&db.conn, target, &first)
            .await
            .unwrap();

        let resumed = claim_pending_store_only_for_turn(&db.conn, target, "resume-attempt")
            .await
            .unwrap()
            .expect("released mail must be claimable after resume");
        let PromptInputBlock::Text { text: resumed_text } = &resumed.blocks[0] else {
            panic!("expected a text envelope");
        };
        assert_eq!(first_text, resumed_text);
        assert!(resumed_text.contains("\"eventId\":"));
        assert!(resumed_text.contains(&sent.event_id));
    }

    #[tokio::test]
    async fn overdue_reminder_ignores_human_seen_and_respects_clock() {
        let (db, source, target, _) = seeded_memory().await;
        let sent = send(&db.conn, invoke_input(source, vec![target], "due", "hello"))
            .await
            .unwrap();
        assert!(
            list_overdue_reminder_targets(&db.conn)
                .await
                .unwrap()
                .is_empty(),
            "new unread mail is not due until the five-minute clock elapses"
        );

        db.conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE collaboration_delivery \
                 SET created_at = datetime('now', '-6 minutes') \
                 WHERE event_id = ?",
                vec![sent.event_id.clone().into()],
            ))
            .await
            .unwrap();
        let immediately = list_overdue_reminder_targets(&db.conn).await.unwrap();
        assert_eq!(
            immediately.len(),
            1,
            "unread mail is due after five minutes"
        );
        assert_eq!(immediately[0].conversation_id, target);

        mark_seen(&db.conn, target, vec![sent.deliveries[0].id.clone()])
            .await
            .unwrap();
        let after_human_open = list_overdue_reminder_targets(&db.conn).await.unwrap();
        assert_eq!(after_human_open.len(), 1);
        assert_eq!(after_human_open[0].overdue_unread, 1);

        record_successful_reminder(&db.conn, target, 0)
            .await
            .unwrap();
        assert!(
            list_overdue_reminder_targets(&db.conn)
                .await
                .unwrap()
                .is_empty(),
            "cooldown after a successful reminder must suppress the next scan"
        );
    }

    #[tokio::test]
    async fn reply_phase_gets_a_fresh_reminder_budget_after_unread_nags() {
        let (db, source, target, _) = seeded_memory().await;
        let mut letter = invoke_input(source, vec![target], "starved-reply", "answer me");
        letter.expects_reply = true;
        let sent = send(&db.conn, letter).await.unwrap();
        // The unread phase burned the whole per-episode budget, and the last
        // nag happened after the letter arrived (nothing newer became due).
        db.conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE collaboration_delivery \
                 SET created_at = datetime('now', '-30 minutes') \
                 WHERE event_id = ?",
                vec![sent.event_id.clone().into()],
            ))
            .await
            .unwrap();
        record_successful_reminder(&db.conn, target, 2)
            .await
            .unwrap();
        db.conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE conversation_collaboration_state \
                 SET reminder_last_at = datetime('now', '-6 minutes') \
                 WHERE conversation_id = ?",
                vec![target.into()],
            ))
            .await
            .unwrap();
        assert!(
            list_overdue_reminder_targets(&db.conn)
                .await
                .unwrap()
                .is_empty(),
            "an exhausted budget with no newer debt must stay silent"
        );

        // The Agent then reads the letter and its five-minute reply clock
        // elapses: that member became due AFTER the last reminder, so the
        // read-awaiting-reply phase must get its own budget.
        mark_agent_read(&db.conn, target, &sent.event_id)
            .await
            .unwrap();
        db.conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE collaboration_delivery \
                 SET agent_received_at = datetime('now', '-5 minutes', '-10 seconds') \
                 WHERE event_id = ?",
                vec![sent.event_id.clone().into()],
            ))
            .await
            .unwrap();
        let overdue = list_overdue_reminder_targets(&db.conn).await.unwrap();
        assert_eq!(
            overdue.len(),
            1,
            "read-awaiting-reply must not be starved by unread-phase nags"
        );
        assert_eq!(overdue[0].conversation_id, target);
        assert_eq!(overdue[0].overdue_reply, 1);
        assert_eq!(
            overdue[0].reminder_repeat_count, 0,
            "a fresh debt episode restarts the repeat budget"
        );
    }

    #[tokio::test]
    async fn overdue_reply_includes_store_only_mail_after_agent_read() {
        let (db, source, target, _) = seeded_memory().await;
        let mut letter = input(
            source,
            vec![target],
            "await-store-only",
            "please answer this",
        );
        letter.expects_reply = true;
        let sent = send(&db.conn, letter).await.unwrap();
        assert!(
            list_overdue_reminder_targets(&db.conn)
                .await
                .unwrap()
                .is_empty(),
            "deliver-only unread mail must not start a reminder turn"
        );

        mark_agent_read(&db.conn, target, &sent.event_id)
            .await
            .unwrap();
        assert!(
            list_overdue_reminder_targets(&db.conn)
                .await
                .unwrap()
                .is_empty(),
            "reply reminder is not due until the five-minute clock elapses"
        );

        db.conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE collaboration_delivery \
                 SET agent_received_at = datetime('now', '-6 minutes') \
                 WHERE event_id = ?",
                vec![sent.event_id.clone().into()],
            ))
            .await
            .unwrap();
        let overdue = list_overdue_reminder_targets(&db.conn).await.unwrap();
        assert_eq!(overdue.len(), 1);
        assert_eq!(overdue[0].conversation_id, target);
        assert_eq!(overdue[0].overdue_unread, 0);
        assert_eq!(overdue[0].overdue_reply, 1);
        assert_eq!(overdue[0].letters.len(), 1);
        assert!(overdue[0].letters[0].awaiting_reply);
    }

    #[tokio::test]
    async fn overdue_unread_does_not_count_failed_deliveries() {
        let (db, source, target, _) = seeded_memory().await;
        let sent = send(
            &db.conn,
            invoke_input(source, vec![target], "failed-unread", "hello"),
        )
        .await
        .unwrap();
        db.conn
            .execute(statement(
                "UPDATE collaboration_delivery \
                 SET state = 'failed', error = 'transport', \
                     created_at = datetime('now', '-6 minutes') \
                 WHERE event_id = ?",
                vec![sent.event_id.clone().into()],
            ))
            .await
            .unwrap();
        assert!(
            list_overdue_reminder_targets(&db.conn)
                .await
                .unwrap()
                .is_empty(),
            "failed deliveries never reached the Agent; they are not reminder debts"
        );
    }

    #[tokio::test]
    async fn overdue_unread_still_counts_queued_deliveries() {
        let (db, source, target, _) = seeded_memory().await;
        let sent = send(
            &db.conn,
            invoke_input(source, vec![target], "queued-unread", "hello"),
        )
        .await
        .unwrap();
        assert_eq!(sent.deliveries[0].state, CollaborationDeliveryState::Queued);
        db.conn
            .execute(statement(
                "UPDATE collaboration_delivery \
                 SET created_at = datetime('now', '-6 minutes') \
                 WHERE event_id = ?",
                vec![sent.event_id.clone().into()],
            ))
            .await
            .unwrap();
        let overdue = list_overdue_reminder_targets(&db.conn).await.unwrap();
        assert_eq!(overdue.len(), 1);
        assert_eq!(overdue[0].conversation_id, target);
        assert_eq!(overdue[0].overdue_unread, 1);
    }

    #[tokio::test]
    async fn first_delivery_puts_body_after_separator_and_asks_read_message_to_consume() {
        let (db, source, target, _) = seeded_memory().await;
        let sent = send(
            &db.conn,
            input(source, vec![target], "resume-envelope", "keep this wording"),
        )
        .await
        .unwrap();
        let draft = prompt_draft_for_origin(&db.conn, target, &sent.event_id)
            .await
            .expect("first delivery must be injectable");
        let PromptInputBlock::Text { text } = &draft.blocks[0] else {
            panic!("expected a text envelope");
        };
        let separator = text
            .find("--- message ---\n")
            .expect("envelope must keep the body after the separator");
        let after = &text[separator + "--- message ---\n".len()..];
        assert!(
            after.starts_with("keep this wording\n"),
            "first delivery must carry the letter body: {after}"
        );
        assert!(text.contains("\"kind\":\"letter\""));
        assert!(text.contains("\"channel\":\"mailbox\""));
        assert!(text.contains("channel=mailbox"));
        assert!(text.contains("Call read_message"));
        assert!(!text.contains("Call list_inbox to see titles"));
        assert!(!text.contains("The letter body is not in this prompt"));
    }

    #[tokio::test]
    async fn first_delivery_truncates_oversized_body() {
        let (db, source, target, _) = seeded_memory().await;
        let body = "x".repeat(MAX_FIRST_DELIVERY_BODY_CHARS + 40);
        let sent = send(&db.conn, input(source, vec![target], "huge-letter", &body))
            .await
            .unwrap();
        let draft = prompt_draft_for_origin(&db.conn, target, &sent.event_id)
            .await
            .expect("truncated first delivery must still be injectable");
        let PromptInputBlock::Text { text } = &draft.blocks[0] else {
            panic!("expected a text envelope");
        };
        assert!(text.contains("Body truncated"));
        assert!(text.contains(&"x".repeat(MAX_FIRST_DELIVERY_BODY_CHARS)));
        assert!(!text.contains(&body));
        assert!(text.contains("Call read_message"));
    }

    #[tokio::test]
    async fn first_delivery_truncates_oversized_room_mention_and_points_at_read_room_post() {
        let (db, source, target, _) = seeded_memory().await;
        let room = seeded_room(&db, source, target).await;
        let body = "x".repeat(MAX_FIRST_DELIVERY_BODY_CHARS + 40);
        let mut post = room_post(&room.id, source, vec![target], "huge-room-mention");
        post.body = body.clone();
        let posted = post_room(&db.conn, post).await.unwrap();
        let draft = prompt_draft_for_origin(&db.conn, target, &posted.event_id)
            .await
            .expect("truncated room mention must still be injectable");
        let PromptInputBlock::Text { text } = &draft.blocks[0] else {
            panic!("expected a text envelope");
        };
        assert!(text.contains("Body truncated"));
        assert!(text.contains("Call read_room_post"));
        assert!(text.contains(&format!("offset={MAX_FIRST_DELIVERY_BODY_CHARS}")));
        assert!(text.contains("Call read_room"));
        assert!(text.contains(&"x".repeat(MAX_FIRST_DELIVERY_BODY_CHARS)));
        assert!(!text.contains(&body));
    }
}
