use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(CollaborationEvent::Table)
                    .add_column(
                        ColumnDef::new(CollaborationEvent::ChainDepth)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .get_connection()
            .execute_unprepared(
                r#"
CREATE INDEX IF NOT EXISTS idx_collaboration_event_reply_source
    ON collaboration_event(reply_to_event_id, source_conversation_id);
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
DROP INDEX IF EXISTS idx_collaboration_event_reply_source;
"#,
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(CollaborationEvent::Table)
                    .drop_column(CollaborationEvent::ChainDepth)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum CollaborationEvent {
    Table,
    ChainDepth,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_orm::{ConnectionTrait, Database, DbBackend, Statement};

    #[tokio::test]
    async fn up_backfills_root_depth_and_indexes_reply_lookup() {
        let conn = Database::connect("sqlite::memory:").await.unwrap();
        conn.execute_unprepared(
            "CREATE TABLE collaboration_event (\
                id TEXT PRIMARY KEY NOT NULL, \
                source_conversation_id INTEGER NOT NULL, \
                reply_to_event_id TEXT NULL\
            ); \
            INSERT INTO collaboration_event \
                (id, source_conversation_id, reply_to_event_id) \
                VALUES ('root', 1, NULL);",
        )
        .await
        .unwrap();

        Migration.up(&SchemaManager::new(&conn)).await.unwrap();

        let root = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT chain_depth FROM collaboration_event WHERE id = 'root'".to_string(),
            ))
            .await
            .unwrap()
            .unwrap();
        let depth: i32 = root.try_get("", "chain_depth").unwrap();
        assert_eq!(depth, 0);

        let indexes = conn
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA index_list('collaboration_event')".to_string(),
            ))
            .await
            .unwrap();
        assert!(indexes.iter().any(|row| {
            row.try_get::<String>("", "name").ok().as_deref()
                == Some("idx_collaboration_event_reply_source")
        }));
    }
}
