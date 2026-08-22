use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, QueryFilter, QueryOrder, Set, TransactionTrait,
};
use std::sync::OnceLock;
use tokio::sync::Mutex;

use crate::db::entities::{opened_tab, workbench};
use crate::db::error::DbError;
use crate::db::service::app_metadata_service;
use crate::models::agent::AgentType;
use crate::models::OpenedTab;

/// Serializes all version-mutating tab operations within the process so the
/// logical clock advances strictly sequentially. Two concurrent writers would
/// otherwise both read the version and then both try to bump it; under SQLite
/// WAL the loser fails with a snapshot/busy error instead of cleanly observing
/// the new version and returning `accepted: false`. Tab mutations are tiny and
/// client-side debounced, so serializing them costs nothing — this is a
/// correctness lock on a shared counter, not a throughput cap.
fn version_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Workspace-global logical clock for the open-tab set, stored in the
/// `app_metadata` KV table (survives restart, stays monotonic). Bumped on every
/// accepted mutation; used for compare-and-set (lost-update prevention) and for
/// client-side echo/ordering on the `tabs://changed` side-channel.
const OPENED_TABS_VERSION_KEY: &str = "opened_tabs_version";

/// Outcome of a compare-and-set tab save.
pub struct CasOutcome {
    /// Whether the write was applied (the version matched).
    pub accepted: bool,
    /// Authoritative version after the call (incremented iff `accepted`).
    pub version: i64,
    /// Canonical persisted tab set after the call.
    pub tabs: Vec<OpenedTab>,
}

fn parse_agent_type(s: &str) -> Option<AgentType> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
}

/// Read the current tab-set version (0 when never written).
pub async fn get_tabs_version<C: ConnectionTrait>(conn: &C) -> Result<i64, DbError> {
    let raw = app_metadata_service::get_value_conn(conn, OPENED_TABS_VERSION_KEY).await?;
    Ok(raw.and_then(|s| s.parse::<i64>().ok()).unwrap_or(0))
}

pub async fn list_all_tabs<C: ConnectionTrait>(conn: &C) -> Result<Vec<OpenedTab>, DbError> {
    list_tabs_for_workbench(conn, 1).await
}

/// List the persisted conversation tabs belonging to one logical workbench.
pub async fn list_tabs_for_workbench<C: ConnectionTrait>(
    conn: &C,
    workbench_id: i32,
) -> Result<Vec<OpenedTab>, DbError> {
    let rows = opened_tab::Entity::find()
        .filter(opened_tab::Column::WorkbenchId.eq(workbench_id))
        .order_by_asc(opened_tab::Column::Position)
        .all(conn)
        .await?;

    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let agent_type = parse_agent_type(&r.agent_type)?;
            Some(OpenedTab {
                id: r.id,
                folder_id: r.folder_id,
                conversation_id: r.conversation_id,
                room_id: r.room_id,
                board_scope: r.board_scope,
                agent_type,
                position: r.position,
                is_active: r.is_active,
                is_pinned: r.is_pinned,
            })
        })
        .collect())
}

/// Read the tab set and its version in a single transaction so a concurrent
/// save can't tear the pair — returning old tabs stamped with the new version,
/// which a client could then CAS-save as if it were current, dropping the
/// concurrent change.
pub async fn snapshot_tabs(conn: &DatabaseConnection) -> Result<(Vec<OpenedTab>, i64), DbError> {
    snapshot_tabs_for_workbench(conn, 1).await
}

pub async fn snapshot_tabs_for_workbench(
    conn: &DatabaseConnection,
    workbench_id: i32,
) -> Result<(Vec<OpenedTab>, i64), DbError> {
    let txn = conn.begin().await?;
    ensure_workbench_exists(&txn, workbench_id).await?;
    let tabs = list_tabs_for_workbench(&txn, workbench_id).await?;
    let version = get_tabs_version(&txn).await?;
    txn.commit().await?;
    Ok((tabs, version))
}

/// Replace all tabs with the given list (full replacement).
///
/// Draft tabs (`conversation_id`, `room_id`, and `board_scope` all empty) are **never
/// persisted** — a draft is a device-local working surface (volatile id,
/// provisional agent, live ACP connection) and must not leak across clients
/// via this shared table. Room and Board tabs persist by their own stable
/// identities. This is the single persistence
/// chokepoint, so the invariant holds for every caller.
///
/// Ensures at most one `is_active = true` (first active wins; others forced
/// false). `is_active` marks the focused tab and is mirrored across clients
/// (see `tab-context.tsx`).
pub async fn save_all_tabs<C: ConnectionTrait>(
    conn: &C,
    items: Vec<OpenedTab>,
) -> Result<(), DbError> {
    save_tabs_for_workbench(conn, 1, items).await
}

