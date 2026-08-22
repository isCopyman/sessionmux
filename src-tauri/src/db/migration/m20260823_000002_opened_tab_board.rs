//! Persist task Board tabs in `opened_tab`. A Board is a view scope over the
//! shared work_task store (`global` or `project:<folderId>`), not a task copy.

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
    ADD COLUMN board_scope TEXT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_opened_tabs_workbench_board
    ON opened_tab(workbench_id, board_scope)
    WHERE board_scope IS NOT NULL;
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
DROP INDEX IF EXISTS idx_opened_tabs_workbench_board;
ALTER TABLE opened_tab DROP COLUMN board_scope;
"#,
            )
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_orm::{ConnectionTrait, Database};

    #[tokio::test]
    async fn persists_only_one_board_per_scope_and_workbench() {
        let conn = Database::connect("sqlite::memory:")
            .await
            .expect("open sqlite");
        conn.execute_unprepared(
            "CREATE TABLE opened_tab (
                 id INTEGER PRIMARY KEY NOT NULL,
                 workbench_id INTEGER NOT NULL DEFAULT 1,
                 folder_id INTEGER NOT NULL,
                 conversation_id INTEGER NULL,
                 room_id TEXT NULL,
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
            .expect("run migration");
        conn.execute_unprepared(
            "INSERT INTO opened_tab (
                 workbench_id, folder_id, conversation_id, room_id, board_scope,
                 agent_type, position, is_active, is_pinned, created_at, updated_at
             ) VALUES (
                 1, 1, NULL, NULL, 'project:1', 'claude_code',
                 0, 1, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
             );",
        )
        .await
        .expect("insert board");
        let duplicate = conn
            .execute_unprepared(
                "INSERT INTO opened_tab (
                     workbench_id, folder_id, conversation_id, room_id, board_scope,
                     agent_type, position, is_active, is_pinned, created_at, updated_at
                 ) VALUES (
                     1, 1, NULL, NULL, 'project:1', 'claude_code',
                     1, 0, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
                 );",
            )
            .await;
        assert!(duplicate.is_err());
        conn.execute_unprepared(
            "INSERT INTO opened_tab (
                 workbench_id, folder_id, conversation_id, room_id, board_scope,
                 agent_type, position, is_active, is_pinned, created_at, updated_at
             ) VALUES (
                 2, 1, NULL, NULL, 'project:1', 'claude_code',
                 0, 1, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
             );",
        )
        .await
        .expect("same scope in another workbench");
    }
}
