use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, QueryResult, Statement,
    TransactionTrait,
};

use crate::db::error::DbError;
use crate::models::{
    AddCollaborationRoomMembersInput, CollaborationRoomDetail, CollaborationRoomMember,
    CollaborationRoomSummary, CollaborationSessionSnapshot, CollaborationUrgency,
    CreateCollaborationRoomInput, RoomAdditionalPath, RoomTimeline, RoomTimelineEvent,
};

const MAX_TITLE_CHARS: usize = 80;
const MAX_MEMBERS: usize = 32;
/// A Room's `@`-search roots are scanned under one shared wall-clock deadline
/// (`WORKSPACE_SCAN_DEADLINE` in `commands::folders`) and one shared entry
/// budget — piling on additional paths only dilutes both, it never buys more
/// of either. This caps the list at a size a human is going to manage by hand
/// through the "manage paths" dialog anyway.
const MAX_ADDITIONAL_PATHS: usize = 20;
const DEFAULT_TIMELINE_LIMIT: u32 = 200;
const MAX_TIMELINE_LIMIT: u32 = 500;

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

fn normalize_title(title: &str) -> Result<String, DbError> {
    let trimmed = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.is_empty() {
        return Err(validation("A Room title cannot be empty"));
    }
    if trimmed.chars().count() > MAX_TITLE_CHARS {
        return Err(validation(format!(
            "A Room title must be at most {MAX_TITLE_CHARS} characters"
        )));
    }
    Ok(trimmed)
}

async fn require_workbench<C: ConnectionTrait>(conn: &C, workbench_id: i32) -> Result<(), DbError> {
    let row = conn
        .query_one(statement(
            "SELECT id FROM workbench WHERE id = ?",
            vec![workbench_id.into()],
        ))
        .await?;
    if row.is_none() {
        return Err(DbError::NotFound(format!("Workbench {workbench_id}")));
    }
    Ok(())
}

async fn require_live_session<C: ConnectionTrait>(
    conn: &C,
    conversation_id: i32,
) -> Result<(Option<String>, String), DbError> {
    let row = conn
        .query_one(statement(
            "SELECT title, agent_type FROM conversation \
             WHERE id = ? AND deleted_at IS NULL",
            vec![conversation_id.into()],
        ))
        .await?
        .ok_or_else(|| DbError::NotFound(format!("Conversation {conversation_id}")))?;
    Ok((row.try_get("", "title")?, row.try_get("", "agent_type")?))
}

pub(crate) async fn require_member<C: ConnectionTrait>(
    conn: &C,
    room_id: &str,
    conversation_id: i32,
) -> Result<(), DbError> {
    let row = conn
        .query_one(statement(
            "SELECT conversation_id FROM collaboration_room_member \
             WHERE room_id = ? AND conversation_id = ?",
            vec![room_id.into(), conversation_id.into()],
        ))
        .await?;
    if row.is_none() {
        return Err(validation(format!(
            "Session {conversation_id} is not a member of Room {room_id}"
        )));
    }
    Ok(())
}

pub(crate) async fn member_ids<C: ConnectionTrait>(
    conn: &C,
    room_id: &str,
) -> Result<Vec<i32>, DbError> {
    let rows = conn
        .query_all(statement(
            "SELECT conversation_id FROM collaboration_room_member \
             WHERE room_id = ? ORDER BY conversation_id",
            vec![room_id.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| row.try_get("", "conversation_id").map_err(DbError::from))
        .collect()
}

pub(crate) async fn created_by_conversation_id<C: ConnectionTrait>(
    conn: &C,
    room_id: &str,
) -> Result<i32, DbError> {
    let row = conn
        .query_one(statement(
            "SELECT created_by_conversation_id FROM collaboration_room \
             WHERE id = ? AND status = 'active'",
            vec![room_id.into()],
        ))
        .await?
        .ok_or_else(|| DbError::NotFound(format!("Room {room_id}")))?;
    row.try_get("", "created_by_conversation_id")
        .map_err(DbError::from)
}

async fn collection_for_session<C: ConnectionTrait>(
    conn: &C,
    conversation_id: i32,
) -> Result<Option<i32>, DbError> {
    let row = conn
        .query_one(statement(
            "SELECT collection_id FROM collection_conversation WHERE conversation_id = ?",
            vec![conversation_id.into()],
        ))
        .await?;
    match row {
        Some(row) => Ok(Some(row.try_get("", "collection_id")?)),
        None => Ok(None),
    }
}

async fn collection_root_folder<C: ConnectionTrait>(
    conn: &C,
    collection_id: i32,
) -> Result<(i32, Option<i32>), DbError> {
    let row = conn
        .query_one(statement(
            "SELECT id, root_folder_id FROM collection WHERE id = ?",
            vec![collection_id.into()],
        ))
        .await?
        .ok_or_else(|| DbError::NotFound(format!("Collection {collection_id}")))?;
    Ok((row.try_get("", "id")?, row.try_get("", "root_folder_id")?))
}

/// Canonical execution Path of a Session: the Folder it runs in, flattened to
/// the repository root a worktree already points at through `folder.parent_id`
/// — the same derivation `collection_service` uses to decide which Path owns a
/// Collection. `chat` scratch folders own no Path (the sidebar tree skips them)
/// and a soft-deleted Folder is no longer a tree root, so both resolve to
/// `None`.
async fn session_root_folder<C: ConnectionTrait>(
    conn: &C,
    conversation_id: i32,
) -> Result<Option<i32>, DbError> {
    let row = conn
        .query_one(statement(
            "SELECT COALESCE(f.parent_id, f.id) AS root_folder_id \
             FROM conversation c \
             JOIN folder f ON f.id = c.folder_id \
             WHERE c.id = ? AND f.kind <> 'chat' AND f.deleted_at IS NULL",
            vec![conversation_id.into()],
        ))
        .await?;
    match row {
        Some(row) => Ok(Some(row.try_get::<i32>("", "root_folder_id")?)),
        None => Ok(None),
    }
}

async fn shared_collection_among<C: ConnectionTrait>(
    conn: &C,
    conversation_ids: &[i32],
) -> Result<Option<i32>, DbError> {
    if conversation_ids.is_empty() {
        return Ok(None);
    }
    let mut shared: Option<i32> = None;
    for id in conversation_ids {
        match collection_for_session(conn, *id).await? {
            Some(collection_id) => match shared {
                None => shared = Some(collection_id),
                Some(existing) if existing != collection_id => return Ok(None),
                Some(_) => {}
            },
            None => return Ok(None),
        }
    }
    Ok(shared)
}

async fn resolve_placement<C: ConnectionTrait>(
    conn: &C,
    input: &CreateCollaborationRoomInput,
    member_ids: &[i32],
) -> Result<(Option<i32>, Option<i32>), DbError> {
    if let Some(collection_id) = input.collection_id {
        let (_, root_folder_id) = collection_root_folder(conn, collection_id).await?;
        return Ok((Some(collection_id), root_folder_id.or(input.root_folder_id)));
    }
    if let Some(root_folder_id) = input.root_folder_id {
        return Ok((None, Some(root_folder_id)));
    }
    if let Some(collection_id) = shared_collection_among(conn, member_ids).await? {
        let (_, root_folder_id) = collection_root_folder(conn, collection_id).await?;
        return Ok((Some(collection_id), root_folder_id));
    }
    // Last resort, and the reason it exists: a Room with neither a Collection
    // nor a Path sits in no sidebar bucket at all and is unreachable from the
    // tree. Agent-created Rooms (`room.create` over MCP / Host Control) pass no
    // placement and their members rarely share one Collection, so before this
    // fallback every one of them landed invisible. The Room still does NOT
    // inherit the creator's Collection — that is a taxonomy choice only the
    // members can imply — but it does adopt the creator's Path, which is merely
    // where the Session that opened the Room runs. A creator with no Path
    // (chat-mode scratch folder) still yields NULL; the sidebar's orphan bucket
    // catches that remainder.
    Ok((
        None,
        session_root_folder(conn, input.created_by_conversation_id).await?,
    ))
}

pub(crate) async fn room_workbench_id<C: ConnectionTrait>(
    conn: &C,
    room_id: &str,
) -> Result<i32, DbError> {
    let row = conn
        .query_one(statement(
            "SELECT workbench_id FROM collaboration_room WHERE id = ? AND status = 'active'",
            vec![room_id.into()],
        ))
        .await?
        .ok_or_else(|| DbError::NotFound(format!("Room {room_id}")))?;
    row.try_get("", "workbench_id").map_err(DbError::from)
}

async fn touch_room(txn: &DatabaseTransaction, room_id: &str) -> Result<(), DbError> {
    txn.execute(statement(
        "UPDATE collaboration_room SET updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        vec![room_id.into()],
    ))
    .await?;
    Ok(())
}

fn unique_member_ids(ids: &[i32]) -> Result<Vec<i32>, DbError> {
    let unique: BTreeSet<i32> = ids.iter().copied().collect();
    if unique.is_empty() {
        return Err(validation("A Room requires at least one Session member"));
    }
    if unique.len() > MAX_MEMBERS {
        return Err(validation(format!(
            "A Room supports at most {MAX_MEMBERS} members"
        )));
    }
    Ok(unique.into_iter().collect())
}

pub async fn create(
    conn: &DatabaseConnection,
    input: CreateCollaborationRoomInput,
) -> Result<CollaborationRoomDetail, DbError> {
    let title = normalize_title(&input.title)?;
    require_workbench(conn, input.workbench_id).await?;
    let mut member_ids = unique_member_ids(&input.member_conversation_ids)?;
    if !member_ids.contains(&input.created_by_conversation_id) {
        if member_ids.len() >= MAX_MEMBERS {
            return Err(validation(format!(
                "A Room supports at most {MAX_MEMBERS} members"
            )));
        }
        member_ids.push(input.created_by_conversation_id);
        member_ids.sort_unstable();
    }
    require_live_session(conn, input.created_by_conversation_id).await?;
    for conversation_id in &member_ids {
        require_live_session(conn, *conversation_id).await?;
    }
    let (collection_id, root_folder_id) = resolve_placement(conn, &input, &member_ids).await?;

    let room_id = format!("rm_{}", uuid::Uuid::new_v4());
    let txn = conn.begin().await?;
    txn.execute(statement(
        "INSERT INTO collaboration_room \
         (id, workbench_id, title, status, created_by_conversation_id, \
          collection_id, root_folder_id, created_at, updated_at) \
         VALUES (?, ?, ?, 'active', ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        vec![
            room_id.clone().into(),
            input.workbench_id.into(),
            title.into(),
            input.created_by_conversation_id.into(),
            collection_id.into(),
            root_folder_id.into(),
        ],
    ))
    .await?;
    for conversation_id in member_ids {
        txn.execute(statement(
            "INSERT INTO collaboration_room_member \
             (room_id, conversation_id, role, joined_at, last_read_at) \
             VALUES (?, ?, 'member', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            vec![room_id.clone().into(), conversation_id.into()],
        ))
        .await?;
    }
    txn.commit().await?;
    get(conn, &room_id).await
}

/// Host / Workbench lists: channel unread is whoever last opened the Room
/// panel (`collaboration_room.last_seen_at`). That is the operator cursor,
/// not a Session mailbox.
const HOST_CHANNEL_UNREAD_SQL: &str = "(SELECT COUNT(*) FROM collaboration_event e \
                      WHERE e.room_id = r.id AND COALESCE(e.visibility, 'direct') = 'room' \
                        AND (r.last_seen_at IS NULL OR datetime(e.created_at) > datetime(r.last_seen_at)))";
/// Agent `list_rooms`: posts after this member's own cursor, including
/// record-only posts that never created a Delivery. "My own post" is an
/// authorship test, not an id test: a human post borrows the Room creator's
/// Session id (see `collaboration_service::post_room`), and the creator has
/// to see it as unread exactly like every other member does.
const MEMBER_CHANNEL_UNREAD_SQL: &str = "(SELECT COUNT(*) FROM collaboration_event e \
                      WHERE e.room_id = r.id AND COALESCE(e.visibility, 'direct') = 'room' \
                        AND NOT (e.source_conversation_id = me.conversation_id \
                                 AND COALESCE(e.author_kind, 'session') = 'session') \
                        AND (
                          (
                            me.last_read_event_id IS NOT NULL
                            AND (
                              datetime(e.created_at) > (
                                SELECT datetime(cur.created_at) FROM collaboration_event cur
                                 WHERE cur.id = me.last_read_event_id
                              )
                              OR (
                                datetime(e.created_at) = (
                                  SELECT datetime(cur.created_at) FROM collaboration_event cur
                                   WHERE cur.id = me.last_read_event_id
                                )
                                AND e.rowid > (
                                  SELECT cur.rowid FROM collaboration_event cur
                                   WHERE cur.id = me.last_read_event_id
                                )
                              )
                            )
                          )
                          OR (
                            me.last_read_event_id IS NULL
                            AND datetime(e.created_at) >= datetime(COALESCE(me.last_read_at, me.joined_at))
                          )
                        ))";
/// Room posts that `@`-mentioned the user, since the operator cursor. The
/// human has no Delivery row of their own — `mention_human` is a flag on the
/// event, not a fan-out target — so this counts events against
/// `r.last_seen_at` exactly like `HOST_CHANNEL_UNREAD_SQL`, of which it is a
/// subset. That is why it cannot reuse the member shape below.
const HOST_MENTION_UNREAD_SQL: &str = "(SELECT COUNT(*) FROM collaboration_event e \
                      WHERE e.room_id = r.id AND COALESCE(e.visibility, 'direct') = 'room' \
                        AND COALESCE(e.mention_human, 0) = 1 \
                        AND (r.last_seen_at IS NULL OR datetime(e.created_at) > datetime(r.last_seen_at)))";
/// The operator watches the whole Room, so the reply ledger is not scoped to
/// one member: this is every obligation still outstanding in the Room
/// ("somebody in here owes an answer"), which is the sum of
/// `MEMBER_NEEDS_REPLY_SQL` over its members. A deleted Session can never pay
/// its debt back, so its rows drop out — otherwise the badge would stay lit
/// with nothing left to open. Archived members are kept, matching direct mail
/// in `collaboration_service::unread_overview`.
const HOST_NEEDS_REPLY_SQL: &str = "(SELECT COUNT(*) FROM collaboration_delivery d \
                      JOIN collaboration_event e ON e.id = d.event_id \
                      WHERE e.room_id = r.id \
                        AND COALESCE(e.visibility, 'direct') = 'room' \
                        AND d.obligation_state = 'awaiting_reply' \
                        AND d.state <> 'dismissed' AND d.state <> 'failed' \
                        AND EXISTS (SELECT 1 FROM conversation c \
                                     WHERE c.id = d.target_conversation_id \
                                       AND c.deleted_at IS NULL))";
