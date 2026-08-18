//! Persist Room tabs in `opened_tab` the same way Session tabs are persisted.
//! A row is either a conversation tab (`conversation_id`) or a Room tab
//! (`room_id`); drafts still have neither and are never written.

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
ALTER TABLE opened_tab
    ADD COLUMN room_id TEXT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_opened_tabs_workbench_room
    ON opened_tab(workbench_id, room_id)
    WHERE room_id IS NOT NULL;
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
DROP INDEX IF EXISTS idx_opened_tabs_workbench_room;
ALTER TABLE opened_tab DROP COLUMN room_id;
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
    async fn adds_room_id_column_and_unique_workbench_index() {
        let conn = Database::connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        conn.execute_unprepared(
            "CREATE TABLE opened_tab (
                 id INTEGER PRIMARY KEY NOT NULL,
                 workbench_id INTEGER NOT NULL DEFAULT 1,
                 folder_id INTEGER NOT NULL,
                 conversation_id INTEGER NULL,
                 agent_type TEXT NOT NULL,
                 position INTEGER NOT NULL,
                 is_active INTEGER NOT NULL DEFAULT 0,
                 is_pinned INTEGER NOT NULL DEFAULT 0,
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             );",
        )
        .await
        .expect("seed table");
        Migration
            .up(&SchemaManager::new(&conn))
            .await
            .expect("run migration up");
        conn.execute_unprepared(
            "INSERT INTO opened_tab (
                 workbench_id, folder_id, conversation_id, room_id, agent_type,
                 position, is_active, is_pinned, created_at, updated_at
             ) VALUES (
                 1, 1, NULL, 'rm_plan', 'claude_code',
                 0, 1, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
             );",
        )
        .await
        .expect("insert room tab");
        let count: i64 = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS count FROM opened_tab WHERE room_id = 'rm_plan'".to_owned(),
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get("", "count")
            .expect("column");
        assert_eq!(count, 1);
        let dup = conn
            .execute_unprepared(
                "INSERT INTO opened_tab (
                     workbench_id, folder_id, conversation_id, room_id, agent_type,
                     position, is_active, is_pinned, created_at, updated_at
                 ) VALUES (
                     1, 1, NULL, 'rm_plan', 'claude_code',
                     1, 0, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
                 );",
            )
            .await;
        assert!(dup.is_err(), "same workbench cannot persist a Room twice");
    }
}
