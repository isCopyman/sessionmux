//! Split the user-facing task workflow from the worktree engine lifecycle.
//!
//! Existing rows predate neutral/manual/session tasks, so they retain their
//! historical engine ownership. New rows may leave `execution_mode` NULL until
//! the user or an Agent decides how the task should be handled.

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
ALTER TABLE work_task
    ADD COLUMN task_status TEXT NOT NULL DEFAULT 'todo'
        CHECK (task_status IN ('todo', 'in_progress', 'blocked', 'review', 'done', 'canceled'));
ALTER TABLE work_task
    ADD COLUMN execution_mode TEXT NULL
        CHECK (execution_mode IN ('manual', 'session', 'engine'));

UPDATE work_task
SET task_status = CASE status
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
    ELSE 'todo'
END,
execution_mode = 'engine';

CREATE INDEX IF NOT EXISTS idx_work_task_task_status
    ON work_task(task_status);
CREATE INDEX IF NOT EXISTS idx_work_task_execution_mode
    ON work_task(execution_mode);

-- Keep the public board workflow in sync with the legacy worktree engine
-- without duplicating this mapping in every CAS transition. Manual/session
-- tasks own task_status themselves, so the trigger deliberately applies only
-- while the row is engine-backed.
CREATE TRIGGER IF NOT EXISTS trg_work_task_engine_projection
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
DROP INDEX IF EXISTS idx_work_task_execution_mode;
DROP INDEX IF EXISTS idx_work_task_task_status;
DROP TRIGGER IF EXISTS trg_work_task_engine_projection;
ALTER TABLE work_task DROP COLUMN execution_mode;
ALTER TABLE work_task DROP COLUMN task_status;
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
    async fn backfills_business_status_without_moving_legacy_engine_tasks() {
        let conn = Database::connect("sqlite::memory:")
            .await
            .expect("open sqlite");
        conn.execute_unprepared(
            "CREATE TABLE work_task (
                 id INTEGER PRIMARY KEY NOT NULL,
                 status TEXT NOT NULL
             );
             INSERT INTO work_task VALUES
                 (1, 'todo'), (2, 'running'), (3, 'awaiting_input'),
                 (4, 'merging'), (5, 'done'), (6, 'failed');",
        )
        .await
        .expect("seed legacy tasks");

        Migration
            .up(&SchemaManager::new(&conn))
            .await
            .expect("migrate");

        let rows = conn
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id, task_status, execution_mode FROM work_task ORDER BY id".to_owned(),
            ))
            .await
            .expect("query");
        let got: Vec<(i32, String, Option<String>)> = rows
            .into_iter()
            .map(|row| {
                (
                    row.try_get("", "id").expect("id"),
                    row.try_get("", "task_status").expect("task status"),
                    row.try_get("", "execution_mode").expect("mode"),
                )
            })
            .collect();
        assert_eq!(
            got,
            vec![
                (1, "todo".into(), Some("engine".into())),
                (2, "in_progress".into(), Some("engine".into())),
                (3, "blocked".into(), Some("engine".into())),
                (4, "review".into(), Some("engine".into())),
                (5, "done".into(), Some("engine".into())),
                (6, "blocked".into(), Some("engine".into())),
            ]
        );

        conn.execute_unprepared("UPDATE work_task SET status = 'review' WHERE id = 2")
            .await
            .expect("advance engine task");
        let task_status: String = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT task_status FROM work_task WHERE id = 2".to_owned(),
            ))
            .await
            .expect("query updated row")
            .expect("updated row")
            .try_get("", "task_status")
            .expect("task status");
        assert_eq!(task_status, "review");

        conn.execute_unprepared(
            "UPDATE work_task SET execution_mode = 'manual', task_status = 'blocked', status = 'done' WHERE id = 2",
        )
        .await
        .expect("advance manual task");
        let task_status: String = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT task_status FROM work_task WHERE id = 2".to_owned(),
            ))
            .await
            .expect("query manual row")
            .expect("manual row")
            .try_get("", "task_status")
            .expect("manual task status");
        assert_eq!(task_status, "blocked");
    }
}