/// Direct mail splits the ledger into obligations owed *to* the viewer
/// (`needs_reply`) and obligations the viewer's own messages created
/// (`awaiting_reply`). The host posts through the Room UI, which stamps
/// `author_kind = 'human'`, so the same split here means "the user asked and
/// nobody has answered yet". It is a subset of `HOST_NEEDS_REPLY_SQL`, not a
/// second copy of it — a Room-wide sender-side count would re-count the very
/// same Delivery rows.
const HOST_AWAITING_REPLY_SQL: &str = "(SELECT COUNT(*) FROM collaboration_delivery d \
                      JOIN collaboration_event e ON e.id = d.event_id \
                      WHERE e.room_id = r.id \
                        AND COALESCE(e.visibility, 'direct') = 'room' \
                        AND COALESCE(e.author_kind, 'session') = 'human' \
                        AND d.obligation_state = 'awaiting_reply' \
                        AND d.state <> 'dismissed' AND d.state <> 'failed' \
                        AND EXISTS (SELECT 1 FROM conversation c \
                                     WHERE c.id = d.target_conversation_id \
                                       AND c.deleted_at IS NULL))";
const MEMBER_MENTION_UNREAD_SQL: &str = "(SELECT COUNT(*) FROM collaboration_delivery d \
                      JOIN collaboration_event e ON e.id = d.event_id \
                      WHERE d.target_conversation_id = me.conversation_id \
                        AND e.room_id = r.id \
                        AND COALESCE(e.visibility, 'direct') = 'room' \
                        AND d.agent_received_at IS NULL \
                        AND d.state <> 'dismissed' AND d.state <> 'failed')";
const MEMBER_NEEDS_REPLY_SQL: &str = "(SELECT COUNT(*) FROM collaboration_delivery d \
                      JOIN collaboration_event e ON e.id = d.event_id \
                      WHERE d.target_conversation_id = me.conversation_id \
                        AND e.room_id = r.id \
                        AND COALESCE(e.visibility, 'direct') = 'room' \
                        AND d.obligation_state = 'awaiting_reply' \
                        AND d.state <> 'dismissed' AND d.state <> 'failed')";
/// Obligations this member's own asks created. Human posts are excluded even
/// though they carry the creator's Session id: the user asked, not the
/// creator, and `HOST_AWAITING_REPLY_SQL` is where that ask is counted.
/// Without the authorship test a human `@` of the creator would show up twice
/// on the creator's own card — once as a debt owed, once as a debt awaited —
/// off a single Delivery row.
const MEMBER_AWAITING_REPLY_SQL: &str = "(SELECT COUNT(*) FROM collaboration_delivery d \
                      JOIN collaboration_event e ON e.id = d.event_id \
                      WHERE e.room_id = r.id \
                        AND e.source_conversation_id = me.conversation_id \
                        AND COALESCE(e.author_kind, 'session') = 'session' \
                        AND COALESCE(e.visibility, 'direct') = 'room' \
                        AND d.obligation_state = 'awaiting_reply' \
                        AND d.state <> 'dismissed' AND d.state <> 'failed')";

fn room_summary_select(
    unread_sql: &str,
    mention_unread_sql: &str,
    needs_reply_sql: &str,
    awaiting_reply_sql: &str,
) -> String {
    format!(
        "SELECT r.id, r.workbench_id, r.title, r.created_by_conversation_id, \
                r.collection_id, r.root_folder_id, \
                r.created_at, r.updated_at, r.last_seen_at, \
                (SELECT COUNT(*) FROM collaboration_room_member m \
                  WHERE m.room_id = r.id) AS member_count, \
                (SELECT COUNT(*) FROM collaboration_room_path p \
                  WHERE p.room_id = r.id) AS additional_path_count, \
                (SELECT MAX(e.created_at) FROM collaboration_event e \
                  WHERE e.room_id = r.id AND COALESCE(e.visibility, 'direct') = 'room') \
                  AS last_event_at, \
                {unread_sql} AS unread_count, \
                {mention_unread_sql} AS mention_unread_count, \
                {needs_reply_sql} AS needs_reply_count, \
                {awaiting_reply_sql} AS awaiting_reply_count \
         FROM collaboration_room r"
    )
}

/// Rooms on one Workbench. Host Control `room.list` / `room.list_workbench`
/// and the Rooms page use this. Agents that only need Rooms they joined
/// should call `list_for_member` instead.
pub async fn list_for_workbench(
    conn: &DatabaseConnection,
    workbench_id: i32,
) -> Result<Vec<CollaborationRoomSummary>, DbError> {
    require_workbench(conn, workbench_id).await?;
    let rows = conn
        .query_all(statement(
            &format!(
                "{} \
             WHERE r.workbench_id = ? AND r.status = 'active' \
             ORDER BY datetime(COALESCE(last_event_at, r.updated_at)) DESC, r.id DESC",
                room_summary_select(
                    HOST_CHANNEL_UNREAD_SQL,
                    HOST_MENTION_UNREAD_SQL,
                    HOST_NEEDS_REPLY_SQL,
                    HOST_AWAITING_REPLY_SQL,
                )
            ),
            vec![workbench_id.into()],
        ))
        .await?;
    rows.iter().map(summary_from_row).collect()
}

/// Every active Room in the install, whatever Workbench it lives on, with the
/// same host counts and the same most-recent-activity order as
/// `list_for_workbench`. The Session Center lists Rooms beside Sessions and has
/// no Workbench to scope by, so it reads this instead of fanning out one
/// `list_for_workbench` per Workbench.
///
/// `search` narrows by title with the same `LIKE '%q%'` shape
/// `conversation_service::list_all` gives the Session title search, so one
/// query typed in the Session Center means the same thing on both lanes —
/// SQLite's ASCII-insensitive `LIKE`, and a `%` the user types is a wildcard
/// there exactly as it is here.
pub async fn list_all_for_host(
    conn: &DatabaseConnection,
    search: Option<&str>,
) -> Result<Vec<CollaborationRoomSummary>, DbError> {
    let mut sql = format!(
        "{} WHERE r.status = 'active'",
        room_summary_select(
            HOST_CHANNEL_UNREAD_SQL,
            HOST_MENTION_UNREAD_SQL,
            HOST_NEEDS_REPLY_SQL,
            HOST_AWAITING_REPLY_SQL,
        )
    );
    let mut values: Vec<sea_orm::Value> = Vec::new();
    if let Some(needle) = search.map(str::trim).filter(|s| !s.is_empty()) {
        sql.push_str(" AND r.title LIKE ?");
        values.push(format!("%{needle}%").into());
    }
    sql.push_str(" ORDER BY datetime(COALESCE(last_event_at, r.updated_at)) DESC, r.id DESC");
    let rows = conn.query_all(statement(&sql, values)).await?;
    rows.iter().map(summary_from_row).collect()
}

/// Room-side counters for the collaboration unread overview, summed over every
/// active Room on every Workbench.
#[derive(Debug, Clone, Copy)]
pub struct RoomHostTotals {
    pub unread_count: u32,
    pub needs_reply_count: u32,
}

/// Host-view Room debt for the whole install. Deliberately built by summing the
/// very expressions `list_for_workbench` shows per Room, so the sidebar total
/// can never drift from the Rooms the user opens to clear it.
pub async fn host_totals(conn: &DatabaseConnection) -> Result<RoomHostTotals, DbError> {
    let row = conn
        .query_one(statement(
            &format!(
                "SELECT COALESCE(SUM({HOST_CHANNEL_UNREAD_SQL}), 0) AS unread_count, \
                        COALESCE(SUM({HOST_NEEDS_REPLY_SQL}), 0) AS needs_reply_count \
                 FROM collaboration_room r WHERE r.status = 'active'"
            ),
            vec![],
        ))
        .await?
        .ok_or_else(|| validation("Could not count outstanding Room replies"))?;
    let count = |column| -> Result<u32, DbError> {
        let raw: i64 = row.try_get("", column)?;
        Ok(u32::try_from(raw.max(0)).unwrap_or(u32::MAX))
    };
    Ok(RoomHostTotals {
        unread_count: count("unread_count")?,
        needs_reply_count: count("needs_reply_count")?,
    })
}

/// Rooms the Session already belongs to, across Workbenches. Agents use this
/// instead of listing every Room on Workbench 1.
pub async fn list_for_member(
    conn: &DatabaseConnection,
    conversation_id: i32,
) -> Result<Vec<CollaborationRoomSummary>, DbError> {
    require_live_session(conn, conversation_id).await?;
    let rows = conn
        .query_all(statement(
            &format!(
                "{} \
             JOIN collaboration_room_member me \
               ON me.room_id = r.id AND me.conversation_id = ? \
             WHERE r.status = 'active' \
             ORDER BY datetime(COALESCE(last_event_at, r.updated_at)) DESC, r.id DESC",
                room_summary_select(
                    MEMBER_CHANNEL_UNREAD_SQL,
                    MEMBER_MENTION_UNREAD_SQL,
                    MEMBER_NEEDS_REPLY_SQL,
                    MEMBER_AWAITING_REPLY_SQL,
                )
            ),
            vec![conversation_id.into()],
        ))
        .await?;
    rows.iter().map(summary_from_row).collect()
}

fn summary_from_row(row: &QueryResult) -> Result<CollaborationRoomSummary, DbError> {
    let member_count: i64 = row.try_get("", "member_count")?;
    let additional_path_count: i64 = row.try_get("", "additional_path_count")?;
    let unread_count: i64 = row.try_get("", "unread_count")?;
    let mention_unread_count: i64 = row.try_get("", "mention_unread_count")?;
    let needs_reply_count: i64 = row.try_get("", "needs_reply_count")?;
    let awaiting_reply_count: i64 = row.try_get("", "awaiting_reply_count")?;
    Ok(CollaborationRoomSummary {
        id: row.try_get("", "id")?,
        workbench_id: row.try_get("", "workbench_id")?,
        title: row.try_get("", "title")?,
        created_by_conversation_id: row.try_get("", "created_by_conversation_id")?,
        collection_id: row.try_get("", "collection_id")?,
        root_folder_id: row.try_get("", "root_folder_id")?,
        member_count: u32::try_from(member_count.max(0)).unwrap_or(u32::MAX),
        additional_path_count: u32::try_from(additional_path_count.max(0)).unwrap_or(u32::MAX),
        unread_count: u32::try_from(unread_count.max(0)).unwrap_or(u32::MAX),
        mention_unread_count: u32::try_from(mention_unread_count.max(0)).unwrap_or(u32::MAX),
        needs_reply_count: u32::try_from(needs_reply_count.max(0)).unwrap_or(u32::MAX),
        awaiting_reply_count: u32::try_from(awaiting_reply_count.max(0)).unwrap_or(u32::MAX),
        last_event_at: parse_optional_timestamp(row, "last_event_at")?,
        created_at: parse_timestamp(row, "created_at")?,
        updated_at: parse_timestamp(row, "updated_at")?,
    })
}

pub async fn get(
    conn: &DatabaseConnection,
    room_id: &str,
) -> Result<CollaborationRoomDetail, DbError> {
    let row = conn
        .query_one(statement(
            "SELECT id, workbench_id, title, created_by_conversation_id, \
                    collection_id, root_folder_id, created_at, updated_at \
             FROM collaboration_room WHERE id = ? AND status = 'active'",
            vec![room_id.into()],
        ))
        .await?
        .ok_or_else(|| DbError::NotFound(format!("Room {room_id}")))?;
    let members = list_members(conn, room_id).await?;
    let additional_paths = list_paths(conn, room_id).await?;
    Ok(CollaborationRoomDetail {
        id: row.try_get("", "id")?,
        workbench_id: row.try_get("", "workbench_id")?,
        title: row.try_get("", "title")?,
        created_by_conversation_id: row.try_get("", "created_by_conversation_id")?,
        collection_id: row.try_get("", "collection_id")?,
        root_folder_id: row.try_get("", "root_folder_id")?,
        members,
        additional_paths,
        created_at: parse_timestamp(&row, "created_at")?,
        updated_at: parse_timestamp(&row, "updated_at")?,
    })
}

