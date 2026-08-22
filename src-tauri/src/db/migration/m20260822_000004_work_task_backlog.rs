//! Add a real parking-lot status to the user-facing task workflow.
//!
//! `task_status` was introduced with a SQLite CHECK constraint, which SQLite
//! cannot widen in place. Rebuild only `work_task`; its ids and every existing
//! row remain unchanged, so event, assignment, and PromptQueue references keep
//! pointing at the same cards.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const COMMON_COLUMNS: &str = r#"
    id, folder_id, title, config, status, failure_reason, last_error, run_seq,
    sort_order, worktree_folder_id, conversation_id, connection_id, base_branch,
    base_sha, work_branch, merge_state, cleanup_state, verdict, result_summary,
    files_changed, additions, deletions, merge_commit, created_at, updated_at,
    started_at, settled_at, finished_at, deleted_at, pending_merge, preflight,
    archived_at, scheduled_at, task_status, execution_mode
"#;

fn table_sql(allowed_statuses: &str) -> String {
    format!(
        r#"
CREATE TABLE work_task_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    folder_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    config TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'todo',
    failure_reason TEXT NULL,
    last_error TEXT NULL,
    run_seq INTEGER NOT NULL DEFAULT 0,
    sort_order INTEGER NOT NULL DEFAULT 0,
    worktree_folder_id INTEGER NULL,
    conversation_id INTEGER NULL,
    connection_id TEXT NULL,
    base_branch TEXT NULL,
    base_sha TEXT NULL,
    work_branch TEXT NULL,
    merge_state TEXT NULL,
    cleanup_state TEXT NULL,
    verdict TEXT NULL,
    result_summary TEXT NULL,
    files_changed INTEGER NULL,
    additions INTEGER NULL,
    deletions INTEGER NULL,
    merge_commit TEXT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    started_at TEXT NULL,
    settled_at TEXT NULL,
    finished_at TEXT NULL,
    deleted_at TEXT NULL,
    pending_merge TEXT NULL,
    preflight TEXT NULL,
    archived_at TEXT NULL,
    scheduled_at TEXT NULL,
    task_status TEXT NOT NULL DEFAULT 'todo'
        CHECK (task_status IN ({allowed_statuses})),
    execution_mode TEXT NULL
        CHECK (execution_mode IN ('manual', 'session', 'engine'))
);
"#
    )
}

