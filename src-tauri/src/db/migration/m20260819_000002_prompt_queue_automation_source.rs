//! The prompt queue's `source` CHECK learns 'automation' (scheduled prompts an
//! automation enqueues into an existing Session). SQLite cannot alter a CHECK,
//! so the item table is rebuilt — same shape as the room rebuild in
//! m20260818_000008. 'automation' sits in the middle dispatch class with
//! 'collaboration'/'reminder' (see CLASS_ORDER_SQL in prompt_queue_service).

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
INSERT INTO conversation_prompt_queue_item_new (
    id, conversation_id, position, draft_json, origin_event_id, mode_id, state,
    client_dedupe_id, claimed_by, claim_expires_at, dispatch_started_at, attempts,
    paused_reason, created_at, updated_at, source
)
SELECT
    id, conversation_id, position, draft_json, origin_event_id, mode_id, state,
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
        CHECK (source IN ('user', 'collaboration', 'reminder', 'timer')),
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
    CASE WHEN source = 'automation' THEN 'user' ELSE source END
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

    /// Seed the pre-migration table shape (without the 'automation' CHECK
    /// arm), run the rebuild, and prove both old and new source values land.
    #[tokio::test]
    async fn rebuild_preserves_rows_and_accepts_automation_source() {
        let conn = Database::connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        conn.execute_unprepared(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE conversation (
                 id INTEGER PRIMARY KEY NOT NULL
             );
             CREATE TABLE conversation_prompt_queue_item (
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
                     CHECK (source IN ('user', 'collaboration', 'reminder', 'timer')),
                 FOREIGN KEY (conversation_id) REFERENCES conversation(id) ON DELETE CASCADE,
                 CHECK ((draft_json IS NOT NULL) <> (origin_event_id IS NOT NULL)),
                 UNIQUE (conversation_id, client_dedupe_id)
             );
             INSERT INTO conversation VALUES (1);
             INSERT INTO conversation_prompt_queue_item (
                 id, conversation_id, position, draft_json, client_dedupe_id, source
             ) VALUES ('keep-me', 1, 0, '{\"blocks\":[]}', 'keep-me', 'timer');",
        )
        .await
        .expect("seed");
        Migration
            .up(&SchemaManager::new(&conn))
            .await
            .expect("run migration up");

        let count: i64 = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS count FROM conversation_prompt_queue_item \
                 WHERE id = 'keep-me' AND source = 'timer'"
                    .to_owned(),
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get("", "count")
            .expect("column");
        assert_eq!(count, 1, "the rebuild must carry existing rows over");

        conn.execute_unprepared(
            "INSERT INTO conversation_prompt_queue_item (
                 id, conversation_id, position, draft_json, client_dedupe_id, source
             ) VALUES ('auto-1', 1, 1, '{\"blocks\":[]}', 'auto-1', 'automation');",
        )
        .await
        .expect("automation source is accepted after the rebuild");

        let rejected = conn
            .execute_unprepared(
                "INSERT INTO conversation_prompt_queue_item (
                     id, conversation_id, position, draft_json, client_dedupe_id, source
                 ) VALUES ('bogus', 1, 2, '{\"blocks\":[]}', 'bogus', 'bogus');",
            )
            .await;
        assert!(
            rejected.is_err(),
            "the CHECK must still reject junk sources"
        );
    }
}
