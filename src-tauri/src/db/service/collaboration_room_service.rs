use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, QueryResult, Statement,
    TransactionTrait,
};

use crate::db::error::DbError;
use crate::models::{
    AddCollaborationRoomMembersInput, CollaborationRoomDetail, CollaborationRoomMember,
    CollaborationRoomSummary, CollaborationSessionSnapshot, CollaborationUrgency,
    CreateCollaborationRoomInput, RoomTimeline, RoomTimelineEvent,
};

const MAX_TITLE_CHARS: usize = 80;
const MAX_MEMBERS: usize = 32;
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
    Ok((None, None))
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
    if unique.len() < 2 {
        return Err(validation("A Room requires at least two Session members"));
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
/// record-only posts that never created a Delivery.
const MEMBER_CHANNEL_UNREAD_SQL: &str = "(SELECT COUNT(*) FROM collaboration_event e \
                      WHERE e.room_id = r.id AND COALESCE(e.visibility, 'direct') = 'room' \
                        AND e.source_conversation_id != me.conversation_id \
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
const MEMBER_AWAITING_REPLY_SQL: &str = "(SELECT COUNT(*) FROM collaboration_delivery d \
                      JOIN collaboration_event e ON e.id = d.event_id \
                      WHERE e.room_id = r.id \
                        AND e.source_conversation_id = me.conversation_id \
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
                room_summary_select(HOST_CHANNEL_UNREAD_SQL, "0", "0", "0")
            ),
            vec![workbench_id.into()],
        ))
        .await?;
    rows.iter().map(summary_from_row).collect()
}

pub async fn list(
    conn: &DatabaseConnection,
    workbench_id: i32,
) -> Result<Vec<CollaborationRoomSummary>, DbError> {
    list_for_workbench(conn, workbench_id).await
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
    Ok(CollaborationRoomDetail {
        id: row.try_get("", "id")?,
        workbench_id: row.try_get("", "workbench_id")?,
        title: row.try_get("", "title")?,
        created_by_conversation_id: row.try_get("", "created_by_conversation_id")?,
        collection_id: row.try_get("", "collection_id")?,
        root_folder_id: row.try_get("", "root_folder_id")?,
        members,
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
    if remaining <= 2 {
        return Err(validation("A Room requires at least two Session members"));
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
        RoomTimelineMode::Unread { conversation_id } => {
            require_member(conn, room_id, conversation_id).await?;
            let rows = conn
                .query_all(statement(
                    &format!(
                        "{EVENT_SELECT} \
                     JOIN collaboration_room_member me \
                       ON me.room_id = e.room_id AND me.conversation_id = ? \
                     WHERE e.room_id = ? AND COALESCE(e.visibility, 'direct') = 'room' \
                       AND e.source_conversation_id != me.conversation_id \
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
    Ok(RoomTimeline {
        room_id: room_id.to_string(),
        events,
        truncated,
    })
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
        created_at: parse_timestamp(row, "created_at")?,
    })
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
    async fn create_room_requires_two_live_members_and_lists_on_workbench() {
        let (db, a, b, _) = seeded().await;
        let err = create(
            &db.conn,
            CreateCollaborationRoomInput {
                workbench_id: 1,
                title: "Solo".into(),
                member_conversation_ids: vec![a],
                created_by_conversation_id: a,
                collection_id: None,
                root_folder_id: None,
            },
        )
        .await
        .expect_err("one member");
        assert!(err.to_string().contains("at least two"));

        let room = make_room(&db, a, vec![a, b]).await;
        assert_eq!(room.members.len(), 2);
        assert_eq!(room.created_by_conversation_id, a);
        assert!(room.members.iter().all(|m| m.role == "member"));
        let listed = list(&db.conn, 1).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, room.id);
        assert_eq!(listed[0].member_count, 2);
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
        assert!(list(&db.conn, 1).await.unwrap().is_empty());

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
    async fn archived_member_stays_mentioned_but_is_not_enqueued() {
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
        assert_eq!(
            archived.invocation_policy,
            CollaborationInvocationPolicy::StoreOnly
        );
        assert_eq!(archived.state, CollaborationDeliveryState::Pending);
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
        let listed = list(&db.conn, 1).await.unwrap();
        assert_eq!(listed[0].unread_count, 1);
        let updated_before = get(&db.conn, &room.id).await.unwrap().updated_at;
        mark_seen(&db.conn, &room.id, Some(a)).await.unwrap();
        let listed = list(&db.conn, 1).await.unwrap();
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
            .expect("creator Session can leave while two members remain");
        let after = get(&db.conn, &room.id).await.unwrap();
        assert_eq!(after.members.len(), 2);
        assert!(after
            .members
            .iter()
            .all(|member| member.conversation_id != a));
        let err = remove_member(&db.conn, &room.id, c)
            .await
            .expect_err("floor is two members");
        assert!(err.to_string().contains("at least two"));
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

        let host = list(&db.conn, 1).await.unwrap();
        assert_eq!(host[0].needs_reply_count, 0);
        assert_eq!(host[0].awaiting_reply_count, 0);

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
            not_the_creators_slot.root_folder_id, None,
            "a Room does not inherit the creator Session folder"
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
}
