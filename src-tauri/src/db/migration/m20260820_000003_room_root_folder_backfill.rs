//! Give already-stored Rooms a Path so the sidebar tree can render them.
//!
//! The Collection tree files a Room by `collection_id` first and by
//! `root_folder_id` second. A row with both NULL belongs to neither bucket and
//! is invisible — no amount of scrolling or expanding reaches it. Every Room an
//! agent opened through `room.create` (MCP / Host Control) was written that way
//! until `collaboration_room_service::resolve_placement` started stamping the
//! creator's Path, so this backfills the rows that predate that fix using the
//! exact same derivation: the creator Session's Folder, flattened to the
//! repository root a worktree points at.
//!
//! Best-effort on purpose. A creator running in a `chat` scratch folder or in a
//! Folder the user has since removed owns no Path; those rows stay NULL and the
//! sidebar's orphan bucket renders them instead of this migration guessing.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"
UPDATE collaboration_room
SET root_folder_id = (
    SELECT COALESCE(f.parent_id, f.id)
    FROM conversation c
    JOIN folder f ON f.id = c.folder_id
    WHERE c.id = collaboration_room.created_by_conversation_id
      AND f.kind <> 'chat'
      AND f.deleted_at IS NULL
)
WHERE root_folder_id IS NULL;
"#,
            )
            .await?;
        Ok(())
    }

    /// Not reversible, and deliberately a no-op: after the backfill a stamped
    /// `root_folder_id` is indistinguishable from one the user chose, so
    /// clearing them on the way down would throw away real placements.
    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_orm::{
        ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement,
    };

    /// Folder 1 is a plain repository root, 2 a worktree of it, 3 a chat
    /// scratch dir and 4 a removed Folder. One creator Session per Folder.
    async fn stub_db() -> DatabaseConnection {
        let conn = Database::connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        conn.execute_unprepared(
            "CREATE TABLE folder (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 parent_id INTEGER NULL,
                 kind TEXT NOT NULL DEFAULT 'regular',
                 deleted_at TEXT NULL
             );
             CREATE TABLE conversation (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 folder_id INTEGER NOT NULL
             );
             CREATE TABLE collaboration_room (
                 id TEXT PRIMARY KEY NOT NULL,
                 created_by_conversation_id INTEGER NOT NULL,
                 collection_id INTEGER NULL,
                 root_folder_id INTEGER NULL
             );
             INSERT INTO folder (id, parent_id, kind, deleted_at) VALUES
                 (1, NULL, 'regular', NULL),
                 (2, 1, 'regular', NULL),
                 (3, NULL, 'chat', NULL),
                 (4, NULL, 'regular', '2026-08-19');
             INSERT INTO conversation (id, folder_id) VALUES
                 (11, 1), (12, 2), (13, 3), (14, 4);",
        )
        .await
        .expect("stub tables");
        conn
    }

    async fn root_of(conn: &DatabaseConnection, room_id: &str) -> Option<i32> {
        conn.query_one(Statement::from_string(
            DbBackend::Sqlite,
            format!("SELECT root_folder_id FROM collaboration_room WHERE id = '{room_id}'"),
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get("", "root_folder_id")
        .expect("column")
    }

    #[tokio::test]
    async fn backfills_the_creator_path_and_leaves_the_rest_alone() {
        let conn = stub_db().await;
        conn.execute_unprepared(
            "INSERT INTO collaboration_room
                 (id, created_by_conversation_id, collection_id, root_folder_id)
             VALUES
                 ('rm_plain', 11, NULL, NULL),
                 ('rm_worktree', 12, NULL, NULL),
                 ('rm_chat', 13, NULL, NULL),
                 ('rm_gone', 14, NULL, NULL),
                 ('rm_placed', 11, NULL, 9);",
        )
        .await
        .expect("seed rooms");

        Migration
            .up(&SchemaManager::new(&conn))
            .await
            .expect("run migration up");

        assert_eq!(
            root_of(&conn, "rm_plain").await,
            Some(1),
            "the creator's own Folder is its Path"
        );
        assert_eq!(
            root_of(&conn, "rm_worktree").await,
            Some(1),
            "a worktree creator resolves to the repository root, not the worktree"
        );
        assert_eq!(
            root_of(&conn, "rm_chat").await,
            None,
            "a chat scratch folder owns no Path and must not be guessed at"
        );
        assert_eq!(
            root_of(&conn, "rm_gone").await,
            None,
            "a removed Folder is no longer a tree root"
        );
        assert_eq!(
            root_of(&conn, "rm_placed").await,
            Some(9),
            "a Room that already had a Path keeps it"
        );
    }

    #[tokio::test]
    async fn running_it_twice_changes_nothing() {
        let conn = stub_db().await;
        conn.execute_unprepared(
            "INSERT INTO collaboration_room
                 (id, created_by_conversation_id, collection_id, root_folder_id)
             VALUES ('rm_plain', 11, NULL, NULL);",
        )
        .await
        .expect("seed room");

        let manager = SchemaManager::new(&conn);
        Migration.up(&manager).await.expect("first run");
        Migration.up(&manager).await.expect("second run");

        assert_eq!(root_of(&conn, "rm_plain").await, Some(1));
    }
}
