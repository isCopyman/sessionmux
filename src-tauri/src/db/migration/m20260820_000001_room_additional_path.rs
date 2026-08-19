//! Additional `@`-search paths a Room can be given besides its bound
//! `root_folder_id`. Unlike a Folder, an additional path is a bare string —
//! adding one must not require the user to first "open" it as a Folder
//! elsewhere, which would defeat the quick-add purpose of this feature.

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
CREATE TABLE IF NOT EXISTS collaboration_room_path (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    room_id TEXT NOT NULL,
    path TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (room_id) REFERENCES collaboration_room(id) ON DELETE CASCADE,
    UNIQUE (room_id, path)
);

CREATE INDEX IF NOT EXISTS idx_collaboration_room_path_room
    ON collaboration_room_path(room_id);
"#,
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"
DROP INDEX IF EXISTS idx_collaboration_room_path_room;
DROP TABLE IF EXISTS collaboration_room_path;
"#,
            )
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_orm::{ConnectionTrait, Database, DbBackend, Statement};

    #[tokio::test]
    async fn up_creates_room_path_table_with_unique_and_cascade() {
        let conn = Database::connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        conn.execute_unprepared(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE collaboration_room (
                 id TEXT PRIMARY KEY NOT NULL,
                 workbench_id INTEGER NOT NULL,
                 title TEXT NOT NULL,
                 created_by_conversation_id INTEGER NOT NULL,
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             );
             INSERT INTO collaboration_room
                 (id, workbench_id, title, created_by_conversation_id, created_at, updated_at)
             VALUES ('rm_test', 1, 'Plan', 2, '2026-08-20', '2026-08-20');",
        )
        .await
        .expect("stub tables");

        Migration
            .up(&SchemaManager::new(&conn))
            .await
            .expect("run migration up");

        conn.execute_unprepared(
            "INSERT INTO collaboration_room_path (room_id, path) VALUES ('rm_test', '/home/me/notes');",
        )
        .await
        .expect("insert path");

        // Duplicate (room_id, path) is rejected.
        let dup = conn
            .execute_unprepared(
                "INSERT INTO collaboration_room_path (room_id, path) VALUES ('rm_test', '/home/me/notes');",
            )
            .await;
        assert!(dup.is_err(), "duplicate room_id+path must be rejected");

        let count: i64 = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS count FROM collaboration_room_path WHERE room_id = 'rm_test'"
                    .to_owned(),
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get("", "count")
            .expect("column");
        assert_eq!(count, 1);

        // Deleting the room cascades to its additional paths.
        conn.execute_unprepared("DELETE FROM collaboration_room WHERE id = 'rm_test';")
            .await
            .expect("delete room");
        let remaining: i64 = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS count FROM collaboration_room_path".to_owned(),
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get("", "count")
            .expect("column");
        assert_eq!(remaining, 0, "ON DELETE CASCADE must remove the room's paths");
    }
}
