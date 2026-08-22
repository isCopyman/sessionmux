use chrono::Utc;
use sea_orm::DatabaseConnection;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, ConnectionTrait, DbBackend, EntityTrait,
    IntoActiveModel, QueryFilter, QueryOrder, Set, Statement, TransactionTrait,
};

use crate::db::entities::folder::FolderKind;
use crate::db::entities::{collection, folder};
use crate::db::error::DbError;
use crate::models::agent::AgentType;
use crate::models::{FolderDetail, FolderHistoryEntry};
use crate::parsers::normalize_path_for_matching;

/// Theme color sentinel stored in the DB. The frontend leaves the folder group
/// unscoped so it inherits the app-wide appearance theme color.
pub const DEFAULT_FOLDER_COLOR: &str = "inherit";

fn to_entry(m: folder::Model) -> FolderHistoryEntry {
    FolderHistoryEntry {
        id: m.id,
        path: m.path,
        name: m.name,
        last_opened_at: m.last_opened_at,
    }
}

fn parse_agent_type(s: &Option<String>) -> Option<AgentType> {
    s.as_deref()
        .and_then(|v| serde_json::from_value(serde_json::Value::String(v.to_string())).ok())
}

fn to_detail(m: folder::Model) -> FolderDetail {
    let default_agent_type = parse_agent_type(&m.default_agent_type);
    FolderDetail {
        id: m.id,
        name: m.name,
        path: m.path,
        git_branch: m.git_branch,
        default_agent_type,
        last_opened_at: m.last_opened_at,
        sort_order: m.sort_order,
        color: m.color,
        parent_id: m.parent_id,
        kind: m.kind,
        alias: m.alias,
    }
}

pub async fn get_folder_by_id(
    conn: &DatabaseConnection,
    folder_id: i32,
) -> Result<Option<FolderDetail>, DbError> {
    let row = folder::Entity::find_by_id(folder_id)
        .filter(folder::Column::DeletedAt.is_null())
        .one(conn)
        .await?;

    Ok(row.map(to_detail))
}

/// How [`add_folder_inner`] writes the `parent_id` column. The two callers want
/// different semantics on reopen of an existing path, which a bare `Option<i32>`
/// could not express (it conflates "no parent" with "don't touch the parent").
enum ParentWrite {
    /// Plain open: write this value only while the row has no root of its own.
    /// A plain reopen must never clear or move a recorded root — the user may
    /// have been given one deliberately by the worktree flow — but a row that
    /// has none is free to take the one [`infer_worktree_parent_id`] derived.
    SeedIfUnset(Option<i32>),
    /// Worktree open: write this exact value on both insert and reopen — so the
    /// stored relationship always reflects the latest call (including `None` to
    /// demote to a top-level folder) and can never go stale.
    Set(Option<i32>),
}

/// Register (or reopen) a folder the user picked themselves.
///
/// Unlike [`add_folder_with_parent`] the caller supplies no relationship, so one
/// is derived here: a directory that is a linked git worktree of a repository
/// already in this workspace is filed under that repository. Without it, opening
/// a worktree that lives outside its repo's directory — which is where codeg's
/// own `.claude/worktrees` and most hand-made ones sit — produced a second
/// top-level Path, splitting the sidebar, Room placement and Collection homing
/// for what is one project.
pub async fn add_folder(
    conn: &DatabaseConnection,
    path: &str,
) -> Result<FolderHistoryEntry, DbError> {
    let inferred = infer_worktree_parent_id(conn, path).await?;
    add_folder_inner(conn, path, ParentWrite::SeedIfUnset(inferred)).await
}

