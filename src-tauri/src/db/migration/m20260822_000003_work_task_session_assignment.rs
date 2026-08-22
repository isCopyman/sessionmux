//! Let ordinary persistent Sessions own task-board cards.
//!
//! The assignment table is the durable responsibility record. The prompt
//! queue only carries the first task brief into the assigned Session, so it
//! gains a nullable `task_id` reference and a dedicated `task` scheduling
//! class. SQLite cannot extend the source CHECK in place, hence the rebuild.

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
PRAGMA foreign_keys=OFF;
CREATE TABLE conversation_prompt_queue_item_new (
    id TEXT PRIMARY KEY NOT NULL,
    conversation_id INTEGER NOT NULL,
    position INTEGER NOT NULL,
    draft_json TEXT NULL,
    origin_event_id TEXT NULL,
    task_id INTEGER NULL,
    mode_id TEXT NULL,
    state TEXT NOT NULL DEFAULT 'queued'
        CHECK (state IN ('queued', 'claimed', 'paused')),
    client_dedupe_id TEXT NOT NULL,
    claimed_by TEXT NULL,
    claim_expires_at TEXT NULL,
    dispatch_started_at TEXT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    paused_reason TEXT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    source TEXT NOT NULL DEFAULT 'user'
        CHECK (source IN ('user', 'collaboration', 'reminder', 'timer', 'automation', 'task')),
    FOREIGN KEY (conversation_id) REFERENCES conversation(id) ON DELETE CASCADE,
    FOREIGN KEY (task_id) REFERENCES work_task(id) ON DELETE SET NULL,
    CHECK ((draft_json IS NOT NULL) <> (origin_event_id IS NOT NULL)),
    UNIQUE (conversation_id, client_dedupe_id)
);
INSERT INTO conversation_prompt_queue_item_new (
    id, conversation_id, position, draft_json, origin_event_id, task_id, mode_id, state,
    client_dedupe_id, claimed_by, claim_expires_at, dispatch_started_at, attempts,
    paused_reason, created_at, updated_at, source
)
SELECT
    id, conversation_id, position, draft_json, origin_event_id, NULL, mode_id, state,
    client_dedupe_id, claimed_by, claim_expires_at, dispatch_started_at, attempts,
    paused_reason, created_at, updated_at, source
FROM conversation_prompt_queue_item;
DROP TABLE conversation_prompt_queue_item;
ALTER TABLE conversation_prompt_queue_item_new RENAME TO conversation_prompt_queue_item;
CREATE INDEX IF NOT EXISTS idx_conversation_prompt_queue_order
    ON conversation_prompt_queue_item(conversation_id, position, created_at, id);
CREATE INDEX IF NOT EXISTS idx_conversation_prompt_queue_claim
    ON conversation_prompt_queue_item(state, claim_expires_at);
CREATE INDEX IF NOT EXISTS idx_prompt_queue_claim_order
    ON conversation_prompt_queue_item(conversation_id, state, source, position);
CREATE INDEX IF NOT EXISTS idx_prompt_queue_task
    ON conversation_prompt_queue_item(task_id) WHERE task_id IS NOT NULL;

