//! Shared Room ledger on top of collaboration events.
//!
//! Mail stays `visibility='direct'` (the column default). Room posts write
//! `visibility='room'` plus `room_id`. Existing INSERT paths that omit the
//! new columns keep creating private letters.

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
ALTER TABLE collaboration_event
    ADD COLUMN visibility TEXT NOT NULL DEFAULT 'direct';
ALTER TABLE collaboration_event
    ADD COLUMN room_id TEXT NULL;

CREATE TABLE IF NOT EXISTS collaboration_room (
    id TEXT PRIMARY KEY NOT NULL,
    workbench_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    created_by_conversation_id INTEGER NOT NULL,
    last_seen_at TEXT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (workbench_id) REFERENCES workbench(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS collaboration_room_member (
    room_id TEXT NOT NULL,
    conversation_id INTEGER NOT NULL,
    role TEXT NOT NULL DEFAULT 'member',
    joined_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_read_at TEXT NULL,
    PRIMARY KEY (room_id, conversation_id),
    FOREIGN KEY (room_id) REFERENCES collaboration_room(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_collaboration_event_room_created
    ON collaboration_event(room_id, created_at, id);
CREATE INDEX IF NOT EXISTS idx_collaboration_room_workbench
    ON collaboration_room(workbench_id, updated_at DESC, id);
CREATE INDEX IF NOT EXISTS idx_collaboration_room_member_session
    ON collaboration_room_member(conversation_id, room_id);
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
DROP INDEX IF EXISTS idx_collaboration_room_member_session;
DROP INDEX IF EXISTS idx_collaboration_room_workbench;
DROP INDEX IF EXISTS idx_collaboration_event_room_created;
DROP TABLE IF EXISTS collaboration_room_member;
DROP TABLE IF EXISTS collaboration_room;
ALTER TABLE collaboration_event DROP COLUMN room_id;
ALTER TABLE collaboration_event DROP COLUMN visibility;
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
    async fn up_adds_room_tables_and_defaults_existing_events_to_direct() {
        let conn = Database::connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        conn.execute_unprepared(
            "CREATE TABLE workbench (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
             INSERT INTO workbench (id, name) VALUES (1, 'Main');
             CREATE TABLE collaboration_event (
                 id TEXT PRIMARY KEY NOT NULL,
                 source_conversation_id INTEGER NOT NULL,
                 body TEXT NOT NULL
             );
             INSERT INTO collaboration_event (id, source_conversation_id, body)
             VALUES ('evt-1', 1, 'hello');",
        )
        .await
        .expect("stub tables");

        Migration
            .up(&SchemaManager::new(&conn))
            .await
            .expect("run migration up");

        let visibility: String = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT visibility FROM collaboration_event WHERE id = 'evt-1'".to_owned(),
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get("", "visibility")
            .expect("column");
        assert_eq!(visibility, "direct");

        conn.execute_unprepared(
            "INSERT INTO collaboration_room \
             (id, workbench_id, title, created_by_conversation_id) \
             VALUES ('rm_test', 1, 'Plan', 2);
             INSERT INTO collaboration_room_member (room_id, conversation_id, role)
             VALUES ('rm_test', 2, 'owner');",
        )
        .await
        .expect("insert room");
        let count: i64 = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS count FROM collaboration_room".to_owned(),
            ))
            .await
            .expect("count")
            .expect("row")
            .try_get("", "count")
            .expect("column");
        assert_eq!(count, 1);
    }
}
