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

    let room_id = format!("rm_{}", uuid::Uuid::new_v4());
    let txn = conn.begin().await?;
    txn.execute(statement(
        "INSERT INTO collaboration_room \
         (id, workbench_id, title, status, created_by_conversation_id, created_at, updated_at) \
         VALUES (?, ?, ?, 'active', ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        vec![
            room_id.clone().into(),
            input.workbench_id.into(),
            title.into(),
            input.created_by_conversation_id.into(),
        ],
    ))
    .await?;
    for conversation_id in member_ids {
        let role = if conversation_id == input.created_by_conversation_id {
            "owner"
        } else {
            "member"
        };
        txn.execute(statement(
            "INSERT INTO collaboration_room_member \
             (room_id, conversation_id, role, joined_at) \
             VALUES (?, ?, ?, CURRENT_TIMESTAMP)",
            vec![room_id.clone().into(), conversation_id.into(), role.into()],
        ))
        .await?;
    }
    txn.commit().await?;
    get(conn, &room_id).await
}

pub async fn list(
    conn: &DatabaseConnection,
    workbench_id: i32,
) -> Result<Vec<CollaborationRoomSummary>, DbError> {
    require_workbench(conn, workbench_id).await?;
    let rows = conn
        .query_all(statement(
            "SELECT r.id, r.workbench_id, r.title, r.created_by_conversation_id, \
                    r.created_at, r.updated_at, r.last_seen_at, \
                    (SELECT COUNT(*) FROM collaboration_room_member m \
                      WHERE m.room_id = r.id) AS member_count, \
                    (SELECT MAX(e.created_at) FROM collaboration_event e \
                      WHERE e.room_id = r.id AND COALESCE(e.visibility, 'direct') = 'room') \
                      AS last_event_at, \
                    (SELECT COUNT(*) FROM collaboration_event e \
                      WHERE e.room_id = r.id AND COALESCE(e.visibility, 'direct') = 'room' \
                        AND (r.last_seen_at IS NULL OR datetime(e.created_at) > datetime(r.last_seen_at))) \
                      AS unread_count \
             FROM collaboration_room r \
             WHERE r.workbench_id = ? AND r.status = 'active' \
             ORDER BY datetime(COALESCE(last_event_at, r.updated_at)) DESC, r.id DESC",
            vec![workbench_id.into()],
        ))
        .await?;
    rows.iter().map(summary_from_row).collect()
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
            "SELECT r.id, r.workbench_id, r.title, r.created_by_conversation_id, \
                    r.created_at, r.updated_at, r.last_seen_at, \
                    (SELECT COUNT(*) FROM collaboration_room_member m \
                      WHERE m.room_id = r.id) AS member_count, \
                    (SELECT MAX(e.created_at) FROM collaboration_event e \
                      WHERE e.room_id = r.id AND COALESCE(e.visibility, 'direct') = 'room') \
                      AS last_event_at, \
                    (SELECT COUNT(*) FROM collaboration_event e \
                      WHERE e.room_id = r.id AND COALESCE(e.visibility, 'direct') = 'room' \
                        AND (r.last_seen_at IS NULL OR datetime(e.created_at) > datetime(r.last_seen_at))) \
                      AS unread_count \
             FROM collaboration_room r \
             JOIN collaboration_room_member me \
               ON me.room_id = r.id AND me.conversation_id = ? \
             WHERE r.status = 'active' \
             ORDER BY datetime(COALESCE(last_event_at, r.updated_at)) DESC, r.id DESC",
            vec![conversation_id.into()],
        ))
        .await?;
    rows.iter().map(summary_from_row).collect()
}