/// The folder `path` belongs under when `path` is a linked git worktree, or
/// `None` when it is not one / its repository is not in this workspace.
///
/// Two guards keep this from inventing structure. The main repository must
/// already be a live folder row: opening a worktree must not conjure a Path for
/// a repository the user never added. And a repository that is itself filed
/// under a root yields that root, so the flattening the rest of the app derives
/// from (`COALESCE(parent_id, id)`) never has to walk more than one hop.
///
/// The filesystem probe runs first and costs a single `stat` for the
/// overwhelming majority of opens (a plain directory, or a normal repo whose
/// `.git` is a directory), so the folder scan below only happens for a directory
/// that really is a worktree.
pub async fn infer_worktree_parent_id(
    conn: &DatabaseConnection,
    path: &str,
) -> Result<Option<i32>, DbError> {
    let Some(main_root) = crate::git_repo::linked_worktree_main_root(std::path::Path::new(path))
    else {
        return Ok(None);
    };
    let Some(wanted) = main_root.to_str().map(normalize_path_for_matching) else {
        return Ok(None);
    };
    if wanted == normalize_path_for_matching(path) {
        return Ok(None);
    }

    // Paths are stored exactly as they arrived (`add_folder` never
    // canonicalizes), so the same directory is on file as `D:/x` or `D:\x`
    // depending on which picker produced it. Match on the normalized form the
    // conversation importer already uses rather than with SQL equality — the
    // folder table is workspace-sized, so scanning it is cheaper than teaching
    // SQLite about separators and case.
    let rows = folder::Entity::find()
        .filter(folder::Column::DeletedAt.is_null())
        .filter(folder::Column::Kind.eq(FolderKind::Regular))
        .all(conn)
        .await?;

    Ok(rows
        .into_iter()
        .find(|row| normalize_path_for_matching(&row.path) == wanted)
        .map(|row| row.parent_id.unwrap_or(row.id)))
}

/// Like [`add_folder`] but authoritatively sets `parent_id` — the *root* folder
/// this path was created under (used by the worktree flow so a worktree folder
/// remembers its originating repo folder). The value is written on both insert
/// and reopen, so it always reflects the latest worktree relationship and never
/// a stale one.
pub async fn add_folder_with_parent(
    conn: &DatabaseConnection,
    path: &str,
    parent_id: Option<i32>,
) -> Result<FolderHistoryEntry, DbError> {
    add_folder_inner(conn, path, ParentWrite::Set(parent_id)).await
}

async fn add_folder_inner(
    conn: &DatabaseConnection,
    path: &str,
    parent: ParentWrite,
) -> Result<FolderHistoryEntry, DbError> {
    let now = Utc::now();
    let name = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());

    let existing = folder::Entity::find()
        .filter(folder::Column::Path.eq(path))
        .one(conn)
        .await?;

    let model = if let Some(row) = existing {
        let (row_id, had_parent) = (row.id, row.parent_id.is_some());
        let mut active = row.into_active_model();
        active.name = Set(name);
        active.last_opened_at = Set(now);
        active.updated_at = Set(now);
        active.deleted_at = Set(None);
        active.is_open = Set(true);
        // The worktree flow writes the authoritative value (including NULL) so it
        // can never go stale; a plain reopen only fills a blank, and never with
        // the row itself (a repository recorded under this very folder would
        // otherwise make it its own root).
        match parent {
            ParentWrite::Set(parent_id) => active.parent_id = Set(parent_id),
            ParentWrite::SeedIfUnset(inferred) if !had_parent => {
                if let Some(parent_id) = inferred.filter(|id| *id != row_id) {
                    active.parent_id = Set(Some(parent_id));
                }
            }
            ParentWrite::SeedIfUnset(_) => {}
        }
        active.update(conn).await?
    } else {
        let max_order = folder::Entity::find()
            .order_by_desc(folder::Column::SortOrder)
            .one(conn)
            .await?
            .map(|m| m.sort_order)
            .unwrap_or(0);
        let active = folder::ActiveModel {
            id: NotSet,
            name: Set(name.clone()),
            path: Set(path.to_string()),
            git_branch: Set(None),
            default_agent_type: Set(None),
            last_opened_at: Set(now),
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
            is_open: Set(true),
            sort_order: Set(max_order + 1),
            color: Set(DEFAULT_FOLDER_COLOR.to_string()),
            parent_id: Set(match parent {
                ParentWrite::SeedIfUnset(inferred) => inferred,
                ParentWrite::Set(parent_id) => parent_id,
            }),
            kind: Set(FolderKind::Regular),
            alias: Set(None),
        };
        active.insert(conn).await?
    };

    Ok(to_entry(model))
}