fn rebuild_sql(allowed_statuses: &str, normalize_backlog: bool) -> String {
    let mut sql = String::from("PRAGMA foreign_keys=OFF;\n");
    if normalize_backlog {
        sql.push_str("UPDATE work_task SET task_status = 'todo' WHERE task_status = 'backlog';\n");
    }
    sql.push_str(&table_sql(allowed_statuses));
    sql.push_str(&format!(
        "INSERT INTO work_task_new ({COMMON_COLUMNS}) SELECT {COMMON_COLUMNS} FROM work_task;\n"
    ));
    sql.push_str(
        r#"
DROP TABLE work_task;
ALTER TABLE work_task_new RENAME TO work_task;
CREATE INDEX idx_work_task_folder ON work_task(folder_id);
CREATE INDEX idx_work_task_status ON work_task(status);
CREATE INDEX idx_work_task_task_status ON work_task(task_status);
CREATE INDEX idx_work_task_execution_mode ON work_task(execution_mode);
CREATE TRIGGER trg_work_task_engine_projection
AFTER UPDATE OF status, execution_mode ON work_task
WHEN NEW.execution_mode = 'engine'
BEGIN
    UPDATE work_task
    SET task_status = CASE NEW.status
        WHEN 'todo' THEN 'todo'
        WHEN 'queued' THEN 'todo'
        WHEN 'preparing' THEN 'in_progress'
        WHEN 'running' THEN 'in_progress'
        WHEN 'awaiting_input' THEN 'blocked'
        WHEN 'review' THEN 'review'
        WHEN 'merging' THEN 'review'
        WHEN 'done' THEN 'done'
        WHEN 'failed' THEN 'blocked'
        WHEN 'canceled' THEN 'canceled'
        ELSE task_status
    END
    WHERE id = NEW.id;
END;
PRAGMA foreign_keys=ON;
"#,
    );
    sql
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(&rebuild_sql(
                "'backlog', 'todo', 'in_progress', 'blocked', 'review', 'done', 'canceled'",
                false,
            ))
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(&rebuild_sql(
                "'todo', 'in_progress', 'blocked', 'review', 'done', 'canceled'",
                true,
            ))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_orm::{ConnectionTrait, Database, DbBackend, Statement};

    async fn legacy_db() -> sea_orm_migration::sea_orm::DatabaseConnection {
        let conn = Database::connect("sqlite::memory:").await.expect("db");
        conn.execute_unprepared(&table_sql(
            "'todo', 'in_progress', 'blocked', 'review', 'done', 'canceled'",
        ))
        .await
        .expect("legacy table");
        conn.execute_unprepared("ALTER TABLE work_task_new RENAME TO work_task")
            .await
            .expect("legacy name");
        conn.execute_unprepared(
            "INSERT INTO work_task (folder_id, title, config, status, created_at, updated_at, task_status) \
             VALUES (7, 'Keep me', '{}', 'todo', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, 'todo')",
        )
        .await
        .expect("seed task");
        conn.execute_unprepared(
            "CREATE TABLE task_ref (id INTEGER PRIMARY KEY, task_id INTEGER NOT NULL REFERENCES work_task(id));\
             INSERT INTO task_ref (id, task_id) VALUES (9, 1);\
             PRAGMA foreign_keys=ON;",
        )
        .await
        .expect("seed referencing row");
        conn
    }

    #[tokio::test]
    async fn widens_check_without_changing_existing_identity() {
        let conn = legacy_db().await;
        Migration
            .up(&SchemaManager::new(&conn))
            .await
            .expect("migrate");

        conn.execute_unprepared("UPDATE work_task SET task_status = 'backlog' WHERE id = 1")
            .await
            .expect("backlog is legal");
        let row = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id, folder_id, title, task_status FROM work_task WHERE id = 1",
            ))
            .await
            .expect("query")
            .expect("row");
        assert_eq!(row.try_get::<i32>("", "id").unwrap(), 1);
        assert_eq!(row.try_get::<i32>("", "folder_id").unwrap(), 7);
        assert_eq!(row.try_get::<String>("", "title").unwrap(), "Keep me");
        assert_eq!(row.try_get::<String>("", "task_status").unwrap(), "backlog");
        assert!(conn
            .execute_unprepared("UPDATE work_task SET task_status = 'invented' WHERE id = 1")
            .await
            .is_err());
        let ref_task_id = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT task_id FROM task_ref WHERE id = 9",
            ))
            .await
            .expect("query reference")
            .expect("reference row")
            .try_get::<i32>("", "task_id")
            .unwrap();
        assert_eq!(ref_task_id, 1);
        assert!(conn
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA foreign_key_check",
            ))
            .await
            .expect("foreign key check")
            .is_empty());
    }

    #[tokio::test]
    async fn down_maps_parked_ideas_back_to_todo() {
        let conn = legacy_db().await;
        let manager = SchemaManager::new(&conn);
        Migration.up(&manager).await.expect("up");
        conn.execute_unprepared("UPDATE work_task SET task_status = 'backlog' WHERE id = 1")
            .await
            .expect("park");
        Migration.down(&manager).await.expect("down");

        let status = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT task_status FROM work_task WHERE id = 1",
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get::<String>("", "task_status")
            .unwrap();
        assert_eq!(status, "todo");
        assert!(conn
            .execute_unprepared("UPDATE work_task SET task_status = 'backlog' WHERE id = 1")
            .await
            .is_err());
    }
}