fn summary_from_row(row: &QueryResult) -> Result<CollaborationRoomSummary, DbError> {
    let member_count: i64 = row.try_get("", "member_count")?;
    let unread_count: i64 = row.try_get("", "unread_count")?;
    Ok(CollaborationRoomSummary {
        id: row.try_get("", "id")?,
        workbench_id: row.try_get("", "workbench_id")?,
        title: row.try_get("", "title")?,
        created_by_conversation_id: row.try_get("", "created_by_conversation_id")?,
        member_count: u32::try_from(member_count.max(0)).unwrap_or(u32::MAX),
        unread_count: u32::try_from(unread_count.max(0)).unwrap_or(u32::MAX),
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
            "SELECT id, workbench_id, title, created_by_conversation_id, created_at, updated_at \
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
             ORDER BY CASE m.role WHEN 'owner' THEN 0 ELSE 1 END, m.conversation_id",
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
             (room_id, conversation_id, role, joined_at) \
             VALUES (?, ?, 'member', CURRENT_TIMESTAMP)",
            vec![input.room_id.clone().into(), conversation_id.into()],
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
    let row = conn
        .query_one(statement(
            "SELECT role FROM collaboration_room_member \
             WHERE room_id = ? AND conversation_id = ?",
            vec![room_id.into(), conversation_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            DbError::NotFound(format!(
                "Session {conversation_id} is not a member of Room {room_id}"
            ))
        })?;
    let role: String = row.try_get("", "role")?;
    if role == "owner" {
        let owners: i64 = conn
            .query_one(statement(
                "SELECT COUNT(*) AS count FROM collaboration_room_member \
                 WHERE room_id = ? AND role = 'owner'",
                vec![room_id.into()],
            ))
            .await?
            .expect("COUNT")
            .try_get("", "count")?;
        if owners <= 1 {
            return Err(validation("The last Room owner cannot be removed"));
        }
    }
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

pub async fn mark_seen(
    conn: &DatabaseConnection,
    room_id: &str,
    conversation_id: Option<i32>,
) -> Result<CollaborationRoomDetail, DbError> {
    let _ = room_workbench_id(conn, room_id).await?;
    let txn = conn.begin().await?;
    txn.execute(statement(
        "UPDATE collaboration_room \
         SET last_seen_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP \
         WHERE id = ?",
        vec![room_id.into()],
    ))
    .await?;
    if let Some(conversation_id) = conversation_id {
        require_member(&txn, room_id, conversation_id).await?;
        txn.execute(statement(
            "UPDATE collaboration_room_member \
             SET last_read_at = CURRENT_TIMESTAMP \
             WHERE room_id = ? AND conversation_id = ?",
            vec![room_id.into(), conversation_id.into()],
        ))
        .await?;
    }
    txn.commit().await?;
    get(conn, room_id).await
}

pub async fn timeline(
    conn: &DatabaseConnection,
    room_id: &str,
    limit: Option<u32>,
) -> Result<RoomTimeline, DbError> {
    let _ = room_workbench_id(conn, room_id).await?;
    let limit = limit
        .unwrap_or(DEFAULT_TIMELINE_LIMIT)
        .clamp(1, MAX_TIMELINE_LIMIT) as i64;
    let rows = conn
        .query_all(statement(
            "SELECT e.id, e.room_id, e.source_conversation_id, e.source_title_snapshot, \
                    e.source_agent_type_snapshot, e.source_folder_path_snapshot, \
                    e.subject, e.body, e.reply_to_event_id, e.expects_reply, e.urgency, \
                    e.created_at \
             FROM collaboration_event e \
             WHERE e.room_id = ? AND COALESCE(e.visibility, 'direct') = 'room' \
             ORDER BY datetime(e.created_at) ASC, e.id ASC \
             LIMIT ?",
            vec![room_id.into(), limit.into()],
        ))
        .await?;
    let mut events = Vec::with_capacity(rows.len());
    for row in rows {
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
        events.push(RoomTimelineEvent {
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
            created_at: parse_timestamp(&row, "created_at")?,
        });
    }
    Ok(RoomTimeline {
        room_id: room_id.to_string(),
        events,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::service::collaboration_service::{feed, list_inbox, send};
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::AgentType;
    use crate::models::{
        CollaborationInvocationPolicy, CollaborationObligationState, PostRoomMessageInput,
        SendCollaborationMessageInput,
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
            },
        )
        .await
        .expect_err("one member");
        assert!(err.to_string().contains("at least two"));

        let room = make_room(&db, a, vec![a, b]).await;
        assert_eq!(room.members.len(), 2);
        assert_eq!(room.created_by_conversation_id, a);
        assert!(room
            .members
            .iter()
            .any(|m| m.role == "owner" && m.conversation_id == a));
        let listed = list(&db.conn, 1).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, room.id);
        assert_eq!(listed[0].member_count, 2);
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
                subject: "Note".into(),
                body: "record only".into(),
                client_dedupe_id: "room-record-1".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: Default::default(),
                expects_reply: false,
                urgency: Default::default(),
                reply_to_event_id: None,
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
                subject: "Please look".into(),
                body: "check this".into(),
                client_dedupe_id: "room-at-1".into(),
                invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                delivery_hint: Default::default(),
                expects_reply: true,
                urgency: Default::default(),
                reply_to_event_id: None,
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
                subject: "Ask B".into(),
                body: "please reply in room".into(),
                client_dedupe_id: "room-root".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: Default::default(),
                expects_reply: true,
                urgency: Default::default(),
                reply_to_event_id: None,
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
                subject: "Re".into(),
                body: "here is the room answer".into(),
                client_dedupe_id: "room-reply".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: Default::default(),
                expects_reply: false,
                urgency: Default::default(),
                reply_to_event_id: Some(root.event_id.clone()),
            },
        )
        .await
        .unwrap();
        assert!(reply.deliveries.is_empty());
        let events = timeline(&db.conn, &room.id, None).await.unwrap().events;
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[1].reply_to_event_id.as_deref(),
            Some(root.event_id.as_str())
        );
        assert_eq!(events[1].body, "here is the room answer");

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
    async fn mark_seen_clears_room_unread_and_owner_cannot_leave_last() {
        let (db, a, b, c) = seeded().await;
        let room = make_room(&db, a, vec![a, b]).await;
        crate::db::service::collaboration_service::post_room(
            &db.conn,
            PostRoomMessageInput {
                room_id: room.id.clone(),
                source_conversation_id: a,
                target_conversation_ids: vec![],
                mention_all: false,
                subject: "Note".into(),
                body: "hello room".into(),
                client_dedupe_id: "seen-1".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: Default::default(),
                expects_reply: false,
                urgency: Default::default(),
                reply_to_event_id: None,
            },
        )
        .await
        .unwrap();
        let listed = list(&db.conn, 1).await.unwrap();
        assert_eq!(listed[0].unread_count, 1);
        mark_seen(&db.conn, &room.id, Some(a)).await.unwrap();
        let listed = list(&db.conn, 1).await.unwrap();
        assert_eq!(listed[0].unread_count, 0);

        add_members(
            &db.conn,
            AddCollaborationRoomMembersInput {
                room_id: room.id.clone(),
                conversation_ids: vec![c],
            },
        )
        .await
        .unwrap();
        let err = remove_member(&db.conn, &room.id, a)
            .await
            .expect_err("last owner");
        assert!(err.to_string().contains("last Room owner"));
        remove_member(&db.conn, &room.id, c)
            .await
            .expect("remove extra");
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
}