/// Create a dedicated hidden folder backing a single chat-mode conversation.
///
/// Unlike [`add_folder`], the display name is a fixed sentinel ("Chat") rather
/// than derived from the path, and `kind = chat` is set so the frontend routes
/// this folder's conversations to the sidebar "Chat" group and hides
/// folder-bound chrome. `path` is a freshly generated per-conversation scratch dir, so it
/// never collides on the `UNIQUE(path)` constraint. Returns the full
/// [`FolderDetail`] so the caller can hand it straight to the frontend.
pub async fn add_chat_folder(
    conn: &DatabaseConnection,
    path: &str,
) -> Result<FolderDetail, DbError> {
    let now = Utc::now();
    let max_order = folder::Entity::find()
        .order_by_desc(folder::Column::SortOrder)
        .one(conn)
        .await?
        .map(|m| m.sort_order)
        .unwrap_or(0);
    let active = folder::ActiveModel {
        id: NotSet,
        name: Set("Chat".to_string()),
        path: Set(path.to_string()),
        git_branch: Set(None),
        default_agent_type: Set(None),
        last_opened_at: Set(now),
        created_at: Set(now),
        updated_at: Set(now),
        deleted_at: Set(None),
        is_open: Set(true),
        sort_order: Set(max_order + 1),
        color: Set(DEFAULT_FOLDER_COLOR.to_string()),
        parent_id: Set(None),
        kind: Set(FolderKind::Chat),
        alias: Set(None),
    };
    let model = active.insert(conn).await?;
    Ok(to_detail(model))
}

pub async fn update_folder_color(
    conn: &DatabaseConnection,
    folder_id: i32,
    color: &str,
) -> Result<Option<FolderDetail>, DbError> {
    let row = folder::Entity::find_by_id(folder_id)
        .filter(folder::Column::DeletedAt.is_null())
        .one(conn)
        .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let mut active = row.into_active_model();
    active.color = Set(color.to_string());
    active.updated_at = Set(Utc::now());
    let updated = active.update(conn).await?;
    Ok(Some(to_detail(updated)))
}

/// Sets (or clears) a folder's display alias. `alias = None` clears it; callers
/// are expected to have already normalized empty/whitespace input to `None`.
pub async fn update_folder_alias(
    conn: &DatabaseConnection,
    folder_id: i32,
    alias: Option<String>,
) -> Result<Option<FolderDetail>, DbError> {
    let row = folder::Entity::find_by_id(folder_id)
        .filter(folder::Column::DeletedAt.is_null())
        .one(conn)
        .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let mut active = row.into_active_model();
    active.alias = Set(alias);
    active.updated_at = Set(Utc::now());
    let updated = active.update(conn).await?;
    Ok(Some(to_detail(updated)))
}

/// Fill in a folder's alias only when it has none — the worktree flow's
/// "default the label to the branch name" write.
///
/// A single conditional UPDATE (`… WHERE alias IS NULL OR alias = ''`) rather
/// than read-then-write: a worktree folder can be re-registered concurrently
/// (task relaunch, automation run, the user re-opening the same directory)
/// while its owner is renaming it in the sidebar, and the read-then-write shape
/// would let the seed land on top of that rename. Returns whether a row was
/// actually written, so the caller only re-reads / broadcasts on a real change.
/// The alias is user-editable afterwards, including back to empty — clearing it
/// makes the folder eligible for seeding again, which is the same "no alias set"
/// state a fresh worktree starts in.
pub async fn seed_folder_alias(
    conn: &DatabaseConnection,
    folder_id: i32,
    alias: &str,
) -> Result<bool, DbError> {
    use sea_orm::sea_query::Expr;
    let alias = alias.trim();
    if alias.is_empty() {
        return Ok(false);
    }
    let res = folder::Entity::update_many()
        .col_expr(folder::Column::Alias, Expr::value(alias))
        .col_expr(folder::Column::UpdatedAt, Expr::value(Utc::now()))
        .filter(folder::Column::Id.eq(folder_id))
        .filter(folder::Column::DeletedAt.is_null())
        .filter(
            sea_orm::Condition::any()
                .add(folder::Column::Alias.is_null())
                .add(folder::Column::Alias.eq("")),
        )
        .exec(conn)
        .await?;
    Ok(res.rows_affected > 0)
}

/// `(id, path)` of every live worktree folder still without an alias — the work
/// list for the startup backfill that labels worktrees registered before aliases
/// were seeded at creation. `parent_id IS NOT NULL` is exactly "is a worktree
/// folder" in this schema — written either by the worktree flow or, for one the
/// user opened by hand, by [`infer_worktree_parent_id`].
///
/// Closed folders are included on purpose, so reopening one later already reads
/// as its branch. Whether a row may be *announced* is a separate, later question
/// — see [`get_open_folder_by_id`].
pub async fn list_worktree_folders_missing_alias(
    conn: &DatabaseConnection,
) -> Result<Vec<(i32, String)>, DbError> {
    let rows = folder::Entity::find()
        .filter(folder::Column::DeletedAt.is_null())
        .filter(folder::Column::ParentId.is_not_null())
        .filter(
            sea_orm::Condition::any()
                .add(folder::Column::Alias.is_null())
                .add(folder::Column::Alias.eq("")),
        )
        .order_by_asc(folder::Column::Id)
        .all(conn)
        .await?;

    Ok(rows.into_iter().map(|m| (m.id, m.path)).collect())
}