pub async fn save_tabs_for_workbench<C: ConnectionTrait>(
    conn: &C,
    workbench_id: i32,
    items: Vec<OpenedTab>,
) -> Result<(), DbError> {
    opened_tab::Entity::delete_many()
        .filter(opened_tab::Column::WorkbenchId.eq(workbench_id))
        .exec(conn)
        .await?;

    let now = Utc::now();
    let mut active_seen = false;

    for item in items {
        let board_scope = item
            .board_scope
            .as_ref()
            .map(|scope| scope.trim())
            .filter(|scope| !scope.is_empty())
            .map(|scope| scope.to_string());
        let room_id = item
            .room_id
            .as_ref()
            .map(|id| id.trim())
            .filter(|id| !id.is_empty())
            .map(|id| id.to_string());
        // Skip drafts — never persist a tab with no stable content identity.
        if item.conversation_id.is_none() && room_id.is_none() && board_scope.is_none() {
            continue;
        }
        let room_id = if board_scope.is_some() { None } else { room_id };
        let conversation_id = if room_id.is_some() || board_scope.is_some() {
            None
        } else {
            item.conversation_id
        };

        let agent_str = serde_json::to_value(item.agent_type)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_default();

        let is_active = if item.is_active && !active_seen {
            active_seen = true;
            true
        } else {
            false
        };

        let active = opened_tab::ActiveModel {
            id: NotSet,
            workbench_id: Set(workbench_id),
            folder_id: Set(item.folder_id),
            conversation_id: Set(conversation_id),
            room_id: Set(room_id),
            board_scope: Set(board_scope),
            agent_type: Set(agent_str),
            position: Set(item.position),
            is_active: Set(is_active),
            is_pinned: Set(item.is_pinned),
            created_at: Set(now),
            updated_at: Set(now),
        };
        active.insert(conn).await?;
    }

    Ok(())
}

/// Compare-and-set save: only writes when `expected_version` matches the stored
/// version, then bumps it. A stale save (built from an older version because
/// another client committed first) is rejected without touching the table — the
/// caller is handed the current truth (`accepted: false`) to reconcile against.
/// The read-check-write runs in a single transaction so it is atomic.
pub async fn save_all_tabs_cas(
    conn: &DatabaseConnection,
    items: Vec<OpenedTab>,
    expected_version: i64,
) -> Result<CasOutcome, DbError> {
    save_tabs_cas_for_workbench(conn, 1, items, expected_version).await
}

pub async fn save_tabs_cas_for_workbench(
    conn: &DatabaseConnection,
    workbench_id: i32,
    items: Vec<OpenedTab>,
    expected_version: i64,
) -> Result<CasOutcome, DbError> {
    let _guard = version_lock().lock().await;
    let txn = conn.begin().await?;
    ensure_workbench_exists(&txn, workbench_id).await?;

    let current = get_tabs_version(&txn).await?;
    if current != expected_version {
        let tabs = list_tabs_for_workbench(&txn, workbench_id).await?;
        txn.commit().await?;
        return Ok(CasOutcome {
            accepted: false,
            version: current,
            tabs,
        });
    }

    save_tabs_for_workbench(&txn, workbench_id, items).await?;
    let next = current + 1;
    app_metadata_service::upsert_value(&txn, OPENED_TABS_VERSION_KEY, &next.to_string()).await?;
    let tabs = list_tabs_for_workbench(&txn, workbench_id).await?;
    txn.commit().await?;

    Ok(CasOutcome {
        accepted: true,
        version: next,
        tabs,
    })
}

async fn ensure_workbench_exists<C: ConnectionTrait>(
    conn: &C,
    workbench_id: i32,
) -> Result<(), DbError> {
    if workbench::Entity::find_by_id(workbench_id)
        .one(conn)
        .await?
        .is_some()
    {
        Ok(())
    } else {
        Err(DbError::NotFound(format!("Workbench {workbench_id}")))
    }
}

/// Outcome of a server-side tab invalidation (conversation/folder deletion).
///
/// The `version` is ALWAYS advanced — it acts as a *barrier*: a concurrent stale
/// `save_all_tabs_cas` built before the deletion (and possibly re-adding a tab
/// for the now-deleted entity) no longer matches the version and is rejected,
/// forcing it to reconcile. `emit` carries the new snapshot to broadcast ONLY
/// when a persisted row actually changed; a zero-row invalidation needs no
/// broadcast — any client still holding that tab is mid-debounce and reconciles
/// via its rejected save, and clients with a persisted tab are the rows-removed
/// case.
pub struct TabInvalidation {
    pub version: i64,
    pub emit: Option<Vec<OpenedTab>>,
}