async fn list_members<C: ConnectionTrait>(
    conn: &C,
    room_id: &str,
) -> Result<Vec<CollaborationRoomMember>, DbError> {
    let rows = conn
        .query_all(statement(
            "SELECT m.conversation_id, m.role, m.joined_at, m.last_read_at, \
                    c.title, c.agent_type \
             FROM collaboration_room_member m \
             LEFT JOIN conversation c ON c.id = m.conversation_id \
             WHERE m.room_id = ? \
             ORDER BY m.conversation_id",
            vec![room_id.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| {
            Ok(CollaborationRoomMember {
                conversation_id: row.try_get("", "conversation_id")?,
                title: row.try_get("", "title")?,
                agent_type: row.try_get("", "agent_type")?,
                role: row.try_get("", "role")?,
                joined_at: parse_timestamp(row, "joined_at")?,
                last_read_at: parse_optional_timestamp(row, "last_read_at")?,
            })
        })
        .collect()
}

async fn list_paths<C: ConnectionTrait>(
    conn: &C,
    room_id: &str,
) -> Result<Vec<RoomAdditionalPath>, DbError> {
    let rows = conn
        .query_all(statement(
            "SELECT id, path, created_at \
             FROM collaboration_room_path \
             WHERE room_id = ? \
             ORDER BY id",
            vec![room_id.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| {
            Ok(RoomAdditionalPath {
                id: row.try_get("", "id")?,
                path: row.try_get("", "path")?,
                created_at: parse_timestamp(row, "created_at")?,
            })
        })
        .collect()
}

pub async fn add_members(
    conn: &DatabaseConnection,
    input: AddCollaborationRoomMembersInput,
) -> Result<CollaborationRoomDetail, DbError> {
    let _ = room_workbench_id(conn, &input.room_id).await?;
    let existing: BTreeSet<i32> = member_ids(conn, &input.room_id)
        .await?
        .into_iter()
        .collect();
    let incoming: BTreeSet<i32> = input.conversation_ids.iter().copied().collect();
    if incoming.is_empty() {
        return Err(validation("Add at least one Session member"));
    }
    if existing.len() + incoming.difference(&existing).count() > MAX_MEMBERS {
        return Err(validation(format!(
            "A Room supports at most {MAX_MEMBERS} members"
        )));
    }
    let txn = conn.begin().await?;
    for conversation_id in incoming {
        if existing.contains(&conversation_id) {
            continue;
        }
        require_live_session(&txn, conversation_id).await?;
        txn.execute(statement(
            "INSERT OR IGNORE INTO collaboration_room_member \
             (room_id, conversation_id, role, joined_at, last_read_at, last_read_event_id) \
             VALUES (?, ?, 'member', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, \
               (SELECT e.id FROM collaboration_event e \
                 WHERE e.room_id = ? AND COALESCE(e.visibility, 'direct') = 'room' \
                 ORDER BY datetime(e.created_at) DESC, e.rowid DESC LIMIT 1))",
            vec![
                input.room_id.clone().into(),
                conversation_id.into(),
                input.room_id.clone().into(),
            ],
        ))
        .await?;
    }
    touch_room(&txn, &input.room_id).await?;
    txn.commit().await?;
    get(conn, &input.room_id).await
}

pub async fn remove_member(
    conn: &DatabaseConnection,
    room_id: &str,
    conversation_id: i32,
) -> Result<CollaborationRoomDetail, DbError> {
    let _ = room_workbench_id(conn, room_id).await?;
    conn.query_one(statement(
        "SELECT conversation_id FROM collaboration_room_member \
         WHERE room_id = ? AND conversation_id = ?",
        vec![room_id.into(), conversation_id.into()],
    ))
    .await?
    .ok_or_else(|| {
        DbError::NotFound(format!(
            "Session {conversation_id} is not a member of Room {room_id}"
        ))
    })?;
    let remaining: i64 = conn
        .query_one(statement(
            "SELECT COUNT(*) AS count FROM collaboration_room_member WHERE room_id = ?",
            vec![room_id.into()],
        ))
        .await?
        .expect("COUNT")
        .try_get("", "count")?;
    if remaining <= 1 {
        return Err(validation("A Room requires at least one Session member"));
    }
    let txn = conn.begin().await?;
    txn.execute(statement(
        "DELETE FROM collaboration_room_member WHERE room_id = ? AND conversation_id = ?",
        vec![room_id.into(), conversation_id.into()],
    ))
    .await?;
    touch_room(&txn, room_id).await?;
    txn.commit().await?;
    get(conn, room_id).await
}

/// Adds an extra `@`-search path to a Room, on top of its bound
/// `root_folder_id`. Unlike a Folder (opened through a picker that already
/// guarantees this), an additional path is user-typed free text, so it is
/// validated here — before it is written — to exist and be a directory.
/// Adding a path already on the room is a silent no-op (`INSERT OR IGNORE`
/// against the `UNIQUE(room_id, path)` constraint), matching `add_members`'s
/// "already a member" tolerance rather than erroring.
pub async fn add_path(
    conn: &DatabaseConnection,
    room_id: &str,
    path: &str,
) -> Result<CollaborationRoomDetail, DbError> {
    let _ = room_workbench_id(conn, room_id).await?;
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(validation("Path cannot be empty"));
    }
    match std::fs::metadata(trimmed) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Err(validation(format!("Path is not a directory: {trimmed}"))),
        Err(_) => return Err(validation(format!("Path does not exist: {trimmed}"))),
    }
    let existing: i64 = conn
        .query_one(statement(
            "SELECT COUNT(*) AS count FROM collaboration_room_path WHERE room_id = ?",
            vec![room_id.into()],
        ))
        .await?
        .expect("COUNT")
        .try_get("", "count")?;
    if existing >= MAX_ADDITIONAL_PATHS as i64 {
        return Err(validation(format!(
            "A Room supports at most {MAX_ADDITIONAL_PATHS} additional paths"
        )));
    }
    let txn = conn.begin().await?;
    txn.execute(statement(
        "INSERT OR IGNORE INTO collaboration_room_path (room_id, path, created_at) \
         VALUES (?, ?, CURRENT_TIMESTAMP)",
        vec![room_id.into(), trimmed.into()],
    ))
    .await?;
    touch_room(&txn, room_id).await?;
    txn.commit().await?;
    get(conn, room_id).await
}

/// Removes an extra `@`-search path from a Room by its row id.
pub async fn remove_path(
    conn: &DatabaseConnection,
    room_id: &str,
    path_id: i32,
) -> Result<CollaborationRoomDetail, DbError> {
    let _ = room_workbench_id(conn, room_id).await?;
    let txn = conn.begin().await?;
    let changed = txn
        .execute(statement(
            "DELETE FROM collaboration_room_path WHERE id = ? AND room_id = ?",
            vec![path_id.into(), room_id.into()],
        ))
        .await?
        .rows_affected();
    if changed == 0 {
        return Err(DbError::NotFound(format!(
            "Path {path_id} not found on Room {room_id}"
        )));
    }
    touch_room(&txn, room_id).await?;
    txn.commit().await?;
    get(conn, room_id).await
}

/// Reassign a Room to another Workbench. Same-workbench is a no-op (not an
/// error). Archived / missing Rooms are rejected. Members are not rewritten:
/// membership is Session-scoped, independent of which Workbench the Room sits
/// on — changing that coupling is a product call, not this function's job.
pub async fn set_workbench(
    conn: &DatabaseConnection,
    room_id: &str,
    workbench_id: i32,
) -> Result<CollaborationRoomDetail, DbError> {
    require_workbench(conn, workbench_id).await?;
    let row = conn
        .query_one(statement(
            "SELECT workbench_id, status FROM collaboration_room WHERE id = ?",
            vec![room_id.into()],
        ))
        .await?
        .ok_or_else(|| DbError::NotFound(format!("Room {room_id}")))?;
    let status: String = row.try_get("", "status")?;
    if status != "active" {
        return Err(validation(format!("Room {room_id} is not active")));
    }
    let current: i32 = row.try_get("", "workbench_id")?;
    if current == workbench_id {
        return get(conn, room_id).await;
    }
    let changed = conn
        .execute(statement(
            "UPDATE collaboration_room \
             SET workbench_id = ?, updated_at = CURRENT_TIMESTAMP \
             WHERE id = ? AND status = 'active'",
            vec![workbench_id.into(), room_id.into()],
        ))
        .await?
        .rows_affected();
    if changed == 0 {
        return Err(DbError::NotFound(format!("Room {room_id}")));
    }
    get(conn, room_id).await
}

pub async fn rename(
    conn: &DatabaseConnection,
    room_id: &str,
    title: &str,
) -> Result<CollaborationRoomDetail, DbError> {
    let title = normalize_title(title)?;
    let _ = room_workbench_id(conn, room_id).await?;
    let changed = conn
        .execute(statement(
            "UPDATE collaboration_room \
             SET title = ?, updated_at = CURRENT_TIMESTAMP \
             WHERE id = ? AND status = 'active'",
            vec![title.into(), room_id.into()],
        ))
        .await?
        .rows_affected();
    if changed == 0 {
        return Err(DbError::NotFound(format!("Room {room_id}")));
    }
    get(conn, room_id).await
}

pub async fn delete(conn: &DatabaseConnection, room_id: &str) -> Result<i32, DbError> {
    let workbench_id = room_workbench_id(conn, room_id).await?;
    let txn = conn.begin().await?;
    txn.execute(statement(
        "DELETE FROM conversation_prompt_queue_item \
         WHERE origin_event_id IN ( \
             SELECT id FROM collaboration_event \
             WHERE room_id = ? AND COALESCE(visibility, 'direct') = 'room' \
         )",
        vec![room_id.into()],
    ))
    .await?;
    txn.execute(statement(
        "UPDATE collaboration_event \
         SET reply_to_event_id = NULL \
         WHERE room_id = ? AND COALESCE(visibility, 'direct') = 'room'",
        vec![room_id.into()],
    ))
    .await?;
    txn.execute(statement(
        "DELETE FROM collaboration_event \
         WHERE room_id = ? AND COALESCE(visibility, 'direct') = 'room'",
        vec![room_id.into()],
    ))
    .await?;
    let removed = txn
        .execute(statement(
            "DELETE FROM collaboration_room WHERE id = ?",
            vec![room_id.into()],
        ))
        .await?
        .rows_affected();
    if removed == 0 {
        return Err(DbError::NotFound(format!("Room {room_id}")));
    }
    txn.commit().await?;
    Ok(workbench_id)
}

pub async fn assign_to_collection(
    conn: &DatabaseConnection,
    room_ids: Vec<String>,
    collection_id: Option<i32>,
    root_folder_id: Option<i32>,
) -> Result<Vec<CollaborationRoomSummary>, DbError> {
    if room_ids.is_empty() {
        return Ok(Vec::new());
    }
    if room_ids.len() > 200 {
        return Err(validation("At most 200 Rooms can be moved at once"));
    }
    let (next_collection, next_root) = if let Some(collection_id) = collection_id {
        let (_, collection_root) = collection_root_folder(conn, collection_id).await?;
        (Some(collection_id), collection_root.or(root_folder_id))
    } else {
        (None, root_folder_id)
    };
    let txn = conn.begin().await?;
    for room_id in &room_ids {
        let _ = room_workbench_id(&txn, room_id).await?;
        txn.execute(statement(
            "UPDATE collaboration_room \
             SET collection_id = ?, root_folder_id = COALESCE(?, root_folder_id), \
                 updated_at = CURRENT_TIMESTAMP \
             WHERE id = ? AND status = 'active'",
            vec![
                next_collection.into(),
                next_root.into(),
                room_id.clone().into(),
            ],
        ))
        .await?;
    }
    txn.commit().await?;
    let mut rooms = Vec::new();
    for room_id in room_ids {
        let detail = get(conn, &room_id).await?;
        rooms.push(CollaborationRoomSummary {
            id: detail.id,
            workbench_id: detail.workbench_id,
            title: detail.title,
            created_by_conversation_id: detail.created_by_conversation_id,
            collection_id: detail.collection_id,
            root_folder_id: detail.root_folder_id,
            member_count: u32::try_from(detail.members.len()).unwrap_or(u32::MAX),
            additional_path_count: u32::try_from(detail.additional_paths.len()).unwrap_or(u32::MAX),
            unread_count: 0,
            mention_unread_count: 0,
            needs_reply_count: 0,
            awaiting_reply_count: 0,
            last_event_at: None,
            created_at: detail.created_at,
            updated_at: detail.updated_at,
        });
    }
    Ok(rooms)
}

pub async fn mark_seen(
    conn: &DatabaseConnection,
    room_id: &str,
    conversation_id: Option<i32>,
) -> Result<CollaborationRoomDetail, DbError> {
    let _ = room_workbench_id(conn, room_id).await?;
    let txn = conn.begin().await?;
    txn.execute(statement(
        "UPDATE collaboration_room \
         SET last_seen_at = CURRENT_TIMESTAMP \
         WHERE id = ?",
        vec![room_id.into()],
    ))
    .await?;
    if let Some(conversation_id) = conversation_id {
        require_member(&txn, room_id, conversation_id).await?;
        txn.execute(statement(
            "UPDATE collaboration_room_member \
             SET last_read_at = CURRENT_TIMESTAMP, \
                 last_read_event_id = (
                    SELECT e.id FROM collaboration_event e \
                     WHERE e.room_id = ? AND COALESCE(e.visibility, 'direct') = 'room' \
                     ORDER BY datetime(e.created_at) DESC, e.rowid DESC LIMIT 1
                 ) \
             WHERE room_id = ? AND conversation_id = ?",
            vec![room_id.into(), room_id.into(), conversation_id.into()],
        ))
        .await?;
    }
    txn.commit().await?;
    get(conn, room_id).await
}

pub enum RoomTimelineMode<'a> {
    /// Newest `limit` posts, returned oldest-first so a panel reads naturally.
    Recent,
    /// Posts after this member's cursor, oldest-first. Catch-up, not search.
    Unread { conversation_id: i32 },
    /// Page older than `event_id` (still a newest-of-the-older window).
    Before { event_id: &'a str },
    /// Unpaid reply obligations that involve this Session: it owes a reply,
    /// or someone still owes it one. Does not advance the read cursor.
    NeedsReply { conversation_id: i32 },
}

const EVENT_SELECT: &str =
    "SELECT e.id, e.room_id, e.source_conversation_id, e.source_title_snapshot, \
                    e.source_agent_type_snapshot, e.source_folder_path_snapshot, \
                    e.source_model_snapshot, e.source_profile_snapshot, \
                    e.subject, e.body, e.reply_to_event_id, e.expects_reply, e.urgency, \
                    COALESCE(e.author_kind, 'session') AS author_kind, \
                    COALESCE(e.mention_human, 0) AS mention_human, \
                    e.created_at, e.rowid AS event_rowid \
             FROM collaboration_event e";

pub async fn timeline(
    conn: &DatabaseConnection,
    room_id: &str,
    limit: Option<u32>,
) -> Result<RoomTimeline, DbError> {
    timeline_with(conn, room_id, limit, RoomTimelineMode::Recent).await
}

pub async fn timeline_with(
    conn: &DatabaseConnection,
    room_id: &str,
    limit: Option<u32>,
    mode: RoomTimelineMode<'_>,
) -> Result<RoomTimeline, DbError> {
    let _ = room_workbench_id(conn, room_id).await?;
    let limit = limit
        .unwrap_or(DEFAULT_TIMELINE_LIMIT)
        .clamp(1, MAX_TIMELINE_LIMIT) as i64;
    let fetch_limit = limit.saturating_add(1);
    let (rows, truncated) = match mode {
        RoomTimelineMode::Recent => {
            newest_window(
                conn,
                &format!(
                    "{EVENT_SELECT} \
                     WHERE e.room_id = ? AND COALESCE(e.visibility, 'direct') = 'room' \
                     ORDER BY datetime(e.created_at) DESC, e.rowid DESC \
                     LIMIT ?"
                ),
                vec![room_id.into(), fetch_limit.into()],
                limit,
            )
            .await?
        }
        RoomTimelineMode::Before { event_id } => {
            newest_window(
                conn,
                &format!(
                    "{EVENT_SELECT} \
                     WHERE e.room_id = ? AND COALESCE(e.visibility, 'direct') = 'room' \
                       AND (
                         datetime(e.created_at) < (
                           SELECT datetime(cur.created_at) FROM collaboration_event cur
                            WHERE cur.id = ? AND cur.room_id = ?
                         )
                         OR (
                           datetime(e.created_at) = (
                             SELECT datetime(cur.created_at) FROM collaboration_event cur
                              WHERE cur.id = ? AND cur.room_id = ?
                           )
                           AND e.rowid < (
                             SELECT cur.rowid FROM collaboration_event cur
                              WHERE cur.id = ? AND cur.room_id = ?
                           )
                         )
                       ) \
                     ORDER BY datetime(e.created_at) DESC, e.rowid DESC \
                     LIMIT ?"
                ),
                vec![
                    room_id.into(),
                    event_id.into(),
                    room_id.into(),
                    event_id.into(),
                    room_id.into(),
                    event_id.into(),
                    room_id.into(),
                    fetch_limit.into(),
                ],
                limit,
            )
            .await?
        }
        // The cursor half of `MEMBER_CHANNEL_UNREAD_SQL`, and it skips "my own
        // posts" on the same authorship test: a human post carries the Room
        // creator's borrowed Session id but is not the creator's own writing.
        RoomTimelineMode::Unread { conversation_id } => {
            require_member(conn, room_id, conversation_id).await?;
            let rows = conn
                .query_all(statement(
                    &format!(
                        "{EVENT_SELECT} \
                     JOIN collaboration_room_member me \
                       ON me.room_id = e.room_id AND me.conversation_id = ? \
                     WHERE e.room_id = ? AND COALESCE(e.visibility, 'direct') = 'room' \
                       AND NOT (
                         e.source_conversation_id = me.conversation_id
                         AND COALESCE(e.author_kind, 'session') = 'session'
                       ) \
                       AND (
                         (
                           me.last_read_event_id IS NOT NULL
                           AND (
                             datetime(e.created_at) > (
                               SELECT datetime(cur.created_at) FROM collaboration_event cur
                                WHERE cur.id = me.last_read_event_id
                             )
                             OR (
                               datetime(e.created_at) = (
                                 SELECT datetime(cur.created_at) FROM collaboration_event cur
                                  WHERE cur.id = me.last_read_event_id
                               )
                               AND e.rowid > (
                                 SELECT cur.rowid FROM collaboration_event cur
                                  WHERE cur.id = me.last_read_event_id
                               )
                             )
                           )
                         )
                         OR (
                           me.last_read_event_id IS NULL
                           AND datetime(e.created_at) >= datetime(COALESCE(me.last_read_at, me.joined_at))
                         )
                       ) \
                     ORDER BY datetime(e.created_at) ASC, e.rowid ASC \
                     LIMIT ?"
                    ),
                    vec![
                        conversation_id.into(),
                        room_id.into(),
                        fetch_limit.into(),
                    ],
                ))
                .await?;
            let truncated = (rows.len() as i64) > limit;
            let rows = if truncated {
                rows.into_iter().take(limit as usize).collect()
            } else {
                rows
            };
            (rows, truncated)
        }
        // Both sides of one obligation: posts this member owes an answer to,
        // plus its own asks nobody has answered. The second half stays keyed
        // to Session authorship so a human post filed under the Room
        // creator's borrowed id is not read back as the creator's own ask —
        // it reaches the creator through the first half instead, when the
        // human actually `@`-ed them.
        RoomTimelineMode::NeedsReply { conversation_id } => {
            require_member(conn, room_id, conversation_id).await?;
            newest_window(
                conn,
                &format!(
                    "{EVENT_SELECT} \
                     WHERE e.room_id = ? AND COALESCE(e.visibility, 'direct') = 'room' \
                       AND (
                         EXISTS (
                           SELECT 1 FROM collaboration_delivery d
                           WHERE d.event_id = e.id
                             AND d.target_conversation_id = ?
                             AND d.obligation_state = 'awaiting_reply'
                             AND d.state NOT IN ('dismissed', 'failed')
                         )
                         OR (
                           e.source_conversation_id = ?
                           AND COALESCE(e.author_kind, 'session') = 'session'
                           AND EXISTS (
                             SELECT 1 FROM collaboration_delivery d
                             WHERE d.event_id = e.id
                               AND d.obligation_state = 'awaiting_reply'
                               AND d.state NOT IN ('dismissed', 'failed')
                           )
                         )
                       ) \
                     ORDER BY datetime(e.created_at) DESC, e.rowid DESC \
                     LIMIT ?"
                ),
                vec![
                    room_id.into(),
                    conversation_id.into(),
                    conversation_id.into(),
                    fetch_limit.into(),
                ],
                limit,
            )
            .await?
        }
    };
    let mut events = Vec::with_capacity(rows.len());
    for row in rows {
        events.push(timeline_event_from_row(conn, &row).await?);
    }
    attach_reply_progress(conn, &mut events).await?;
    Ok(RoomTimeline {
        room_id: room_id.to_string(),
        events,
        truncated,
    })
}