/// `(id, path)` of every live folder still recorded as a top-level Path — the
/// work list for the startup pass that files worktrees opened before
/// [`infer_worktree_parent_id`] existed under their repository.
///
/// Chat scratch folders are excluded: they are hidden per-conversation
/// directories, never a checkout of anything. Closed folders are included, for
/// the same reason the alias backfill includes them — reopening one later should
/// already land in the right Path.
pub async fn list_rootless_folders(
    conn: &DatabaseConnection,
) -> Result<Vec<(i32, String)>, DbError> {
    let rows = folder::Entity::find()
        .filter(folder::Column::DeletedAt.is_null())
        .filter(folder::Column::ParentId.is_null())
        .filter(folder::Column::Kind.ne(FolderKind::Chat))
        .order_by_asc(folder::Column::Id)
        .all(conn)
        .await?;

    Ok(rows.into_iter().map(|m| (m.id, m.path)).collect())
}

/// Record a folder's root only while it has none — the backfill counterpart of
/// the seeding [`add_folder`] now does at open time.
///
/// A single conditional UPDATE (`… WHERE parent_id IS NULL`) rather than
/// read-then-write, mirroring [`seed_folder_alias`]: the same directory can be
/// re-registered by the worktree flow (a task relaunch, a branch switch) while
/// this best-effort pass walks the workspace, and the read-then-write shape
/// would let the backfill land on top of that authoritative write. Refuses to
/// make a folder its own root. Returns whether a row was actually written, so
/// the caller only re-reads / broadcasts on a real change.
pub async fn seed_folder_parent(
    conn: &DatabaseConnection,
    folder_id: i32,
    parent_id: i32,
) -> Result<bool, DbError> {
    use sea_orm::sea_query::Expr;
    if parent_id == folder_id {
        return Ok(false);
    }
    let txn = conn.begin().await?;
    let now = Utc::now();
    let res = folder::Entity::update_many()
        .col_expr(folder::Column::ParentId, Expr::value(parent_id))
        .col_expr(folder::Column::UpdatedAt, Expr::value(now))
        .filter(folder::Column::Id.eq(folder_id))
        .filter(folder::Column::DeletedAt.is_null())
        .filter(folder::Column::ParentId.is_null())
        .exec(&txn)
        .await?;
    if res.rows_affected == 0 {
        txn.commit().await?;
        return Ok(false);
    }

    // A worktree may have been opened before its repository root. During that
    // interval it was a legitimate top-level Path, so Collections and Rooms
    // created there stored the worktree id as their semantic root. Re-homing
    // only `folder.parent_id` would strand those objects under a Path that the
    // UI no longer renders as a root. Move all three facts atomically.
    collection::Entity::update_many()
        .col_expr(collection::Column::RootFolderId, Expr::value(parent_id))
        .col_expr(collection::Column::UpdatedAt, Expr::value(now))
        .filter(collection::Column::RootFolderId.eq(folder_id))
        .exec(&txn)
        .await?;
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE collaboration_room \
         SET root_folder_id = ?, updated_at = CURRENT_TIMESTAMP \
         WHERE root_folder_id = ?",
        vec![parent_id.into(), folder_id.into()],
    ))
    .await?;

    txn.commit().await?;
    Ok(true)
}

/// Like [`get_folder_by_id`], but `None` unless the folder is currently in the
/// workspace.
///
/// For background producers deciding whether to broadcast a `folder://changed`
/// Upsert, which means "insert-or-replace in the client's OPEN folder list" —
/// sending one for a closed folder puts it back in the user's sidebar.
/// `FolderDetail` carries no open state, so that check cannot be made from the
/// returned value, and a producer that walks many folders must not reuse an
/// open/closed reading taken when its work list was built: the user can close a
/// folder while it works. Reading both here, as late as possible, keeps the two
/// facts consistent.
pub async fn get_open_folder_by_id(
    conn: &DatabaseConnection,
    folder_id: i32,
) -> Result<Option<FolderDetail>, DbError> {
    let row = folder::Entity::find_by_id(folder_id)
        .filter(folder::Column::DeletedAt.is_null())
        .filter(folder::Column::IsOpen.eq(true))
        .one(conn)
        .await?;

    Ok(row.map(to_detail))
}