CREATE TABLE work_task_assignment (
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    task_id INTEGER NOT NULL,
    conversation_id INTEGER NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('owner', 'collaborator')),
    state TEXT NOT NULL CHECK (state IN ('assigned', 'active', 'completed', 'removed')),
    assigned_by TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    completed_at TEXT NULL,
    removed_at TEXT NULL,
    FOREIGN KEY (task_id) REFERENCES work_task(id) ON DELETE CASCADE,
    FOREIGN KEY (conversation_id) REFERENCES conversation(id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX idx_work_task_assignment_active_participant
    ON work_task_assignment(task_id, conversation_id)
    WHERE state IN ('assigned', 'active');
CREATE UNIQUE INDEX idx_work_task_assignment_active_owner
    ON work_task_assignment(task_id)
    WHERE role = 'owner' AND state IN ('assigned', 'active');
CREATE UNIQUE INDEX idx_work_task_assignment_active_owner_session
    ON work_task_assignment(conversation_id)
    WHERE role = 'owner' AND state IN ('assigned', 'active');
CREATE INDEX idx_work_task_assignment_conversation
    ON work_task_assignment(conversation_id, state, task_id);
PRAGMA foreign_keys=ON;
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
PRAGMA foreign_keys=OFF;
DROP TABLE IF EXISTS work_task_assignment;
CREATE TABLE conversation_prompt_queue_item_old (
    id TEXT PRIMARY KEY NOT NULL,
    conversation_id INTEGER NOT NULL,
    position INTEGER NOT NULL,
    draft_json TEXT NULL,
    origin_event_id TEXT NULL,
    mode_id TEXT NULL,
    state TEXT NOT NULL DEFAULT 'queued'
        CHECK (state IN ('queued', 'claimed', 'paused')),
    client_dedupe_id TEXT NOT NULL,
    claimed_by TEXT NULL,
    claim_expires_at TEXT NULL,
    dispatch_started_at TEXT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    paused_reason TEXT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    source TEXT NOT NULL DEFAULT 'user'
        CHECK (source IN ('user', 'collaboration', 'reminder', 'timer', 'automation')),
    FOREIGN KEY (conversation_id) REFERENCES conversation(id) ON DELETE CASCADE,
    CHECK ((draft_json IS NOT NULL) <> (origin_event_id IS NOT NULL)),
    UNIQUE (conversation_id, client_dedupe_id)
);
INSERT INTO conversation_prompt_queue_item_old (
    id, conversation_id, position, draft_json, origin_event_id, mode_id, state,
    client_dedupe_id, claimed_by, claim_expires_at, dispatch_started_at, attempts,
    paused_reason, created_at, updated_at, source
)
SELECT
    id, conversation_id, position, draft_json, origin_event_id, mode_id, state,
    client_dedupe_id, claimed_by, claim_expires_at, dispatch_started_at, attempts,
    paused_reason, created_at, updated_at,
    CASE WHEN source = 'task' THEN 'user' ELSE source END
FROM conversation_prompt_queue_item;
DROP TABLE conversation_prompt_queue_item;
ALTER TABLE conversation_prompt_queue_item_old RENAME TO conversation_prompt_queue_item;
CREATE INDEX IF NOT EXISTS idx_conversation_prompt_queue_order
    ON conversation_prompt_queue_item(conversation_id, position, created_at, id);
CREATE INDEX IF NOT EXISTS idx_conversation_prompt_queue_claim
    ON conversation_prompt_queue_item(state, claim_expires_at);
CREATE INDEX IF NOT EXISTS idx_prompt_queue_claim_order
    ON conversation_prompt_queue_item(conversation_id, state, source, position);
PRAGMA foreign_keys=ON;
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
    async fn adds_task_queue_reference_and_single_active_owner_guards() {
        let conn = Database::connect("sqlite::memory:").await.expect("db");
        conn.execute_unprepared(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE conversation (id INTEGER PRIMARY KEY NOT NULL);
             CREATE TABLE work_task (id INTEGER PRIMARY KEY NOT NULL);
             CREATE TABLE conversation_prompt_queue_item (
                 id TEXT PRIMARY KEY NOT NULL, conversation_id INTEGER NOT NULL,
                 position INTEGER NOT NULL, draft_json TEXT NULL, origin_event_id TEXT NULL,
                 mode_id TEXT NULL, state TEXT NOT NULL DEFAULT 'queued',
                 client_dedupe_id TEXT NOT NULL, claimed_by TEXT NULL,
                 claim_expires_at TEXT NULL, dispatch_started_at TEXT NULL,
                 attempts INTEGER NOT NULL DEFAULT 0, paused_reason TEXT NULL,
                 created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                 updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                 source TEXT NOT NULL DEFAULT 'user',
                 UNIQUE (conversation_id, client_dedupe_id)
             );
             INSERT INTO conversation VALUES (1), (2);
             INSERT INTO work_task VALUES (7);",
        )
        .await
        .expect("seed");
        Migration
            .up(&SchemaManager::new(&conn))
            .await
            .expect("migrate");

        conn.execute_unprepared(
            "INSERT INTO conversation_prompt_queue_item
             (id, conversation_id, position, draft_json, task_id, client_dedupe_id, source)
             VALUES ('task:7', 1, 0, '{}', 7, 'task:7', 'task');
             INSERT INTO work_task_assignment
             (task_id, conversation_id, role, state, assigned_by)
             VALUES (7, 1, 'owner', 'assigned', 'user');",
        )
        .await
        .expect("new rows accepted");
        assert!(conn
            .execute_unprepared(
                "INSERT INTO work_task_assignment
                 (task_id, conversation_id, role, state, assigned_by)
                 VALUES (7, 2, 'owner', 'active', 'user');"
            )
            .await
            .is_err());

        let task_id: i32 = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT task_id FROM conversation_prompt_queue_item WHERE id = 'task:7'",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "task_id")
            .unwrap();
        assert_eq!(task_id, 7);
    }
}