/// Fill in the "M of N answered" ledger for every asking post in one page.
///
/// One `GROUP BY` for the whole window, never one query per post — a page is
/// capped at `MAX_TIMELINE_LIMIT`, which stays well under SQLite's bound
/// parameter limit, so the `IN` list is safe to expand in full.
///
/// Counting follows the obligation invariant the model audit wrote down
/// (`MODEL-AUDIT-RFC-2026-08-21`, slice ③): `obligation_state` says whether a
/// debt is open, but "this ask was voided" lives in `delivery.state`, so a
/// `dismissed` or `failed` row is not an obligation and drops out of *both*
/// the numerator and the denominator. Leaving it in the denominator would
/// park a post at "1/2 answered" with nobody left who could ever answer.
async fn attach_reply_progress(
    conn: &DatabaseConnection,
    events: &mut [RoomTimelineEvent],
) -> Result<(), DbError> {
    let ids: Vec<sea_orm::Value> = events
        .iter()
        .filter(|event| event.expects_reply)
        .map(|event| event.id.clone().into())
        .collect();
    if ids.is_empty() {
        return Ok(());
    }
    let placeholders = std::iter::repeat_n("?", ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let rows = conn
        .query_all(statement(
            &format!(
                "SELECT d.event_id AS event_id, \
                        COUNT(*) AS expected_count, \
                        SUM(CASE WHEN d.obligation_state = 'resolved' THEN 1 ELSE 0 END) \
                          AS resolved_count \
                   FROM collaboration_delivery d \
                  WHERE d.event_id IN ({placeholders}) \
                    AND d.state <> 'dismissed' AND d.state <> 'failed' \
                  GROUP BY d.event_id"
            ),
            ids,
        ))
        .await?;
    let mut progress: HashMap<String, (u32, u32)> = HashMap::with_capacity(rows.len());
    for row in rows {
        let event_id: String = row.try_get("", "event_id")?;
        let expected: i64 = row.try_get("", "expected_count")?;
        let resolved: i64 = row.try_get("", "resolved_count")?;
        let expected = u32::try_from(expected.max(0)).unwrap_or(u32::MAX);
        // `SUM` over a subset of the counted rows can never exceed `COUNT`;
        // the cap only keeps a nonsense fraction off the screen if it did.
        let resolved = u32::try_from(resolved.max(0))
            .unwrap_or(u32::MAX)
            .min(expected);
        progress.insert(event_id, (expected, resolved));
    }
    for event in events.iter_mut() {
        if !event.expects_reply {
            continue;
        }
        // An ask with no surviving Delivery row still reports `Some(0)`: the
        // post did ask, the UI just has no fraction worth drawing.
        let (expected, resolved) = progress.get(&event.id).copied().unwrap_or((0, 0));
        event.expected_reply_count = Some(expected);
        event.resolved_reply_count = Some(resolved);
    }
    Ok(())
}

async fn newest_window(
    conn: &DatabaseConnection,
    sql: &str,
    values: Vec<sea_orm::Value>,
    limit: i64,
) -> Result<(Vec<QueryResult>, bool), DbError> {
    let mut rows = conn.query_all(statement(sql, values)).await?;
    let truncated = (rows.len() as i64) > limit;
    if truncated {
        rows.truncate(limit as usize);
    }
    rows.reverse();
    Ok((rows, truncated))
}

async fn timeline_event_from_row(
    conn: &DatabaseConnection,
    row: &QueryResult,
) -> Result<RoomTimelineEvent, DbError> {
    let event_id: String = row.try_get("", "id")?;
    let mention_rows = conn
        .query_all(statement(
            "SELECT target_conversation_id FROM collaboration_delivery \
             WHERE event_id = ? ORDER BY target_conversation_id",
            vec![event_id.clone().into()],
        ))
        .await?;
    let mention_conversation_ids = mention_rows
        .iter()
        .map(|item| {
            item.try_get("", "target_conversation_id")
                .map_err(DbError::from)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let urgency_raw: String = row.try_get("", "urgency")?;
    let urgency = CollaborationUrgency::parse(&urgency_raw)
        .ok_or_else(|| validation(format!("Unknown urgency: {urgency_raw}")))?;
    let expects_reply: i64 = row.try_get("", "expects_reply")?;
    Ok(RoomTimelineEvent {
        id: event_id,
        room_id: row.try_get("", "room_id")?,
        source: CollaborationSessionSnapshot {
            conversation_id: row.try_get("", "source_conversation_id")?,
            title: row.try_get("", "source_title_snapshot")?,
            agent_type: Some(row.try_get("", "source_agent_type_snapshot")?),
            folder_path: row.try_get("", "source_folder_path_snapshot")?,
            backend: "current".to_string(),
            model: row.try_get("", "source_model_snapshot")?,
            profile: row.try_get("", "source_profile_snapshot")?,
        },
        subject: row
            .try_get::<Option<String>>("", "subject")?
            .unwrap_or_default(),
        body: row.try_get("", "body")?,
        reply_to_event_id: row.try_get("", "reply_to_event_id")?,
        expects_reply: expects_reply != 0,
        urgency,
        mention_conversation_ids,
        mention_human: {
            let raw: i64 = row.try_get("", "mention_human")?;
            raw != 0
        },
        author_kind: {
            let raw: String = row.try_get("", "author_kind")?;
            crate::models::CollaborationAuthorKind::parse(&raw)
                .unwrap_or(crate::models::CollaborationAuthorKind::Session)
        },
        // Filled in per page by `attach_reply_progress`, not per row.
        expected_reply_count: None,
        resolved_reply_count: None,
        created_at: parse_timestamp(row, "created_at")?,
    })
}

/// Load one Room-visibility event by id. Does not check membership and does
/// not consume a read cursor — callers that expose this to an Agent must
/// `require_member` themselves.
pub async fn get_room_event(
    conn: &DatabaseConnection,
    event_id: &str,
) -> Result<RoomTimelineEvent, DbError> {
    let row = conn
        .query_one(statement(
            &format!(
                "{EVENT_SELECT} \
                 WHERE e.id = ? AND COALESCE(e.visibility, 'direct') = 'room'"
            ),
            vec![event_id.into()],
        ))
        .await?
        .ok_or_else(|| DbError::NotFound(format!("Room post {event_id}")))?;
    timeline_event_from_row(conn, &row).await
}

/// Mark Room `@` deliveries in this window as consumed, and optionally move
/// the member channel cursor to the last returned post. Does not clear
/// `expects_reply`; that still needs `post_room` with `reply_to_event_id`.
pub async fn consume_room_window(
    conn: &DatabaseConnection,
    room_id: &str,
    conversation_id: i32,
    event_ids: &[String],
    advance_cursor: bool,
) -> Result<(), DbError> {
    require_member(conn, room_id, conversation_id).await?;
    if event_ids.is_empty() && !advance_cursor {
        return Ok(());
    }
    let txn = conn.begin().await?;
    for event_id in event_ids {
        txn.execute(statement(
            "UPDATE collaboration_delivery \
             SET agent_received_at = COALESCE(agent_received_at, CURRENT_TIMESTAMP), \
                 agent_receipt_kind = COALESCE(agent_receipt_kind, 'managed_acp'), \
                 agent_receipt_ref = COALESCE(agent_receipt_ref, 'room_read'), \
                 updated_at = CURRENT_TIMESTAMP \
             WHERE target_conversation_id = ? AND event_id = ? \
               AND state <> 'dismissed'",
            vec![conversation_id.into(), event_id.clone().into()],
        ))
        .await?;
    }
    if advance_cursor {
        if let Some(last_id) = event_ids.last() {
            txn.execute(statement(
                "UPDATE collaboration_room_member \
                 SET last_read_event_id = ?, \
                     last_read_at = (SELECT created_at FROM collaboration_event WHERE id = ?) \
                 WHERE room_id = ? AND conversation_id = ?",
                vec![
                    last_id.clone().into(),
                    last_id.clone().into(),
                    room_id.into(),
                    conversation_id.into(),
                ],
            ))
            .await?;
        }
    }
    txn.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::service::collaboration_service::{feed, list_inbox, send};
    use crate::db::service::workbench_service;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::AgentType;
    use crate::models::{
        CollaborationDeliveryState, CollaborationInvocationPolicy, CollaborationObligationState,
        PostRoomMessageInput, SendCollaborationMessageInput,
    };
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    async fn seeded() -> (crate::db::AppDatabase, i32, i32, i32) {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/codeg-room").await;
        let a = seed_conversation(&db, folder_id, AgentType::Codex).await;
        let b = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let c = seed_conversation(&db, folder_id, AgentType::Gemini).await;
        (db, a, b, c)
    }

    async fn make_room(
        db: &crate::db::AppDatabase,
        created_by: i32,
        members: Vec<i32>,
    ) -> CollaborationRoomDetail {
        create(
            &db.conn,
            CreateCollaborationRoomInput {
                workbench_id: 1,
                title: "Plan".into(),
                member_conversation_ids: members,
                created_by_conversation_id: created_by,
                collection_id: None,
                root_folder_id: None,
            },
        )
        .await
        .expect("create room")
    }

    #[tokio::test]
    async fn create_room_requires_at_least_one_live_member_and_lists_on_workbench() {
        let (db, a, b, _) = seeded().await;
        let err = create(
            &db.conn,
            CreateCollaborationRoomInput {
                workbench_id: 1,
                title: "Empty".into(),
                member_conversation_ids: vec![],
                created_by_conversation_id: a,
                collection_id: None,
                root_folder_id: None,
            },
        )
        .await
        .expect_err("zero members");
        assert!(err.to_string().contains("at least one"));

        let room = make_room(&db, a, vec![a, b]).await;
        assert_eq!(room.members.len(), 2);
        assert_eq!(room.created_by_conversation_id, a);
        assert!(room.members.iter().all(|m| m.role == "member"));
        let listed = list_for_workbench(&db.conn, 1).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, room.id);
        assert_eq!(listed[0].member_count, 2);
    }

    #[tokio::test]
    async fn create_room_accepts_a_single_live_member() {
        let (db, a, _, _) = seeded().await;
        let room = make_room(&db, a, vec![a]).await;
        assert_eq!(room.members.len(), 1);
        assert_eq!(room.created_by_conversation_id, a);
        assert_eq!(room.members[0].conversation_id, a);
        assert_eq!(room.members[0].role, "member");
        let listed = list_for_workbench(&db.conn, 1).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, room.id);
        assert_eq!(listed[0].member_count, 1);
    }

    #[tokio::test]
    async fn room_post_freezes_the_source_model_and_profile() {
        let (db, a, claude, _) = seeded().await;
        db.conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE conversation SET model = ?, preferred_config_values = ? WHERE id = ?",
                vec![
                    "claude-opus-5".into(),
                    r#"{"__codeg_profile__":"cpa"}"#.into(),
                    claude.into(),
                ],
            ))
            .await
            .expect("bind runtime selectors");
        let room = make_room(&db, a, vec![a, claude]).await;

        crate::db::service::collaboration_service::post_room(
            &db.conn,
            ask_post(
                room.id.clone(),
                claude,
                vec![a],
                "runtime-snapshot",
                crate::models::CollaborationAuthorKind::Session,
            ),
        )
        .await
        .expect("post");

        let posts = timeline(&db.conn, &room.id, None).await.expect("timeline");
        assert_eq!(
            posts.events[0].source.model.as_deref(),
            Some("claude-opus-5")
        );
        assert_eq!(posts.events[0].source.profile.as_deref(), Some("cpa"));

        db.conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE conversation SET model = ?, preferred_config_values = ? WHERE id = ?",
                vec![
                    "claude-sonnet-5".into(),
                    r#"{"__codeg_profile__":"official"}"#.into(),
                    claude.into(),
                ],
            ))
            .await
            .expect("change selectors later");
        let posts = timeline(&db.conn, &room.id, None).await.expect("timeline");
        assert_eq!(
            posts.events[0].source.model.as_deref(),
            Some("claude-opus-5")
        );
        assert_eq!(posts.events[0].source.profile.as_deref(), Some("cpa"));
    }

    #[tokio::test]
    async fn list_all_for_host_spans_workbenches_and_filters_by_title() {
        let (db, a, b, c) = seeded().await;
        let second = workbench_service::create(&db.conn, Some("Review".into()))
            .await
            .expect("second workbench");
        let planning = create(
            &db.conn,
            CreateCollaborationRoomInput {
                workbench_id: 1,
                title: "Release planning".into(),
                member_conversation_ids: vec![a, b],
                created_by_conversation_id: a,
                collection_id: None,
                root_folder_id: None,
            },
        )
        .await
        .expect("room on the default workbench");
        let triage = create(
            &db.conn,
            CreateCollaborationRoomInput {
                workbench_id: second.id,
                title: "Bug triage".into(),
                member_conversation_ids: vec![a, b, c],
                created_by_conversation_id: b,
                collection_id: None,
                root_folder_id: None,
            },
        )
        .await
        .expect("room on the second workbench");

        let all = list_all_for_host(&db.conn, None).await.unwrap();
        assert_eq!(
            all.len(),
            2,
            "the Session Center has no Workbench to scope by: both must be listed"
        );
        let triage_summary = all
            .iter()
            .find(|room| room.id == triage.id)
            .expect("the second Workbench's Room");
        assert_eq!(triage_summary.member_count, 3);
        assert_eq!(triage_summary.workbench_id, second.id);

        // Lower-cased on purpose: SQLite's LIKE is ASCII-insensitive, which is
        // what the Session title search relies on too.
        let hits = list_all_for_host(&db.conn, Some("release"))
            .await
            .expect("title search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, planning.id);
        assert!(
            list_all_for_host(&db.conn, Some("nothing by this name"))
                .await
                .unwrap()
                .is_empty(),
            "a query nothing matches must narrow to nothing, not fall back to everything"
        );
        assert_eq!(
            list_all_for_host(&db.conn, Some("   "))
                .await
                .unwrap()
                .len(),
            2,
            "whitespace is not a query"
        );
    }

    #[tokio::test]
    async fn list_all_for_host_carries_host_counts_and_skips_inactive_rooms() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        crate::db::service::collaboration_service::post_room(
            &db.conn,
            PostRoomMessageInput {
                room_id: room.id.clone(),
                source_conversation_id: a,
                target_conversation_ids: vec![b],
                mention_all: false,
                body: "please review the plan".into(),
                client_dedupe_id: "room-list-all-1".into(),
                invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                delivery_hint: Default::default(),
                expects_reply: true,
                urgency: Default::default(),
                reply_to_event_id: None,
                mention_human: false,
                author_kind: Default::default(),
            },
        )
        .await
        .expect("post");

        let listed = list_all_for_host(&db.conn, None).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].unread_count, 1);
        assert_eq!(
            listed[0].needs_reply_count, 1,
            "the same host expressions list_for_workbench shows, so the two can never disagree"
        );
        assert_eq!(
            listed[0].awaiting_reply_count, 0,
            "an Agent asked, not the user"
        );

        db.conn
            .execute(Statement::from_string(
                DbBackend::Sqlite,
                format!(
                    "UPDATE collaboration_room SET status = 'archived' WHERE id = '{}'",
                    room.id
                ),
            ))
            .await
            .unwrap();
        assert!(
            list_all_for_host(&db.conn, None).await.unwrap().is_empty(),
            "a Room parked out of 'active' leaves the Session Center with it"
        );
    }

    #[tokio::test]
    async fn add_path_validates_existence_and_directory_before_writing() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;

        let missing_err = add_path(&db.conn, &room.id, "/definitely/does/not/exist-xyz")
            .await
            .expect_err("a nonexistent path must be rejected");
        assert!(matches!(missing_err, DbError::Validation(_)));

        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("a-file.txt");
        std::fs::write(&file_path, b"x").expect("write file");
        let not_dir_err = add_path(&db.conn, &room.id, &file_path.to_string_lossy())
            .await
            .expect_err("a file (not a directory) must be rejected");
        assert!(matches!(not_dir_err, DbError::Validation(_)));

        let dir_path = dir.path().to_string_lossy().to_string();
        let updated = add_path(&db.conn, &room.id, &dir_path)
            .await
            .expect("a real, existing directory must be accepted");
        assert_eq!(updated.additional_paths.len(), 1);
        assert_eq!(updated.additional_paths[0].path, dir_path);
    }

    #[tokio::test]
    async fn add_path_is_a_no_op_for_an_exact_duplicate() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        let dir = tempfile::tempdir().expect("tempdir");
        let dir_path = dir.path().to_string_lossy().to_string();

        add_path(&db.conn, &room.id, &dir_path)
            .await
            .expect("first add");
        let updated = add_path(&db.conn, &room.id, &dir_path)
            .await
            .expect("re-adding the same path is a no-op, not an error — mirrors add_members");
        assert_eq!(
            updated.additional_paths.len(),
            1,
            "a duplicate path must not be stored twice"
        );
    }

    #[tokio::test]
    async fn add_path_enforces_the_max_additional_paths_cap() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        let dirs: Vec<_> = (0..MAX_ADDITIONAL_PATHS)
            .map(|_| tempfile::tempdir().expect("tempdir"))
            .collect();
        for dir in &dirs {
            add_path(&db.conn, &room.id, &dir.path().to_string_lossy())
                .await
                .expect("adding up to the cap must succeed");
        }

        let one_more = tempfile::tempdir().expect("tempdir");
        let err = add_path(&db.conn, &room.id, &one_more.path().to_string_lossy())
            .await
            .expect_err("exceeding the cap must be rejected");
        assert!(matches!(err, DbError::Validation(_)));
    }

    #[tokio::test]
    async fn remove_path_deletes_by_id_and_rejects_a_second_removal() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        let dir = tempfile::tempdir().expect("tempdir");
        let updated = add_path(&db.conn, &room.id, &dir.path().to_string_lossy())
            .await
            .expect("add");
        let path_id = updated.additional_paths[0].id;

        let after_remove = remove_path(&db.conn, &room.id, path_id)
            .await
            .expect("remove");
        assert!(after_remove.additional_paths.is_empty());

        let err = remove_path(&db.conn, &room.id, path_id)
            .await
            .expect_err("removing an already-removed path id must fail, not silently no-op");
        assert!(matches!(err, DbError::NotFound(_)));
    }

    #[tokio::test]
    async fn room_summary_reports_the_additional_path_count() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        let dir = tempfile::tempdir().expect("tempdir");
        add_path(&db.conn, &room.id, &dir.path().to_string_lossy())
            .await
            .expect("add");

        let summaries = list_for_workbench(&db.conn, room.workbench_id)
            .await
            .expect("list");
        let summary = summaries
            .iter()
            .find(|s| s.id == room.id)
            .expect("room present in the workbench listing");
        assert_eq!(summary.additional_path_count, 1);
    }

    #[tokio::test]
    async fn delete_room_drops_events_members_and_the_room() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        crate::db::service::collaboration_service::post_room(
            &db.conn,
            PostRoomMessageInput {
                room_id: room.id.clone(),
                source_conversation_id: a,
                target_conversation_ids: vec![b],
                mention_all: false,
                body: "check this".into(),
                client_dedupe_id: "room-delete-1".into(),
                invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                delivery_hint: Default::default(),
                expects_reply: true,
                urgency: Default::default(),
                reply_to_event_id: None,
                mention_human: false,
                author_kind: Default::default(),
            },
        )
        .await
        .expect("post");

        let workbench_id = delete(&db.conn, &room.id).await.expect("delete room");
        assert_eq!(workbench_id, 1);
        assert!(get(&db.conn, &room.id).await.is_err());
        assert!(list_for_workbench(&db.conn, 1).await.unwrap().is_empty());

        let events: i64 = db
            .conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                format!(
                    "SELECT COUNT(*) AS count FROM collaboration_event WHERE room_id = '{}'",
                    room.id
                ),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "count")
            .unwrap();
        assert_eq!(events, 0);
        let members: i64 = db
            .conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                format!(
                    "SELECT COUNT(*) AS count FROM collaboration_room_member WHERE room_id = '{}'",
                    room.id
                ),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "count")
            .unwrap();
        assert_eq!(members, 0);
        let queued: i64 = db
            .conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS count FROM conversation_prompt_queue_item".to_owned(),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "count")
            .unwrap();
        assert_eq!(queued, 0);
    }

    #[tokio::test]
    async fn record_only_room_post_is_visible_in_room_and_absent_from_mail() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        let posted = crate::db::service::collaboration_service::post_room(
            &db.conn,
            PostRoomMessageInput {
                room_id: room.id.clone(),
                source_conversation_id: a,
                target_conversation_ids: vec![],
                mention_all: false,
                body: "record only".into(),
                client_dedupe_id: "room-record-1".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: Default::default(),
                expects_reply: false,
                urgency: Default::default(),
                reply_to_event_id: None,
                mention_human: false,
                author_kind: Default::default(),
            },
        )
        .await
        .expect("post");
        assert!(posted.deliveries.is_empty());
        let timeline = timeline(&db.conn, &room.id, None).await.unwrap();
        assert_eq!(timeline.events.len(), 1);
        assert_eq!(timeline.events[0].body, "record only");
        assert!(timeline.events[0].mention_conversation_ids.is_empty());

        assert_eq!(feed(&db.conn, b, None).await.unwrap().unread_count, 0);
        assert!(list_inbox(
            &db.conn,
            b,
            crate::acp::session_collaboration::SessionMailboxScope::Inbox,
            crate::acp::session_collaboration::SessionInboxFilter::All,
            None,
            20,
        )
        .await
        .unwrap()
        .is_empty());
        let queued: i64 = db
            .conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS count FROM conversation_prompt_queue_item".to_owned(),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "count")
            .unwrap();
        assert_eq!(queued, 0);
    }

    #[tokio::test]
    async fn room_mention_creates_delivery_without_leaking_into_mail_lists() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        let posted = crate::db::service::collaboration_service::post_room(
            &db.conn,
            PostRoomMessageInput {
                room_id: room.id.clone(),
                source_conversation_id: a,
                target_conversation_ids: vec![b],
                mention_all: false,
                body: "check this".into(),
                client_dedupe_id: "room-at-1".into(),
                invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                delivery_hint: Default::default(),
                expects_reply: true,
                urgency: Default::default(),
                reply_to_event_id: None,
                mention_human: false,
                author_kind: Default::default(),
            },
        )
        .await
        .expect("post");
        assert_eq!(posted.deliveries.len(), 1);
        assert_eq!(posted.deliveries[0].target.conversation_id, b);
        assert_eq!(
            posted.deliveries[0].obligation_state,
            CollaborationObligationState::AwaitingReply
        );
        let queued: i64 = db
            .conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS count FROM conversation_prompt_queue_item".to_owned(),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "count")
            .unwrap();
        assert_eq!(queued, 1);

        let draft = crate::db::service::collaboration_service::prompt_draft_for_origin(
            &db.conn,
            b,
            &posted.event_id,
        )
        .await
        .expect("Room @ must be injectable");
        let crate::acp::types::PromptInputBlock::Text { text } = &draft.blocks[0] else {
            panic!("Room @ must resolve to one text envelope");
        };
        assert!(text.contains("\"kind\":\"room_mention\""));
        assert!(text.contains("\"channel\":\"room\""));
        assert!(text.contains("channel=room"));
        assert!(draft.display_text.starts_with("Codeg room:"));
        assert!(text.contains("check this"));
        assert!(text.contains("Call read_room"));
        assert!(text.contains("does not wake that author"));
        assert!(!text.contains("Call list_inbox"));
        assert!(!text.contains("Call read_message"));
        assert!(!text.contains("channel=mailbox"));

        db.conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE collaboration_delivery \
                 SET created_at = datetime('now', '-6 minutes') \
                 WHERE event_id = ?",
                vec![posted.event_id.clone().into()],
            ))
            .await
            .unwrap();
        assert!(
            crate::db::service::collaboration_service::list_overdue_reminder_targets(&db.conn)
                .await
                .unwrap()
                .iter()
                .any(|target| target.conversation_id == b),
            "Room @ must share the overdue reply/unread reminder sweep"
        );

        let inbox = list_inbox(
            &db.conn,
            b,
            crate::acp::session_collaboration::SessionMailboxScope::Inbox,
            crate::acp::session_collaboration::SessionInboxFilter::All,
            None,
            20,
        )
        .await
        .unwrap();
        assert!(
            inbox.is_empty(),
            "Room @ must not appear in the mailbox list"
        );
        assert_eq!(
            feed(&db.conn, b, None).await.unwrap().unread_count,
            0,
            "Room @ must not bump mailbox unread"
        );

        let letter = send(
            &db.conn,
            SendCollaborationMessageInput::letter(a, vec![b], "mail-1", "Private", "secret"),
        )
        .await
        .unwrap();
        assert_eq!(letter.deliveries.len(), 1);
        let inbox = list_inbox(
            &db.conn,
            b,
            crate::acp::session_collaboration::SessionMailboxScope::Inbox,
            crate::acp::session_collaboration::SessionInboxFilter::All,
            None,
            20,
        )
        .await
        .unwrap();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].body, "secret");
        assert_eq!(feed(&db.conn, b, None).await.unwrap().unread_count, 1);
    }

    #[tokio::test]
    async fn archived_member_stays_mentioned_but_the_delivery_fails_visibly() {
        let (db, a, b, c) = seeded().await;
        let room = make_room(&db, a, vec![a, b, c]).await;
        crate::db::service::conversation_service::update_archive(&db.conn, b, true)
            .await
            .expect("archive");

        let posted = crate::db::service::collaboration_service::post_room(
            &db.conn,
            PostRoomMessageInput {
                room_id: room.id.clone(),
                source_conversation_id: a,
                target_conversation_ids: vec![b, c],
                mention_all: false,
                body: "archived stays on the ledger".into(),
                client_dedupe_id: "room-archived-1".into(),
                invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                delivery_hint: Default::default(),
                expects_reply: true,
                urgency: Default::default(),
                reply_to_event_id: None,
                mention_human: false,
                author_kind: Default::default(),
            },
        )
        .await
        .expect("post");
        assert_eq!(posted.deliveries.len(), 2);

        let archived = posted
            .deliveries
            .iter()
            .find(|item| item.target.conversation_id == b)
            .expect("archived delivery");
        assert_eq!(archived.state, CollaborationDeliveryState::Failed);
        assert_eq!(archived.error.as_deref(), Some("target_archived"));
        assert_eq!(
            archived.obligation_state,
            CollaborationObligationState::None
        );
        assert!(archived.queue_item_id.is_none());

        let live = posted
            .deliveries
            .iter()
            .find(|item| item.target.conversation_id == c)
            .expect("live delivery");
        assert_eq!(
            live.invocation_policy,
            CollaborationInvocationPolicy::InvokeWhenIdle
        );
        assert_eq!(live.state, CollaborationDeliveryState::Queued);
        assert_eq!(
            live.obligation_state,
            CollaborationObligationState::AwaitingReply
        );
        assert!(live.queue_item_id.is_some());

        let queued: i64 = db
            .conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS count FROM conversation_prompt_queue_item".to_owned(),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "count")
            .unwrap();
        assert_eq!(queued, 1);

        let timeline = timeline(&db.conn, &room.id, None).await.unwrap();
        assert_eq!(timeline.events.len(), 1);
        assert_eq!(
            timeline.events[0].mention_conversation_ids,
            vec![b, c],
            "the public @ remains visible even when the target is archived"
        );
        assert!(list_inbox(
            &db.conn,
            b,
            crate::acp::session_collaboration::SessionMailboxScope::Inbox,
            crate::acp::session_collaboration::SessionInboxFilter::All,
            None,
            20,
        )
        .await
        .unwrap()
        .is_empty());
    }

    #[tokio::test]
    async fn room_reply_stays_in_room_and_mail_fanout_still_isolates_peers() {
        let (db, a, b, c) = seeded().await;
        let room = make_room(&db, a, vec![a, b, c]).await;
        let root = crate::db::service::collaboration_service::post_room(
            &db.conn,
            PostRoomMessageInput {
                room_id: room.id.clone(),
                source_conversation_id: a,
                target_conversation_ids: vec![b],
                mention_all: false,
                body: "please reply in room".into(),
                client_dedupe_id: "room-root".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: Default::default(),
                expects_reply: true,
                urgency: Default::default(),
                reply_to_event_id: None,
                mention_human: false,
                author_kind: Default::default(),
            },
        )
        .await
        .unwrap();
        let reply = crate::db::service::collaboration_service::post_room(
            &db.conn,
            PostRoomMessageInput {
                room_id: room.id.clone(),
                source_conversation_id: b,
                target_conversation_ids: vec![],
                mention_all: false,
                body: "here is the room answer".into(),
                client_dedupe_id: "room-reply".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: Default::default(),
                expects_reply: false,
                urgency: Default::default(),
                reply_to_event_id: Some(root.event_id.clone()),
                mention_human: false,
                author_kind: Default::default(),
            },
        )
        .await
        .unwrap();
        assert!(reply.deliveries.is_empty());
        let events = timeline(&db.conn, &room.id, None).await.unwrap().events;
        assert_eq!(events.len(), 2);
        let reply_event = events
            .iter()
            .find(|event| event.body == "here is the room answer")
            .expect("room reply");
        assert_eq!(
            reply_event.reply_to_event_id.as_deref(),
            Some(root.event_id.as_str())
        );

        let fanout = send(
            &db.conn,
            SendCollaborationMessageInput::letter(
                a,
                vec![b, c],
                "fanout",
                "Mail",
                "private fanout",
            ),
        )
        .await
        .unwrap();
        assert_eq!(fanout.deliveries.len(), 2);
        let b_inbox = list_inbox(
            &db.conn,
            b,
            crate::acp::session_collaboration::SessionMailboxScope::Inbox,
            crate::acp::session_collaboration::SessionInboxFilter::All,
            None,
            20,
        )
        .await
        .unwrap();
        assert_eq!(b_inbox.len(), 1);
        assert_eq!(b_inbox[0].body, "private fanout");
    }

    #[tokio::test]
    async fn mark_seen_clears_room_unread_and_creator_can_leave() {
        let (db, a, b, c) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        crate::db::service::collaboration_service::post_room(
            &db.conn,
            PostRoomMessageInput {
                room_id: room.id.clone(),
                source_conversation_id: a,
                target_conversation_ids: vec![],
                mention_all: false,
                body: "hello room".into(),
                client_dedupe_id: "seen-1".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: Default::default(),
                expects_reply: false,
                urgency: Default::default(),
                reply_to_event_id: None,
                mention_human: false,
                author_kind: Default::default(),
            },
        )
        .await
        .unwrap();
        let listed = list_for_workbench(&db.conn, 1).await.unwrap();
        assert_eq!(listed[0].unread_count, 1);
        let updated_before = get(&db.conn, &room.id).await.unwrap().updated_at;
        mark_seen(&db.conn, &room.id, Some(a)).await.unwrap();
        let listed = list_for_workbench(&db.conn, 1).await.unwrap();
        assert_eq!(listed[0].unread_count, 0);
        assert_eq!(
            get(&db.conn, &room.id).await.unwrap().updated_at,
            updated_before,
            "opening a Room must not reshuffle the sidebar by bumping updated_at"
        );

        add_members(
            &db.conn,
            AddCollaborationRoomMembersInput {
                room_id: room.id.clone(),
                conversation_ids: vec![c],
            },
        )
        .await
        .unwrap();
        remove_member(&db.conn, &room.id, a)
            .await
            .expect("creator Session can leave while other members remain");
        let after = get(&db.conn, &room.id).await.unwrap();
        assert_eq!(after.members.len(), 2);
        assert!(after
            .members
            .iter()
            .all(|member| member.conversation_id != a));
        remove_member(&db.conn, &room.id, c)
            .await
            .expect("a Room may shrink to a single Session");
        let after_one = get(&db.conn, &room.id).await.unwrap();
        assert_eq!(after_one.members.len(), 1);
        let last = after_one.members[0].conversation_id;
        let err = remove_member(&db.conn, &room.id, last)
            .await
            .expect_err("floor is one member");
        assert!(err.to_string().contains("at least one"));
    }

    #[tokio::test]
    async fn list_for_member_returns_only_rooms_the_session_joined() {
        let (db, a, b, c) = seeded().await;
        let ab = make_room(&db, a, vec![a, b]).await;
        let ac = create(
            &db.conn,
            CreateCollaborationRoomInput {
                workbench_id: 1,
                title: "A and C".into(),
                member_conversation_ids: vec![a, c],
                created_by_conversation_id: a,
                collection_id: None,
                root_folder_id: None,
            },
        )
        .await
        .unwrap();

        let for_b = list_for_member(&db.conn, b).await.unwrap();
        assert_eq!(for_b.len(), 1);
        assert_eq!(for_b[0].id, ab.id);
        let for_a = list_for_member(&db.conn, a).await.unwrap();
        let ids: Vec<_> = for_a.iter().map(|room| room.id.as_str()).collect();
        assert!(ids.contains(&ab.id.as_str()));
        assert!(ids.contains(&ac.id.as_str()));
        assert_eq!(for_a.len(), 2);
    }

    #[tokio::test]
    async fn reply_mention_envelope_embeds_a_parent_snippet() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        let parent = crate::db::service::collaboration_service::post_room(
            &db.conn,
            PostRoomMessageInput {
                room_id: room.id.clone(),
                source_conversation_id: a,
                target_conversation_ids: vec![],
                mention_all: false,
                body: "please review the plan".into(),
                client_dedupe_id: "room-parent-1".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: Default::default(),
                expects_reply: false,
                urgency: Default::default(),
                reply_to_event_id: None,
                mention_human: false,
                author_kind: Default::default(),
            },
        )
        .await
        .expect("parent");
        let posted = crate::db::service::collaboration_service::post_room(
            &db.conn,
            PostRoomMessageInput {
                room_id: room.id.clone(),
                source_conversation_id: a,
                target_conversation_ids: vec![b],
                mention_all: false,
                body: "looking now".into(),
                client_dedupe_id: "room-reply-at".into(),
                invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                delivery_hint: Default::default(),
                expects_reply: true,
                urgency: Default::default(),
                reply_to_event_id: Some(parent.event_id.clone()),
                mention_human: false,
                author_kind: Default::default(),
            },
        )
        .await
        .expect("reply mention");
        let draft = crate::db::service::collaboration_service::prompt_draft_for_origin(
            &db.conn,
            b,
            &posted.event_id,
        )
        .await
        .expect("reply mention must be injectable");
        let crate::acp::types::PromptInputBlock::Text { text } = &draft.blocks[0] else {
            panic!("expected a text envelope");
        };
        assert!(text.contains("looking now"));
        assert!(text.contains("Quoted post"));
        assert!(text.contains("please review the plan"));
        assert!(text.contains("\"parentSnippet\":\"please review the plan\""));
        assert!(text.contains("does not wake that author"));
    }

    #[tokio::test]
    async fn list_for_member_counts_unpaid_replies_and_timeline_can_filter_them() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        crate::db::service::collaboration_service::post_room(
            &db.conn,
            PostRoomMessageInput {
                room_id: room.id.clone(),
                source_conversation_id: a,
                target_conversation_ids: vec![b],
                mention_all: false,
                body: "please answer".into(),
                client_dedupe_id: "room-owes-1".into(),
                invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                delivery_hint: Default::default(),
                expects_reply: true,
                urgency: Default::default(),
                reply_to_event_id: None,
                mention_human: false,
                author_kind: Default::default(),
            },
        )
        .await
        .expect("ask");
        crate::db::service::collaboration_service::post_room(
            &db.conn,
            PostRoomMessageInput {
                room_id: room.id.clone(),
                source_conversation_id: a,
                target_conversation_ids: vec![],
                mention_all: false,
                body: "status only".into(),
                client_dedupe_id: "room-status-1".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: Default::default(),
                expects_reply: false,
                urgency: Default::default(),
                reply_to_event_id: None,
                mention_human: false,
                author_kind: Default::default(),
            },
        )
        .await
        .expect("status");

        let for_a = list_for_member(&db.conn, a).await.unwrap();
        assert_eq!(for_a[0].awaiting_reply_count, 1);
        assert_eq!(for_a[0].needs_reply_count, 0);
        let for_b = list_for_member(&db.conn, b).await.unwrap();
        assert_eq!(for_b[0].needs_reply_count, 1);
        assert_eq!(for_b[0].awaiting_reply_count, 0);

        let host = list_for_workbench(&db.conn, 1).await.unwrap();
        assert_eq!(
            host[0].needs_reply_count, 1,
            "the operator sees the Room's debt whoever owes it"
        );
        assert_eq!(
            host[0].awaiting_reply_count, 0,
            "nobody is waiting on the user: this post came from an Agent"
        );

        let unpaid = timeline_with(
            &db.conn,
            &room.id,
            Some(50),
            RoomTimelineMode::NeedsReply { conversation_id: b },
        )
        .await
        .unwrap();
        assert_eq!(unpaid.events.len(), 1);
        assert_eq!(unpaid.events[0].body, "please answer");

        let from_asker = timeline_with(
            &db.conn,
            &room.id,
            Some(50),
            RoomTimelineMode::NeedsReply { conversation_id: a },
        )
        .await
        .unwrap();
        assert_eq!(from_asker.events.len(), 1);
        assert_eq!(from_asker.events[0].body, "please answer");
    }

    fn ask_post(
        room_id: String,
        source: i32,
        targets: Vec<i32>,
        dedupe: &str,
        author_kind: crate::models::CollaborationAuthorKind,
    ) -> PostRoomMessageInput {
        PostRoomMessageInput {
            room_id,
            source_conversation_id: source,
            target_conversation_ids: targets,
            mention_all: false,
            body: "who is on this?".into(),
            client_dedupe_id: dedupe.into(),
            invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
            delivery_hint: Default::default(),
            expects_reply: true,
            urgency: Default::default(),
            reply_to_event_id: None,
            mention_human: false,
            author_kind,
        }
    }

    #[tokio::test]
    async fn host_room_counts_span_every_member_and_totals_match_the_list() {
        use crate::models::CollaborationAuthorKind;
        let (db, a, b, c) = seeded().await;
        let room = make_room(&db, a, vec![a, b, c]).await;
        // Two Agents asking each other, plus the user asking the other two:
        // four obligations sit in the Room, half of them the user's own.
        for ask in [
            ask_post(
                room.id.clone(),
                a,
                vec![b],
                "host-ask-1",
                CollaborationAuthorKind::Session,
            ),
            ask_post(
                room.id.clone(),
                b,
                vec![c],
                "host-ask-2",
                CollaborationAuthorKind::Session,
            ),
            ask_post(
                room.id.clone(),
                a,
                vec![b, c],
                "host-ask-human",
                CollaborationAuthorKind::Human,
            ),
        ] {
            crate::db::service::collaboration_service::post_room(&db.conn, ask)
                .await
                .expect("post");
        }

        let host = list_for_workbench(&db.conn, 1).await.unwrap();
        assert_eq!(host.len(), 1);
        assert_eq!(
            host[0].needs_reply_count, 4,
            "two Agent asks plus the user's fan-out to two members"
        );
        assert_eq!(
            host[0].awaiting_reply_count, 2,
            "only the human-authored post is the user's own outstanding ask"
        );

        let totals = host_totals(&db.conn).await.unwrap();
        assert_eq!(
            totals.needs_reply_count, host[0].needs_reply_count,
            "the overview total must be the sum of what the Rooms page shows"
        );
        assert_eq!(totals.unread_count, host[0].unread_count);

        // A deleted Session can never answer, so its debt must not keep the
        // badge lit; the user's own ask to that Session goes with it.
        db.conn
            .execute(statement(
                "UPDATE conversation SET deleted_at = CURRENT_TIMESTAMP WHERE id = ?",
                vec![c.into()],
            ))
            .await
            .unwrap();
        let after_delete = list_for_workbench(&db.conn, 1).await.unwrap();
        assert_eq!(after_delete[0].needs_reply_count, 2);
        assert_eq!(after_delete[0].awaiting_reply_count, 1);
        assert_eq!(
            host_totals(&db.conn).await.unwrap().needs_reply_count,
            after_delete[0].needs_reply_count
        );
    }

    /// The Room composer has no Session id of its own, so a human post is
    /// filed under the creator's (`rooms-page.tsx` sends
    /// `createdByConversationId`). Dropping the author from their own mention
    /// list used to swallow a human `@` of that creator whole — no Delivery,
    /// no wake, no badge, and no error to say why.
    #[tokio::test]
    async fn a_human_at_the_room_creator_is_delivered_woken_and_owed() {
        use crate::models::CollaborationAuthorKind;
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        let posted = crate::db::service::collaboration_service::post_room(
            &db.conn,
            ask_post(
                room.id.clone(),
                a,
                vec![a],
                "human-at-creator",
                CollaborationAuthorKind::Human,
            ),
        )
        .await
        .expect("human post");

        assert_eq!(
            posted.deliveries.len(),
            1,
            "the creator is an ordinary target of a human post"
        );
        let delivery = &posted.deliveries[0];
        assert_eq!(delivery.target.conversation_id, a);
        assert_eq!(delivery.state, CollaborationDeliveryState::Queued);
        assert_eq!(
            delivery.invocation_policy,
            CollaborationInvocationPolicy::InvokeWhenIdle
        );
        assert!(delivery.expects_reply);
        assert_eq!(
            delivery.obligation_state,
            CollaborationObligationState::AwaitingReply
        );

        let queue = crate::db::service::prompt_queue_service::snapshot(&db.conn, a)
            .await
            .expect("creator queue");
        assert_eq!(queue.items.len(), 1, "the creator is woken like any member");
        assert_eq!(
            queue.items[0].origin_event_id.as_deref(),
            Some(posted.event_id.as_str())
        );

        let for_creator = list_for_member(&db.conn, a).await.unwrap();
        assert_eq!(for_creator[0].mention_unread_count, 1);
        assert_eq!(for_creator[0].needs_reply_count, 1);
        assert_eq!(
            for_creator[0].awaiting_reply_count, 0,
            "the user asked, not the creator: one Delivery, counted on one side"
        );
        assert_eq!(
            for_creator[0].unread_count, 1,
            "a human post is not the creator's own writing, borrowed id or not"
        );
        assert_eq!(
            posted.open_reply_debt, 1,
            "the borrowed Session walks away owing this answer"
        );
    }

    /// The other half of the rule, kept honest: a Session author is still
    /// dropped from its own mention list and never wakes itself.
    #[tokio::test]
    async fn a_session_post_that_mentions_itself_still_creates_no_delivery() {
        use crate::models::CollaborationAuthorKind;
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        let posted = crate::db::service::collaboration_service::post_room(
            &db.conn,
            ask_post(
                room.id.clone(),
                a,
                vec![a, b],
                "session-at-self",
                CollaborationAuthorKind::Session,
            ),
        )
        .await
        .expect("session post");

        let targets: Vec<i32> = posted
            .deliveries
            .iter()
            .map(|delivery| delivery.target.conversation_id)
            .collect();
        assert_eq!(targets, vec![b], "the author's own `@` is dropped");
        assert!(
            crate::db::service::prompt_queue_service::snapshot(&db.conn, a)
                .await
                .unwrap()
                .items
                .is_empty(),
            "an Agent must not wake itself with its own post"
        );
    }

    /// `@all` is the same rule from the fan-out side: the user means every
    /// member, and the Room creator is a member.
    #[tokio::test]
    async fn mention_all_reaches_the_creator_for_a_human_but_not_for_a_session() {
        use crate::models::CollaborationAuthorKind;
        let (db, a, b, c) = seeded().await;
        let room = make_room(&db, a, vec![a, b, c]).await;

        let human = PostRoomMessageInput {
            mention_all: true,
            ..ask_post(
                room.id.clone(),
                a,
                vec![],
                "all-human",
                CollaborationAuthorKind::Human,
            )
        };
        let posted = crate::db::service::collaboration_service::post_room(&db.conn, human)
            .await
            .expect("human @all");
        let targets: Vec<i32> = posted
            .deliveries
            .iter()
            .map(|delivery| delivery.target.conversation_id)
            .collect();
        assert_eq!(targets, vec![a, b, c], "the user's @all leaves nobody out");

        let session = PostRoomMessageInput {
            mention_all: true,
            ..ask_post(
                room.id.clone(),
                a,
                vec![],
                "all-session",
                CollaborationAuthorKind::Session,
            )
        };
        let posted = crate::db::service::collaboration_service::post_room(&db.conn, session)
            .await
            .expect("session @all");
        let targets: Vec<i32> = posted
            .deliveries
            .iter()
            .map(|delivery| delivery.target.conversation_id)
            .collect();
        assert_eq!(targets, vec![b, c], "an Agent's @all still excludes itself");
    }

    #[tokio::test]
    async fn host_mention_counts_posts_that_at_the_user_until_the_room_is_opened() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        crate::db::service::collaboration_service::post_room(
            &db.conn,
            record_post(room.id.clone(), a, "host-mention-plain", "just logging"),
        )
        .await
        .unwrap();
        crate::db::service::collaboration_service::post_room(
            &db.conn,
            record_post(
                room.id.clone(),
                a,
                "host-mention-human",
                "codeg://human ptal",
            ),
        )
        .await
        .unwrap();

        let before = list_for_workbench(&db.conn, 1).await.unwrap();
        assert_eq!(before[0].unread_count, 2);
        assert_eq!(
            before[0].mention_unread_count, 1,
            "only the post that @-ed the user counts as a mention"
        );

        mark_seen(&db.conn, &room.id, None).await.unwrap();
        let after = list_for_workbench(&db.conn, 1).await.unwrap();
        assert_eq!(after[0].unread_count, 0);
        assert_eq!(after[0].mention_unread_count, 0);
    }

    #[tokio::test]
    async fn room_placement_is_owned_by_the_room_not_the_creator_session() {
        let (db, a, b, _) = seeded().await;
        let folder_id: i32 = db
            .conn
            .query_one(sea_orm::Statement::from_sql_and_values(
                sea_orm::DbBackend::Sqlite,
                "SELECT folder_id FROM conversation WHERE id = ?",
                [a.into()],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "folder_id")
            .unwrap();
        let collection = crate::db::service::collection_service::create(
            &db.conn,
            "Sources".into(),
            None,
            Some(folder_id),
        )
        .await
        .expect("collection");
        crate::db::service::collection_service::assign_conversations(
            &db.conn,
            vec![a],
            Some(collection.id),
        )
        .await
        .expect("assign creator");

        let not_the_creators_slot = make_room(&db, a, vec![a, b]).await;
        assert_eq!(
            not_the_creators_slot.collection_id, None,
            "a Room does not inherit Collection from the creator Session alone"
        );
        assert_eq!(
            not_the_creators_slot.root_folder_id,
            Some(folder_id),
            "the Collection is not inherited, but the Path is: a Room with no \
             Path at all is unreachable from the sidebar tree"
        );

        crate::db::service::collection_service::assign_conversations(
            &db.conn,
            vec![a, b],
            Some(collection.id),
        )
        .await
        .expect("assign members");

        let shared_home = make_room(&db, a, vec![a, b]).await;
        assert_eq!(shared_home.collection_id, Some(collection.id));
        assert_eq!(shared_home.root_folder_id, Some(folder_id));

        let independent = create(
            &db.conn,
            CreateCollaborationRoomInput {
                workbench_id: 1,
                title: "Own home".into(),
                member_conversation_ids: vec![a, b],
                created_by_conversation_id: a,
                collection_id: None,
                root_folder_id: Some(folder_id),
            },
        )
        .await
        .unwrap();
        assert_eq!(independent.collection_id, None);
        assert_eq!(independent.root_folder_id, Some(folder_id));

        assign_to_collection(
            &db.conn,
            vec![independent.id.clone()],
            Some(collection.id),
            None,
        )
        .await
        .expect("move room");
        let moved = get(&db.conn, &independent.id).await.unwrap();
        assert_eq!(moved.collection_id, Some(collection.id));
    }

    #[tokio::test]
    async fn agent_created_room_adopts_the_creator_path_so_the_sidebar_can_place_it() {
        // `room.create` over MCP / Host Control passes neither Collection nor
        // Path, and its members are typically scattered across Paths — exactly
        // the shape that used to persist a Room no sidebar bucket could hold.
        let db = fresh_in_memory_db().await;
        let repo = seed_folder(&db, "/tmp/codeg-room-repo").await;
        let elsewhere = seed_folder(&db, "/tmp/codeg-room-elsewhere").await;
        let creator = seed_conversation(&db, repo, AgentType::ClaudeCode).await;
        let peer = seed_conversation(&db, elsewhere, AgentType::Codex).await;

        let room = make_room(&db, creator, vec![creator, peer]).await;

        assert_eq!(
            room.collection_id, None,
            "no Collection is implied when the members share none"
        );
        assert_eq!(
            room.root_folder_id,
            Some(repo),
            "the creator's Path is stamped so the Room has a tree bucket"
        );
    }

    #[tokio::test]
    async fn a_worktree_creator_resolves_to_the_repository_root() {
        let db = fresh_in_memory_db().await;
        let repo = seed_folder(&db, "/tmp/codeg-room-repo").await;
        let worktree = seed_folder(&db, "/tmp/codeg-room-repo-wt/feature").await;
        db.conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE folder SET parent_id = ? WHERE id = ?",
                [repo.into(), worktree.into()],
            ))
            .await
            .expect("point the worktree at its repository root");
        let creator = seed_conversation(&db, worktree, AgentType::ClaudeCode).await;
        let peer = seed_conversation(&db, repo, AgentType::Codex).await;

        let room = make_room(&db, creator, vec![creator, peer]).await;

        assert_eq!(
            room.root_folder_id,
            Some(repo),
            "a worktree is not its own tree root — the sidebar only renders \
             canonical Paths"
        );
    }

    #[tokio::test]
    async fn a_creator_with_no_path_leaves_the_room_unplaced_for_the_ui_fallback() {
        let db = fresh_in_memory_db().await;
        let scratch = crate::db::service::folder_service::add_chat_folder(
            &db.conn,
            "/tmp/codeg-room-chat-scratch",
        )
        .await
        .expect("chat scratch folder")
        .id;
        let repo = seed_folder(&db, "/tmp/codeg-room-repo").await;
        let creator = seed_conversation(&db, scratch, AgentType::ClaudeCode).await;
        let peer = seed_conversation(&db, repo, AgentType::Codex).await;

        let room = make_room(&db, creator, vec![creator, peer]).await;

        assert_eq!(
            room.root_folder_id, None,
            "a chat scratch folder owns no Path; guessing one would file the \
             Room under a Path it has nothing to do with, so the sidebar's \
             orphan bucket takes it instead"
        );
    }

    #[tokio::test]
    async fn prose_at_mentions_do_not_create_room_deliveries() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        let posted = crate::db::service::collaboration_service::post_room(
            &db.conn,
            PostRoomMessageInput {
                room_id: room.id.clone(),
                source_conversation_id: a,
                target_conversation_ids: vec![],
                mention_all: false,
                body: "hey @alice look at @/src/lib.rs and ping @user".into(),
                client_dedupe_id: "prose-at".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: Default::default(),
                expects_reply: false,
                urgency: Default::default(),
                reply_to_event_id: None,
                mention_human: false,
                author_kind: Default::default(),
            },
        )
        .await
        .expect("post");
        assert!(posted.deliveries.is_empty());

        let with_uri = crate::db::service::collaboration_service::post_room(
            &db.conn,
            PostRoomMessageInput {
                room_id: room.id.clone(),
                source_conversation_id: a,
                target_conversation_ids: vec![],
                mention_all: false,
                body: format!("please look codeg://session/{b}"),
                client_dedupe_id: "uri-at".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: Default::default(),
                expects_reply: false,
                urgency: Default::default(),
                reply_to_event_id: None,
                mention_human: false,
                author_kind: Default::default(),
            },
        )
        .await
        .expect("post uri");
        assert_eq!(with_uri.deliveries.len(), 1);
        assert_eq!(with_uri.deliveries[0].target.conversation_id, b);

        let human = crate::db::service::collaboration_service::post_room(
            &db.conn,
            PostRoomMessageInput {
                room_id: room.id.clone(),
                source_conversation_id: a,
                target_conversation_ids: vec![],
                mention_all: false,
                body: "human speaking codeg://human".into(),
                client_dedupe_id: "human-post".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: Default::default(),
                expects_reply: false,
                urgency: Default::default(),
                reply_to_event_id: None,
                mention_human: false,
                author_kind: crate::models::CollaborationAuthorKind::Human,
            },
        )
        .await
        .expect("human post");
        assert!(human.deliveries.is_empty());
        let events = timeline(&db.conn, &room.id, None).await.unwrap().events;
        let human_event = events
            .iter()
            .find(|event| event.body.contains("codeg://human"))
            .expect("human event");
        assert_eq!(
            human_event.author_kind,
            crate::models::CollaborationAuthorKind::Human
        );
        assert!(human_event.mention_human);
        assert_eq!(human_event.source.agent_type.as_deref(), Some("human"));
    }

    fn record_post(room_id: String, source: i32, dedupe: &str, body: &str) -> PostRoomMessageInput {
        PostRoomMessageInput {
            room_id,
            source_conversation_id: source,
            target_conversation_ids: vec![],
            mention_all: false,
            body: body.into(),
            client_dedupe_id: dedupe.into(),
            invocation_policy: CollaborationInvocationPolicy::StoreOnly,
            delivery_hint: Default::default(),
            expects_reply: false,
            urgency: Default::default(),
            reply_to_event_id: None,
            mention_human: false,
            author_kind: Default::default(),
        }
    }

    /// A post that parks a needs-reply obligation on every mentioned member.
    /// `StoreOnly` keeps the prompt queue out of it: the ledger, not the wake,
    /// is what these tests are about.
    fn debt_ask_post(
        room_id: String,
        source: i32,
        targets: Vec<i32>,
        dedupe: &str,
        body: &str,
    ) -> PostRoomMessageInput {
        PostRoomMessageInput {
            target_conversation_ids: targets,
            expects_reply: true,
            ..record_post(room_id, source, dedupe, body)
        }
    }

    #[tokio::test]
    async fn member_channel_unread_is_not_mailbox_and_catch_up_advances_cursor() {
        let (db, a, b, c) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        crate::db::service::collaboration_service::post_room(
            &db.conn,
            record_post(room.id.clone(), a, "catch-1", "first"),
        )
        .await
        .unwrap();
        crate::db::service::collaboration_service::post_room(
            &db.conn,
            record_post(room.id.clone(), a, "catch-2", "second"),
        )
        .await
        .unwrap();

        let for_b = list_for_member(&db.conn, b).await.unwrap();
        assert_eq!(for_b[0].unread_count, 2);
        assert_eq!(for_b[0].mention_unread_count, 0);
        assert!(list_inbox(
            &db.conn,
            b,
            crate::acp::session_collaboration::SessionMailboxScope::Inbox,
            crate::acp::session_collaboration::SessionInboxFilter::All,
            None,
            20,
        )
        .await
        .unwrap()
        .is_empty());
        assert_eq!(
            feed(&db.conn, b, None).await.unwrap().unread_count,
            0,
            "channel unread must not bump mailbox unread"
        );

        let window = timeline_with(
            &db.conn,
            &room.id,
            Some(50),
            RoomTimelineMode::Unread { conversation_id: b },
        )
        .await
        .unwrap();
        assert_eq!(window.events.len(), 2);
        assert_eq!(window.events[0].body, "first");
        assert_eq!(window.events[1].body, "second");
        let ids: Vec<String> = window.events.iter().map(|event| event.id.clone()).collect();
        consume_room_window(&db.conn, &room.id, b, &ids, true)
            .await
            .unwrap();
        assert_eq!(
            list_for_member(&db.conn, b).await.unwrap()[0].unread_count,
            0
        );

        add_members(
            &db.conn,
            AddCollaborationRoomMembersInput {
                room_id: room.id.clone(),
                conversation_ids: vec![c],
            },
        )
        .await
        .unwrap();
        assert_eq!(
            list_for_member(&db.conn, c).await.unwrap()[0].unread_count,
            0,
            "new members start caught up and do not inherit history"
        );
    }

    #[tokio::test]
    async fn room_mention_consume_does_not_clear_reply_debt() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        let posted = crate::db::service::collaboration_service::post_room(
            &db.conn,
            PostRoomMessageInput {
                room_id: room.id.clone(),
                source_conversation_id: a,
                target_conversation_ids: vec![b],
                mention_all: false,
                body: "please answer in the room".into(),
                client_dedupe_id: "mention-debt".into(),
                invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                delivery_hint: Default::default(),
                expects_reply: true,
                urgency: Default::default(),
                reply_to_event_id: None,
                mention_human: false,
                author_kind: Default::default(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            list_for_member(&db.conn, b).await.unwrap()[0].mention_unread_count,
            1
        );
        consume_room_window(
            &db.conn,
            &room.id,
            b,
            std::slice::from_ref(&posted.event_id),
            false,
        )
        .await
        .unwrap();
        assert_eq!(
            list_for_member(&db.conn, b).await.unwrap()[0].mention_unread_count,
            0
        );
        let delivery = crate::db::service::collaboration_service::get_inbound_message(
            &db.conn,
            b,
            &posted.event_id,
        )
        .await
        .unwrap();
        assert!(delivery.agent_received_at.is_some());
        assert_eq!(
            delivery.obligation_state,
            CollaborationObligationState::AwaitingReply
        );

        crate::db::service::collaboration_service::post_room(
            &db.conn,
            PostRoomMessageInput {
                room_id: room.id.clone(),
                source_conversation_id: b,
                target_conversation_ids: vec![],
                mention_all: false,
                body: "done".into(),
                client_dedupe_id: "mention-reply".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: Default::default(),
                expects_reply: false,
                urgency: Default::default(),
                reply_to_event_id: Some(posted.event_id.clone()),
                mention_human: false,
                author_kind: Default::default(),
            },
        )
        .await
        .unwrap();
        let closed = crate::db::service::collaboration_service::get_inbound_message(
            &db.conn,
            b,
            &posted.event_id,
        )
        .await
        .unwrap();
        assert_eq!(
            closed.obligation_state,
            CollaborationObligationState::Resolved
        );
    }

    /// Posting is the only way to clear a needs-reply debt, so the post itself
    /// has to say whether it did. Without that an Agent replies, sees nothing,
    /// and assumes it must `read_room` to audit its own ledger.
    #[tokio::test]
    async fn a_room_post_reports_the_debt_it_cleared_and_what_is_left() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        let asked = crate::db::service::collaboration_service::post_room(
            &db.conn,
            debt_ask_post(room.id.clone(), a, vec![b], "debt-ask", "please answer"),
        )
        .await
        .unwrap();
        assert!(asked.cleared_reply_to_event_id.is_none());
        assert_eq!(
            asked.open_reply_debt, 0,
            "asking parks the obligation on the target, never on the asker"
        );

        // An unlinked post pays nothing, and must say how much b still owes.
        let chatter = crate::db::service::collaboration_service::post_room(
            &db.conn,
            record_post(room.id.clone(), b, "debt-chatter", "looking into it"),
        )
        .await
        .unwrap();
        assert!(chatter.cleared_reply_to_event_id.is_none());
        assert_eq!(chatter.open_reply_debt, 1);

        let mut paid_input = record_post(room.id.clone(), b, "debt-paid", "done");
        paid_input.reply_to_event_id = Some(asked.event_id.clone());
        let paid = crate::db::service::collaboration_service::post_room(&db.conn, paid_input)
            .await
            .unwrap();
        assert_eq!(
            paid.cleared_reply_to_event_id.as_deref(),
            Some(asked.event_id.as_str())
        );
        assert_eq!(paid.open_reply_debt, 0);
        let settled = crate::db::service::collaboration_service::get_inbound_message(
            &db.conn,
            b,
            &asked.event_id,
        )
        .await
        .unwrap();
        assert_eq!(
            settled.obligation_state,
            CollaborationObligationState::Resolved
        );

        // Paying twice is not a second payment: the ledger is already clean,
        // so the follow-up must not claim it cleared anything.
        let mut again_input = record_post(room.id.clone(), b, "debt-again", "and one more thing");
        again_input.reply_to_event_id = Some(asked.event_id.clone());
        let again = crate::db::service::collaboration_service::post_room(&db.conn, again_input)
            .await
            .unwrap();
        assert!(again.cleared_reply_to_event_id.is_none());
        assert_eq!(again.open_reply_debt, 0);
    }

    /// `(expected, resolved)` for one post in a fetched window.
    fn reply_progress(events: &[RoomTimelineEvent], event_id: &str) -> (Option<u32>, Option<u32>) {
        let event = events
            .iter()
            .find(|event| event.id == event_id)
            .expect("event is in the window");
        (event.expected_reply_count, event.resolved_reply_count)
    }

    /// A post that `@`-ed N Sessions carries its own "M of N answered" ledger,
    /// because N asks are N independent Delivery rows. A post that asked
    /// nothing carries none — `None` is what stops the panel drawing "0/0".
    #[tokio::test]
    async fn timeline_reports_reply_progress_only_for_posts_that_asked() {
        let (db, a, b, c) = seeded().await;
        let room = make_room(&db, a, vec![a, b, c]).await;
        let asked = crate::db::service::collaboration_service::post_room(
            &db.conn,
            debt_ask_post(
                room.id.clone(),
                a,
                vec![b, c],
                "progress-ask",
                "who is taking this?",
            ),
        )
        .await
        .unwrap();
        crate::db::service::collaboration_service::post_room(
            &db.conn,
            record_post(room.id.clone(), a, "progress-chatter", "just a note"),
        )
        .await
        .unwrap();

        let events = timeline(&db.conn, &room.id, None).await.unwrap().events;
        assert_eq!(
            reply_progress(&events, &asked.event_id),
            (Some(2), Some(0)),
            "two Sessions were asked and neither has answered"
        );
        let plain = events
            .iter()
            .find(|event| event.body == "just a note")
            .expect("the plain post is in the window");
        assert_eq!(
            (plain.expected_reply_count, plain.resolved_reply_count),
            (None, None),
            "a post that asked nothing has no ledger to report"
        );

        // One answer moves the numerator; the ask itself stays on the board
        // until everyone it named has paid.
        let mut paid = record_post(room.id.clone(), b, "progress-paid", "on it");
        paid.reply_to_event_id = Some(asked.event_id.clone());
        crate::db::service::collaboration_service::post_room(&db.conn, paid)
            .await
            .unwrap();
        let events = timeline(&db.conn, &room.id, None).await.unwrap().events;
        assert_eq!(
            reply_progress(&events, &asked.event_id),
            (Some(2), Some(1)),
            "b answered, c has not"
        );
    }

    /// The obligation invariant, read from the progress side: "voided" lives
    /// in `delivery.state`, not in `obligation_state`, so a dismissed ask has
    /// to leave *both* halves of the fraction. Keeping it in the denominator
    /// would strand the post at "1/2 answered" with nobody left who could
    /// ever answer it.
    #[tokio::test]
    async fn dismissed_asks_leave_both_sides_of_the_reply_progress() {
        let (db, a, b, c) = seeded().await;
        let room = make_room(&db, a, vec![a, b, c]).await;
        let asked = crate::db::service::collaboration_service::post_room(
            &db.conn,
            debt_ask_post(
                room.id.clone(),
                a,
                vec![b, c],
                "dismiss-ask",
                "please answer",
            ),
        )
        .await
        .unwrap();
        let mut paid = record_post(room.id.clone(), b, "dismiss-paid", "done");
        paid.reply_to_event_id = Some(asked.event_id.clone());
        crate::db::service::collaboration_service::post_room(&db.conn, paid)
            .await
            .unwrap();

        let for_c = asked
            .deliveries
            .iter()
            .find(|delivery| delivery.target.conversation_id == c)
            .expect("c was asked");
        assert_eq!(
            for_c.state,
            CollaborationDeliveryState::Pending,
            "store_only asks land pending, which is what dismiss consumes"
        );
        crate::db::service::collaboration_service::dismiss(&db.conn, c, &for_c.id)
            .await
            .unwrap();

        let events = timeline(&db.conn, &room.id, None).await.unwrap().events;
        assert_eq!(
            reply_progress(&events, &asked.event_id),
            (Some(1), Some(1)),
            "c's dismissed ask is not an unanswered obligation"
        );
    }

    /// `@human` is a flag on the event, not a fan-out target, so an ask that
    /// only reaches the operator has no Delivery row behind it. The ledger
    /// still has to be `Some` — the post did ask — but with nothing to draw.
    #[tokio::test]
    async fn an_ask_with_no_delivery_row_reports_an_empty_ledger() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        let asked = crate::db::service::collaboration_service::post_room(
            &db.conn,
            PostRoomMessageInput {
                expects_reply: true,
                mention_human: true,
                ..record_post(room.id.clone(), a, "human-only-ask", "human, thoughts?")
            },
        )
        .await
        .unwrap();

        let events = timeline(&db.conn, &room.id, None).await.unwrap().events;
        assert_eq!(reply_progress(&events, &asked.event_id), (Some(0), Some(0)));
    }

    /// `reply_to_event_id` pointing outside the Room is rejected, and the
    /// rejection must not be mistaken for payment — the obligation stays open
    /// and the next honest reply is still able to clear it.
    #[tokio::test]
    async fn a_room_reply_to_a_foreign_event_is_rejected_and_keeps_the_debt() {
        let (db, a, b, c) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        let elsewhere = create(
            &db.conn,
            CreateCollaborationRoomInput {
                workbench_id: 1,
                title: "A and C".into(),
                member_conversation_ids: vec![a, c],
                created_by_conversation_id: a,
                collection_id: None,
                root_folder_id: None,
            },
        )
        .await
        .unwrap();
        let asked = crate::db::service::collaboration_service::post_room(
            &db.conn,
            debt_ask_post(room.id.clone(), a, vec![b], "foreign-ask", "please answer"),
        )
        .await
        .unwrap();
        let foreign = crate::db::service::collaboration_service::post_room(
            &db.conn,
            record_post(elsewhere.id.clone(), a, "foreign-post", "other room"),
        )
        .await
        .unwrap();

        let mut wrong = record_post(room.id.clone(), b, "foreign-reply", "done");
        wrong.reply_to_event_id = Some(foreign.event_id.clone());
        let err = crate::db::service::collaboration_service::post_room(&db.conn, wrong)
            .await
            .expect_err("a parent from another Room is not a reply here");
        assert!(err.to_string().contains("same Room"), "{err}");

        let still_owed = crate::db::service::collaboration_service::get_inbound_message(
            &db.conn,
            b,
            &asked.event_id,
        )
        .await
        .unwrap();
        assert_eq!(
            still_owed.obligation_state,
            CollaborationObligationState::AwaitingReply,
            "a rejected reply clears nothing"
        );
        assert_eq!(
            list_for_member(&db.conn, b).await.unwrap()[0].needs_reply_count,
            1
        );

        let mut right = record_post(room.id.clone(), b, "foreign-reply-fixed", "done");
        right.reply_to_event_id = Some(asked.event_id.clone());
        let paid = crate::db::service::collaboration_service::post_room(&db.conn, right)
            .await
            .unwrap();
        assert_eq!(
            paid.cleared_reply_to_event_id.as_deref(),
            Some(asked.event_id.as_str())
        );
        assert_eq!(paid.open_reply_debt, 0);
    }

    #[tokio::test]
    async fn recent_timeline_returns_newest_window_not_oldest() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        for index in 1..=3 {
            crate::db::service::collaboration_service::post_room(
                &db.conn,
                record_post(
                    room.id.clone(),
                    a,
                    &format!("win-{index}"),
                    &format!("n{index}"),
                ),
            )
            .await
            .unwrap();
        }
        let window = timeline_with(&db.conn, &room.id, Some(2), RoomTimelineMode::Recent)
            .await
            .unwrap();
        assert!(window.truncated);
        assert_eq!(
            window
                .events
                .iter()
                .map(|event| event.body.as_str())
                .collect::<Vec<_>>(),
            vec!["n2", "n3"]
        );
        let older = timeline_with(
            &db.conn,
            &room.id,
            Some(2),
            RoomTimelineMode::Before {
                event_id: &window.events[0].id,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            older
                .events
                .iter()
                .map(|event| event.body.as_str())
                .collect::<Vec<_>>(),
            vec!["n1"]
        );
    }

    #[tokio::test]
    async fn set_workbench_moves_an_active_room_and_leaves_members_alone() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        let second = workbench_service::create(&db.conn, Some("Review".into()))
            .await
            .expect("second workbench");
        let before_members: Vec<i32> = room.members.iter().map(|m| m.conversation_id).collect();

        let moved = set_workbench(&db.conn, &room.id, second.id)
            .await
            .expect("move");
        assert_eq!(moved.workbench_id, second.id);
        let after_members: Vec<i32> = moved.members.iter().map(|m| m.conversation_id).collect();
        assert_eq!(after_members, before_members);

        let on_main = list_for_workbench(&db.conn, 1).await.unwrap();
        assert!(
            on_main.iter().all(|r| r.id != room.id),
            "the Room must leave the source Workbench list"
        );
        let on_review = list_for_workbench(&db.conn, second.id).await.unwrap();
        assert_eq!(on_review.len(), 1);
        assert_eq!(on_review[0].id, room.id);
        assert_eq!(on_review[0].workbench_id, second.id);
    }

    #[tokio::test]
    async fn set_workbench_same_workbench_is_idempotent() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        let again = set_workbench(&db.conn, &room.id, room.workbench_id)
            .await
            .expect("noop");
        assert_eq!(again.id, room.id);
        assert_eq!(again.workbench_id, 1);
        assert_eq!(list_for_workbench(&db.conn, 1).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn set_workbench_rejects_a_missing_workbench() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        let err = set_workbench(&db.conn, &room.id, 9_999)
            .await
            .expect_err("missing workbench");
        assert!(matches!(err, DbError::NotFound(_)), "got {err:?}");
        assert_eq!(
            get(&db.conn, &room.id).await.unwrap().workbench_id,
            1,
            "a rejected move must not rewrite the Room"
        );
    }

    #[tokio::test]
    async fn set_workbench_rejects_a_non_active_room() {
        let (db, a, b, _) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        db.conn
            .execute(Statement::from_string(
                DbBackend::Sqlite,
                format!(
                    "UPDATE collaboration_room SET status = 'archived' WHERE id = '{}'",
                    room.id
                ),
            ))
            .await
            .unwrap();
        let err = set_workbench(&db.conn, &room.id, 1)
            .await
            .expect_err("archived");
        assert!(
            matches!(err, DbError::Validation(_)),
            "non-active must be an explicit reject, got {err:?}"
        );
        assert!(err.to_string().contains("not active"));
    }

    #[tokio::test]
    async fn set_workbench_rejects_a_missing_room() {
        let (db, _, _, _) = seeded().await;
        let err = set_workbench(&db.conn, "rm_does-not-exist", 1)
            .await
            .expect_err("missing room");
        assert!(matches!(err, DbError::NotFound(_)), "got {err:?}");
    }
}