pub async fn update_folder_default_agent(
    conn: &DatabaseConnection,
    folder_id: i32,
    default_agent_type: Option<AgentType>,
) -> Result<Option<FolderDetail>, DbError> {
    let row = folder::Entity::find_by_id(folder_id)
        .filter(folder::Column::DeletedAt.is_null())
        .one(conn)
        .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    // Serialize AgentType to its snake_case wire form (e.g. "claude_code").
    // Mirrors `parse_agent_type`'s round-trip through serde_json.
    let serialized = default_agent_type
        .map(|t| serde_json::to_value(t).ok())
        .and_then(|v| v.and_then(|val| val.as_str().map(|s| s.to_string())));

    let mut active = row.into_active_model();
    active.default_agent_type = Set(serialized);
    active.updated_at = Set(Utc::now());
    let updated = active.update(conn).await?;
    Ok(Some(to_detail(updated)))
}

pub async fn list_folders(conn: &DatabaseConnection) -> Result<Vec<FolderHistoryEntry>, DbError> {
    let rows = folder::Entity::find()
        .filter(folder::Column::DeletedAt.is_null())
        // Only regular folders are user-facing in folder history / open-folder
        // pickers — hidden chat folders (and future engine-created kinds) are an
        // implementation detail.
        .filter(folder::Column::Kind.eq(FolderKind::Regular))
        .order_by_desc(folder::Column::LastOpenedAt)
        .all(conn)
        .await?;

    Ok(rows.into_iter().map(to_entry).collect())
}

pub async fn remove_folder(conn: &DatabaseConnection, path: &str) -> Result<(), DbError> {
    let now = Utc::now();
    let row = folder::Entity::find()
        .filter(folder::Column::Path.eq(path))
        .filter(folder::Column::DeletedAt.is_null())
        .one(conn)
        .await?;

    if let Some(row) = row {
        let mut active = row.into_active_model();
        active.deleted_at = Set(Some(now));
        active.updated_at = Set(now);
        active.update(conn).await?;
    }
    Ok(())
}

/// Soft-delete a folder by id and close it — the permanent counterpart of
/// [`set_folder_open`]`(.., false)`, used when the directory behind the row is
/// gone for good (a removed git worktree).
///
/// An id that is already soft-deleted (or never existed) succeeds untouched, so
/// a caller retrying a half-finished cleanup still reaches its "announce the
/// drop" step instead of aborting on a row it already removed.
pub async fn soft_delete_folder(conn: &DatabaseConnection, folder_id: i32) -> Result<(), DbError> {
    let Some(row) = folder::Entity::find_by_id(folder_id).one(conn).await? else {
        return Ok(());
    };
    if row.deleted_at.is_some() {
        return Ok(());
    }
    let now = Utc::now();
    let mut active = row.into_active_model();
    active.is_open = Set(false);
    active.deleted_at = Set(Some(now));
    active.updated_at = Set(now);
    active.update(conn).await?;
    Ok(())
}

pub async fn set_folder_open(
    conn: &DatabaseConnection,
    folder_id: i32,
    is_open: bool,
) -> Result<(), DbError> {
    let row = folder::Entity::find_by_id(folder_id).one(conn).await?;

    if let Some(row) = row {
        let mut active = row.into_active_model();
        active.is_open = Set(is_open);
        active.updated_at = Set(Utc::now());
        active.update(conn).await?;
    }
    Ok(())
}

pub async fn list_open_folders(
    conn: &DatabaseConnection,
) -> Result<Vec<FolderHistoryEntry>, DbError> {
    let rows = folder::Entity::find()
        .filter(folder::Column::DeletedAt.is_null())
        .filter(folder::Column::IsOpen.eq(true))
        .filter(folder::Column::Kind.eq(FolderKind::Regular))
        .order_by_desc(folder::Column::LastOpenedAt)
        .all(conn)
        .await?;

    Ok(rows.into_iter().map(to_entry).collect())
}

pub async fn list_open_folder_details(
    conn: &DatabaseConnection,
) -> Result<Vec<FolderDetail>, DbError> {
    // Excludes hidden chat folders from the workspace "open folders" surface.
    // `list_all_folder_details` (below) intentionally keeps them so the frontend
    // can still resolve an active chat conversation's cwd / active folder by id.
    let rows = folder::Entity::find()
        .filter(folder::Column::DeletedAt.is_null())
        .filter(folder::Column::IsOpen.eq(true))
        .filter(folder::Column::Kind.eq(FolderKind::Regular))
        .order_by_asc(folder::Column::SortOrder)
        .order_by_desc(folder::Column::LastOpenedAt)
        .all(conn)
        .await?;

    Ok(rows.into_iter().map(to_detail).collect())
}

