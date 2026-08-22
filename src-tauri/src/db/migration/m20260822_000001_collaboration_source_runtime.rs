//! Freeze the authoring model and Claude launch profile on collaboration
//! events. A Room post is historical content: changing the Session selector
//! later must not rewrite what the post says it was authored with.

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
    ADD COLUMN source_model_snapshot TEXT NULL;
ALTER TABLE collaboration_event
    ADD COLUMN source_profile_snapshot TEXT NULL;
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
ALTER TABLE collaboration_event DROP COLUMN source_profile_snapshot;
ALTER TABLE collaboration_event DROP COLUMN source_model_snapshot;
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
    async fn adds_optional_source_runtime_snapshots() {
        let conn = Database::connect("sqlite::memory:")
            .await
            .expect("open sqlite");
        conn.execute_unprepared(
            "CREATE TABLE collaboration_event (
                 id TEXT PRIMARY KEY NOT NULL,
                 body TEXT NOT NULL
             );
             INSERT INTO collaboration_event (id, body) VALUES ('old', 'before migration');",
        )
        .await
        .expect("stub event");

        Migration
            .up(&SchemaManager::new(&conn))
            .await
            .expect("migrate");

        let row = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT source_model_snapshot, source_profile_snapshot \
                 FROM collaboration_event WHERE id = 'old'"
                    .to_owned(),
            ))
            .await
            .expect("query")
            .expect("row");
        let model: Option<String> = row.try_get("", "source_model_snapshot").expect("model");
        let profile: Option<String> = row.try_get("", "source_profile_snapshot").expect("profile");
        assert_eq!(model, None);
        assert_eq!(profile, None);
    }
}
