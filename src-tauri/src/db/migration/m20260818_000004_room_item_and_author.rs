//! Room as a first-class Collection item, plus author/mention columns.
//!
//! Room placement must not inherit from the creator Session. Human posts are
//! marked `author_kind='human'` instead of impersonating that Session in the UI.
//! Structured `@human` is recorded on the event; it does not create a Session
//! delivery.

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
ALTER TABLE collaboration_room
    ADD COLUMN collection_id INTEGER NULL REFERENCES collection(id) ON DELETE SET NULL;
ALTER TABLE collaboration_room
    ADD COLUMN root_folder_id INTEGER NULL;

UPDATE collaboration_room
SET root_folder_id = (
    SELECT COALESCE(f.parent_id, f.id)
    FROM conversation c
    JOIN folder f ON f.id = c.folder_id
    WHERE c.id = collaboration_room.created_by_conversation_id
);

UPDATE collaboration_room
SET collection_id = (
    SELECT cc.collection_id
    FROM collection_conversation cc
    WHERE cc.conversation_id = collaboration_room.created_by_conversation_id
);

CREATE INDEX IF NOT EXISTS idx_collaboration_room_collection
    ON collaboration_room(collection_id, updated_at DESC, id);
CREATE INDEX IF NOT EXISTS idx_collaboration_room_root_folder
    ON collaboration_room(root_folder_id, updated_at DESC, id);

ALTER TABLE collaboration_event
    ADD COLUMN author_kind TEXT NOT NULL DEFAULT 'session'
        CHECK (author_kind IN ('session', 'human'));
ALTER TABLE collaboration_event
    ADD COLUMN mention_human INTEGER NOT NULL DEFAULT 0
        CHECK (mention_human IN (0, 1));
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
DROP INDEX IF EXISTS idx_collaboration_room_root_folder;
DROP INDEX IF EXISTS idx_collaboration_room_collection;
ALTER TABLE collaboration_event DROP COLUMN mention_human;
ALTER TABLE collaboration_event DROP COLUMN author_kind;
ALTER TABLE collaboration_room DROP COLUMN root_folder_id;
ALTER TABLE collaboration_room DROP COLUMN collection_id;
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
    async fn up_adds_room_placement_and_author_columns() {
        let conn = Database::connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        conn.execute_unprepared(
            "CREATE TABLE folder (
                 id INTEGER PRIMARY KEY,
                 parent_id INTEGER NULL
             );
             CREATE TABLE conversation (
                 id INTEGER PRIMARY KEY,
                 folder_id INTEGER NOT NULL
             );
             CREATE TABLE collection (id INTEGER PRIMARY KEY);
             CREATE TABLE collection_conversation (
                 conversation_id INTEGER PRIMARY KEY,
                 collection_id INTEGER NOT NULL
             );
             CREATE TABLE collaboration_room (
                 id TEXT PRIMARY KEY NOT NULL,
                 workbench_id INTEGER NOT NULL,
                 title TEXT NOT NULL,
                 status TEXT NOT NULL DEFAULT 'active',
                 created_by_conversation_id INTEGER NOT NULL,
                 last_seen_at TEXT NULL,
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             );
             CREATE TABLE collaboration_event (
                 id TEXT PRIMARY KEY NOT NULL,
                 source_conversation_id INTEGER NOT NULL,
                 body TEXT NOT NULL
             );
             INSERT INTO folder (id, parent_id) VALUES (7, NULL);
             INSERT INTO conversation (id, folder_id) VALUES (2, 7);
             INSERT INTO collection (id) VALUES (11);
             INSERT INTO collection_conversation (conversation_id, collection_id)
             VALUES (2, 11);
             INSERT INTO collaboration_room
                 (id, workbench_id, title, created_by_conversation_id, created_at, updated_at)
             VALUES ('rm_test', 1, 'Plan', 2, '2026-08-18', '2026-08-18');
             INSERT INTO collaboration_event (id, source_conversation_id, body)
             VALUES ('evt-1', 2, 'hello');",
        )
        .await
        .expect("stub tables");

        Migration
            .up(&SchemaManager::new(&conn))
            .await
            .expect("run migration up");

        let collection_id: Option<i32> = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT collection_id FROM collaboration_room WHERE id = 'rm_test'".to_owned(),
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get("", "collection_id")
            .expect("column");
        let root_folder_id: Option<i32> = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT root_folder_id FROM collaboration_room WHERE id = 'rm_test'".to_owned(),
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get("", "root_folder_id")
            .expect("column");
        assert_eq!(collection_id, Some(11));
        assert_eq!(root_folder_id, Some(7));

        let author_kind: String = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT author_kind FROM collaboration_event WHERE id = 'evt-1'".to_owned(),
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get("", "author_kind")
            .expect("column");
        assert_eq!(author_kind, "session");
    }
}