pub async fn list_all_folder_details(
    conn: &DatabaseConnection,
) -> Result<Vec<FolderDetail>, DbError> {
    let rows = folder::Entity::find()
        .filter(folder::Column::DeletedAt.is_null())
        .order_by_asc(folder::Column::SortOrder)
        .order_by_desc(folder::Column::LastOpenedAt)
        .all(conn)
        .await?;

    Ok(rows.into_iter().map(to_detail).collect())
}

/// Paths of all *live* (non-deleted) chat scratch folders. Consumed by the
/// startup orphan-scratch-dir GC to spare directories still bound to a chat
/// conversation, while reclaiming pre-send drafts (no row at all) and
/// post-delete dirs (soft-deleted row → `DeletedAt` set → excluded here).
pub async fn list_live_chat_folder_paths(
    conn: &DatabaseConnection,
) -> Result<Vec<String>, DbError> {
    let rows = folder::Entity::find()
        .filter(folder::Column::DeletedAt.is_null())
        .filter(folder::Column::Kind.eq(FolderKind::Chat))
        .all(conn)
        .await?;

    Ok(rows.into_iter().map(|m| m.path).collect())
}

pub async fn reorder_folders(conn: &DatabaseConnection, ids: Vec<i32>) -> Result<(), DbError> {
    if ids.is_empty() {
        return Ok(());
    }

    let now = Utc::now();
    let now_str = now.format("%Y-%m-%d %H:%M:%S %:z").to_string();
    let case_expr = ids
        .iter()
        .enumerate()
        .map(|(idx, id)| format!("WHEN {} THEN {}", id, idx + 1))
        .collect::<Vec<_>>()
        .join(" ");
    let id_list = ids
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(", ");

    let sql = format!(
        "UPDATE folder SET sort_order = CASE id {case_expr} END, updated_at = '{now_str}' WHERE id IN ({id_list})"
    );
    conn.execute(Statement::from_string(DbBackend::Sqlite, sql))
        .await?;

    Ok(())
}

#[cfg(test)]
mod worktree_parent_tests {
    use super::*;
    use crate::db::service::{
        collaboration_room_service, collection_service, conversation_service, workbench_service,
    };
    use crate::db::test_helpers::fresh_in_memory_db;
    use crate::models::{AgentType, CreateCollaborationRoomInput};