/// Atomically invalidate every tab pointing at a conversation: delete the rows,
/// ALWAYS bump the version (barrier), and snapshot — all in ONE transaction
/// under [`version_lock`]. Conversation deletion is a SOFT delete (sets
/// `deleted_at`), so the FK `ON DELETE CASCADE` never fires; without explicit
/// cleanup the tab row survives and resurrects as a ghost. Doing delete + bump +
/// snapshot atomically AND serialized means a concurrent stale `save_all_tabs_cas`
/// either ran first (its row is deleted here) or is rejected by the bumped
/// version — it can never persist a tab for the deleted conversation.
pub async fn delete_conversation_tabs_and_bump(
    conn: &DatabaseConnection,
    conversation_id: i32,
) -> Result<TabInvalidation, DbError> {
    let _guard = version_lock().lock().await;
    let txn = conn.begin().await?;
    let removed = opened_tab::Entity::delete_many()
        .filter(opened_tab::Column::ConversationId.eq(conversation_id))
        .exec(&txn)
        .await?;
    let next = get_tabs_version(&txn).await? + 1;
    app_metadata_service::upsert_value(&txn, OPENED_TABS_VERSION_KEY, &next.to_string()).await?;
    let emit = if removed.rows_affected > 0 {
        Some(list_all_tabs(&txn).await?)
    } else {
        None
    };
    txn.commit().await?;
    Ok(TabInvalidation {
        version: next,
        emit,
    })
}

/// Remove persisted Room tabs after the Room itself is deleted, and bump the
/// tab version so a stale save cannot resurrect them.
pub async fn delete_room_tabs_and_bump(
    conn: &DatabaseConnection,
    room_id: &str,
) -> Result<TabInvalidation, DbError> {
    let _guard = version_lock().lock().await;
    let txn = conn.begin().await?;
    let removed = opened_tab::Entity::delete_many()
        .filter(opened_tab::Column::RoomId.eq(room_id))
        .exec(&txn)
        .await?;
    let next = get_tabs_version(&txn).await? + 1;
    app_metadata_service::upsert_value(&txn, OPENED_TABS_VERSION_KEY, &next.to_string()).await?;
    let emit = if removed.rows_affected > 0 {
        Some(list_all_tabs(&txn).await?)
    } else {
        None
    };
    txn.commit().await?;
    Ok(TabInvalidation {
        version: next,
        emit,
    })
}

