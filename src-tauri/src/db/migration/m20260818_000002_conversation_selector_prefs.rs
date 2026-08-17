//! Per-conversation selector preferences (model / thinking effort / mode).
//!
//! Selector choices used to live only in frontend localStorage keyed by
//! agentType, so changing the model in one Session silently changed what
//! every other Session of that agent reconnected with. These columns pin the
//! choice to the conversation: `preferred_mode_id` mirrors the ACP session
//! mode, `preferred_config_values` is a JSON object of configId → valueId
//! (model, thinking effort, sandbox, …). NULL means "no per-Session choice
//! yet" and the agent-level template still applies.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Conversation::Table)
                    .add_column(ColumnDef::new(Conversation::PreferredModeId).text())
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Conversation::Table)
                    .add_column(ColumnDef::new(Conversation::PreferredConfigValues).text())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Conversation::Table)
                    .drop_column(Conversation::PreferredConfigValues)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Conversation::Table)
                    .drop_column(Conversation::PreferredModeId)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Conversation {
    Table,
    PreferredModeId,
    PreferredConfigValues,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_orm::{ConnectionTrait, Database, DbBackend, Statement};

    #[tokio::test]
    async fn up_adds_nullable_selector_pref_columns() {
        let conn = Database::connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        conn.execute_unprepared(
            "CREATE TABLE conversation (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT)",
        )
        .await
        .expect("create stub table");
        conn.execute_unprepared("INSERT INTO conversation (title) VALUES ('x')")
            .await
            .expect("insert row");

        Migration
            .up(&SchemaManager::new(&conn))
            .await
            .expect("run migration up");

        let rows = conn
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT preferred_mode_id, preferred_config_values FROM conversation".to_owned(),
            ))
            .await
            .expect("query rows");
        let mode: Option<String> = rows[0].try_get("", "preferred_mode_id").expect("column");
        let values: Option<String> = rows[0]
            .try_get("", "preferred_config_values")
            .expect("column");
        assert!(mode.is_none());
        assert!(values.is_none());
    }
}