    /// `(tempdir, repo_path, worktree_path)` for the layout this fix is about:
    /// a linked worktree that does NOT live inside its repository, so nothing
    /// but the `.git` pointer connects the two. Only the pointer file is real —
    /// the derivation never runs git, so no repository has to be initialized.
    fn repo_and_detached_worktree() -> (tempfile::TempDir, String, String) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path().join("repo");
        let wt = dir.path().join("checkouts/fork-rewind");
        std::fs::create_dir_all(repo.join(".git")).expect("mkdir repo/.git");
        std::fs::create_dir_all(&wt).expect("mkdir worktree");
        std::fs::write(
            wt.join(".git"),
            format!(
                "gitdir: {}\n",
                repo.join(".git/worktrees/fork-rewind").display()
            ),
        )
        .expect("write .git pointer");
        (
            dir,
            repo.to_str().expect("utf-8 path").to_string(),
            wt.to_str().expect("utf-8 path").to_string(),
        )
    }

    async fn parent_of(db: &crate::db::AppDatabase, folder_id: i32) -> Option<i32> {
        get_folder_by_id(&db.conn, folder_id)
            .await
            .expect("query")
            .expect("row")
            .parent_id
    }

    #[tokio::test]
    async fn opening_a_worktree_files_it_under_the_registered_repository() {
        let db = fresh_in_memory_db().await;
        let (_dir, repo, wt) = repo_and_detached_worktree();
        let root = add_folder(&db.conn, &repo)
            .await
            .expect("register the repo");

        let folder = add_folder(&db.conn, &wt).await.expect("open the worktree");

        assert_eq!(parent_of(&db, folder.id).await, Some(root.id));
    }

    /// Opening a worktree must not conjure a Path for a repository the user
    /// never added — that would put a directory in their sidebar they did not
    /// ask for.
    #[tokio::test]
    async fn a_worktree_whose_repository_is_unknown_stays_top_level() {
        let db = fresh_in_memory_db().await;
        let (_dir, _repo, wt) = repo_and_detached_worktree();

        let folder = add_folder(&db.conn, &wt).await.expect("open the worktree");

        assert_eq!(parent_of(&db, folder.id).await, None);
    }

    /// A repository that is not in the workspace *any more* is the same case:
    /// the row is there but soft-deleted, and reviving it as someone's root
    /// would put it back in the sidebar sideways.
    #[tokio::test]
    async fn a_removed_repository_is_not_a_root() {
        let db = fresh_in_memory_db().await;
        let (_dir, repo, wt) = repo_and_detached_worktree();
        add_folder(&db.conn, &repo)
            .await
            .expect("register the repo");
        remove_folder(&db.conn, &repo).await.expect("remove it");

        let folder = add_folder(&db.conn, &wt).await.expect("open the worktree");

        assert_eq!(parent_of(&db, folder.id).await, None);
    }

    /// The pointer file spells the repository the way git wrote it; the folder
    /// row spells it the way the user's picker did. Both reach the same row.
    #[tokio::test]
    async fn the_repository_matches_across_separator_spellings() {
        let db = fresh_in_memory_db().await;
        let (_dir, repo, wt) = repo_and_detached_worktree();
        let flipped = if repo.contains('\\') {
            repo.replace('\\', "/")
        } else {
            repo.replace('/', "\\")
        };
        let root = add_folder(&db.conn, &flipped)
            .await
            .expect("register the repo under the other spelling");

        let folder = add_folder(&db.conn, &wt).await.expect("open the worktree");

        assert_eq!(parent_of(&db, folder.id).await, Some(root.id));
    }

    /// Windows paths are case-insensitive, and the two spellings routinely
    /// differ (a drive letter typed lowercase, a picker returning the on-disk
    /// casing).
    #[tokio::test]
    #[cfg(target_os = "windows")]
    async fn the_repository_matches_across_case_differences() {
        let db = fresh_in_memory_db().await;
        let (_dir, repo, wt) = repo_and_detached_worktree();
        let root = add_folder(&db.conn, &repo.to_uppercase())
            .await
            .expect("register the repo in upper case");

        let folder = add_folder(&db.conn, &wt).await.expect("open the worktree");

        assert_eq!(parent_of(&db, folder.id).await, Some(root.id));
    }

    /// A plain checkout keeps its `.git` as a directory and belongs to nobody.
    #[tokio::test]
    async fn a_plain_repository_stays_top_level() {
        let db = fresh_in_memory_db().await;
        let (_dir, repo, _wt) = repo_and_detached_worktree();

        let folder = add_folder(&db.conn, &repo).await.expect("open the repo");

        assert_eq!(parent_of(&db, folder.id).await, None);
    }

    /// Inference fills a blank, it does not relocate. A folder the worktree flow
    /// (or a future manual re-home) already placed keeps that placement, even
    /// when the pointer on disk names a different repository.
    #[tokio::test]
    async fn a_recorded_root_survives_a_plain_reopen() {
        let db = fresh_in_memory_db().await;
        let (_dir, repo, wt) = repo_and_detached_worktree();
        add_folder(&db.conn, &repo)
            .await
            .expect("register the repo");
        let elsewhere = add_folder(&db.conn, &format!("{repo}-elsewhere"))
            .await
            .expect("a second root");
        let folder = add_folder_with_parent(&db.conn, &wt, Some(elsewhere.id))
            .await
            .expect("register the worktree under the second root");

        add_folder(&db.conn, &wt).await.expect("plain reopen");

        assert_eq!(
            parent_of(&db, folder.id).await,
            Some(elsewhere.id),
            "a reopen must not move a folder the user's workflow already placed"
        );
    }

    /// A worktree of a worktree flattens: the repository the pointer names is
    /// itself filed under a root, and that root — not the intermediate — is what
    /// gets recorded, so `COALESCE(parent_id, id)` never needs a second hop.
    #[tokio::test]
    async fn the_root_of_a_nested_worktree_is_flattened() {
        let db = fresh_in_memory_db().await;
        let (_dir, repo, wt) = repo_and_detached_worktree();
        let root = add_folder(&db.conn, &format!("{repo}-origin"))
            .await
            .expect("the original repository");
        add_folder_with_parent(&db.conn, &repo, Some(root.id))
            .await
            .expect("the pointed-at repo is itself a worktree");

        let folder = add_folder(&db.conn, &wt).await.expect("open the worktree");

        assert_eq!(parent_of(&db, folder.id).await, Some(root.id));
    }

    /// The backfill's write is conditional so a concurrent authoritative
    /// registration wins, and so a second launch is a no-op.
    #[tokio::test]
    async fn seeding_a_root_never_clobbers_one_already_recorded() {
        let db = fresh_in_memory_db().await;
        let a = add_folder(&db.conn, "/tmp/codeg-seed-a").await.expect("a");
        let b = add_folder(&db.conn, "/tmp/codeg-seed-b").await.expect("b");
        let wt = add_folder(&db.conn, "/tmp/codeg-seed-wt")
            .await
            .expect("wt");

        assert!(seed_folder_parent(&db.conn, wt.id, a.id)
            .await
            .expect("first seed"));
        assert!(
            !seed_folder_parent(&db.conn, wt.id, b.id)
                .await
                .expect("second seed"),
            "a folder that already has a root is left alone"
        );
        assert!(
            !seed_folder_parent(&db.conn, wt.id, wt.id)
                .await
                .expect("self seed"),
            "a folder can never be its own root"
        );
        assert_eq!(parent_of(&db, wt.id).await, Some(a.id));
    }

    /// If the worktree was opened first, semantic objects could legitimately
    /// choose it as their root before the repository appeared. Filing the
    /// worktree later must move those roots in the same operation; otherwise
    /// the Collection/Room disappears from the canonical Path tree.
    #[tokio::test]
    async fn seeding_a_root_rehomes_worktree_collections_and_rooms() {
        let db = fresh_in_memory_db().await;
        let root = add_folder(&db.conn, "/tmp/codeg-rehome-root")
            .await
            .expect("root");
        let wt = add_folder(&db.conn, "/tmp/codeg-rehome-worktree")
            .await
            .expect("worktree opened before it was recognized");
        let collection =
            collection_service::create(&db.conn, "Worktree ideas".into(), None, Some(wt.id))
                .await
                .expect("collection rooted at the temporary top-level worktree");
        let creator = conversation_service::create(
            &db.conn,
            wt.id,
            AgentType::ClaudeCode,
            Some("Worktree session".into()),
            Some("experiment".into()),
        )
        .await
        .expect("creator session");
        let workbench = workbench_service::create(&db.conn, Some("Main".into()))
            .await
            .expect("workbench");
        let room = collaboration_room_service::create(
            &db.conn,
            CreateCollaborationRoomInput {
                workbench_id: workbench.id,
                title: "Worktree room".into(),
                member_conversation_ids: vec![creator.id],
                created_by_conversation_id: creator.id,
                collection_id: None,
                root_folder_id: Some(wt.id),
            },
        )
        .await
        .expect("room rooted at the temporary top-level worktree");

        assert!(seed_folder_parent(&db.conn, wt.id, root.id)
            .await
            .expect("seed root"));

        let collections = collection_service::list(&db.conn)
            .await
            .expect("list collections");
        assert_eq!(
            collections
                .iter()
                .find(|item| item.id == collection.id)
                .and_then(|item| item.root_folder_id),
            Some(root.id)
        );
        assert_eq!(
            collaboration_room_service::get(&db.conn, &room.id)
                .await
                .expect("reload room")
                .root_folder_id,
            Some(root.id)
        );
        assert_eq!(parent_of(&db, wt.id).await, Some(root.id));
    }

    /// The backfill work list: everything still top-level, minus the hidden chat
    /// scratch dirs, which are per-conversation directories rather than
    /// checkouts.
    #[tokio::test]
    async fn the_backfill_work_list_skips_chat_and_placed_folders() {
        let db = fresh_in_memory_db().await;
        let root = add_folder(&db.conn, "/tmp/codeg-list-root")
            .await
            .expect("root");
        let placed = add_folder_with_parent(&db.conn, "/tmp/codeg-list-wt", Some(root.id))
            .await
            .expect("placed");
        add_chat_folder(&db.conn, "/tmp/codeg-list-chat")
            .await
            .expect("chat scratch dir");

        let ids: Vec<i32> = list_rootless_folders(&db.conn)
            .await
            .expect("work list")
            .into_iter()
            .map(|(id, _)| id)
            .collect();

        assert_eq!(ids, vec![root.id]);
        assert!(!ids.contains(&placed.id));
    }
}