/// Atomically invalidate every tab belonging to a folder (folder removed from
/// the workspace): delete the rows, ALWAYS bump the version (barrier), snapshot —
/// one transaction under [`version_lock`], same race-free guarantee as
/// [`delete_conversation_tabs_and_bump`].
pub async fn delete_folder_tabs_and_bump(
    conn: &DatabaseConnection,
    folder_id: i32,
) -> Result<TabInvalidation, DbError> {
    let _guard = version_lock().lock().await;
    let txn = conn.begin().await?;
    let removed = opened_tab::Entity::delete_many()
        .filter(opened_tab::Column::FolderId.eq(folder_id))
        .filter(opened_tab::Column::RoomId.is_null())
        .filter(opened_tab::Column::BoardScope.is_null())
        .exec(&txn)
        .await?;
    let next = get_tabs_version(&txn).await? + 1;
    app_metadata_service::upsert_value(&txn, OPENED_TABS_VERSION_KEY, &next.to_string()).await?;
    let emit = if removed.rows_affected > 0 {
        Some(list_all_tabs(&txn).await?)
    } else {
        None
    };
    txn.commit().await?;
    Ok(TabInvalidation {
        version: next,
        emit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::service::collaboration_room_service;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::{AgentType, CreateCollaborationRoomInput, OpenedTab};

    fn conv_tab(folder_id: i32, conversation_id: i32) -> OpenedTab {
        OpenedTab {
            id: 0,
            folder_id,
            conversation_id: Some(conversation_id),
            room_id: None,
            board_scope: None,
            agent_type: AgentType::Codex,
            position: 0,
            is_active: false,
            is_pinned: true,
        }
    }

    fn room_tab(folder_id: i32, room_id: &str) -> OpenedTab {
        OpenedTab {
            id: 0,
            folder_id,
            conversation_id: None,
            room_id: Some(room_id.to_string()),
            board_scope: None,
            agent_type: AgentType::ClaudeCode,
            position: 1,
            is_active: true,
            is_pinned: true,
        }
    }

    fn board_tab(folder_id: i32, scope: &str) -> OpenedTab {
        OpenedTab {
            id: 0,
            folder_id,
            conversation_id: None,
            room_id: None,
            board_scope: Some(scope.to_string()),
            agent_type: AgentType::ClaudeCode,
            position: 2,
            is_active: false,
            is_pinned: true,
        }
    }

    #[tokio::test]
    async fn persists_board_scope_and_folder_close_does_not_drop_the_view() {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/codeg-board-tabs").await;
        let session_id = seed_conversation(&db, folder_id, AgentType::Codex).await;
        save_tabs_for_workbench(
            &db.conn,
            1,
            vec![
                conv_tab(folder_id, session_id),
                board_tab(folder_id, &format!("project:{folder_id}")),
            ],
        )
        .await
        .expect("save");

        let listed = list_tabs_for_workbench(&db.conn, 1).await.expect("list");
        assert_eq!(listed.len(), 2);
        let expected_project_scope = format!("project:{folder_id}");
        assert_eq!(
            listed[1].board_scope.as_deref(),
            Some(expected_project_scope.as_str())
        );
        assert_eq!(listed[1].conversation_id, None);

        delete_folder_tabs_and_bump(&db.conn, folder_id)
            .await
            .expect("close folder tabs");
        let remaining = list_tabs_for_workbench(&db.conn, 1).await.expect("list");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].board_scope, listed[1].board_scope);
    }

    #[tokio::test]
    async fn persists_room_tabs_and_still_drops_drafts() {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/codeg-room-tabs").await;
        let a = seed_conversation(&db, folder_id, AgentType::Codex).await;
        let b = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let room = collaboration_room_service::create(
            &db.conn,
            CreateCollaborationRoomInput {
                workbench_id: 1,
                title: "Plan".into(),
                member_conversation_ids: vec![a, b],
                created_by_conversation_id: a,
                collection_id: None,
                root_folder_id: None,
            },
        )
        .await
        .expect("create room");

        save_tabs_for_workbench(
            &db.conn,
            1,
            vec![
                conv_tab(folder_id, a),
                room_tab(folder_id, &room.id),
                OpenedTab {
                    id: 0,
                    folder_id,
                    conversation_id: None,
                    room_id: None,
                    board_scope: None,
                    agent_type: AgentType::Gemini,
                    position: 2,
                    is_active: false,
                    is_pinned: true,
                },
            ],
        )
        .await
        .expect("save");

        let listed = list_tabs_for_workbench(&db.conn, 1).await.expect("list");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].conversation_id, Some(a));
        assert_eq!(listed[0].room_id, None);
        assert_eq!(listed[1].room_id.as_deref(), Some(room.id.as_str()));
        assert_eq!(listed[1].conversation_id, None);
        assert!(listed[1].is_active);
    }

    #[tokio::test]
    async fn folder_invalidation_keeps_room_tabs() {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/codeg-room-tabs-folder").await;
        let a = seed_conversation(&db, folder_id, AgentType::Codex).await;
        let b = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let room = collaboration_room_service::create(
            &db.conn,
            CreateCollaborationRoomInput {
                workbench_id: 1,
                title: "Plan".into(),
                member_conversation_ids: vec![a, b],
                created_by_conversation_id: a,
                collection_id: None,
                root_folder_id: None,
            },
        )
        .await
        .expect("create room");

        save_tabs_for_workbench(
            &db.conn,
            1,
            vec![conv_tab(folder_id, a), room_tab(folder_id, &room.id)],
        )
        .await
        .expect("save");

        delete_folder_tabs_and_bump(&db.conn, folder_id)
            .await
            .expect("invalidate folder");
        let listed = list_tabs_for_workbench(&db.conn, 1).await.expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].room_id.as_deref(), Some(room.id.as_str()));
    }

    #[tokio::test]
    async fn deleting_a_room_drops_its_tabs_and_bumps_version() {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/codeg-room-tabs-delete").await;
        let a = seed_conversation(&db, folder_id, AgentType::Codex).await;
        let b = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let room = collaboration_room_service::create(
            &db.conn,
            CreateCollaborationRoomInput {
                workbench_id: 1,
                title: "Plan".into(),
                member_conversation_ids: vec![a, b],
                created_by_conversation_id: a,
                collection_id: None,
                root_folder_id: None,
            },
        )
        .await
        .expect("create room");

        save_tabs_for_workbench(
            &db.conn,
            1,
            vec![conv_tab(folder_id, a), room_tab(folder_id, &room.id)],
        )
        .await
        .expect("save");
        let before = get_tabs_version(&db.conn).await.expect("version");

        let inv = delete_room_tabs_and_bump(&db.conn, &room.id)
            .await
            .expect("invalidate room tabs");
        assert!(inv.version > before);
        assert!(inv.emit.is_some());
        let listed = list_tabs_for_workbench(&db.conn, 1).await.expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].conversation_id, Some(a));
        assert_eq!(listed[0].room_id, None);
    }
}
