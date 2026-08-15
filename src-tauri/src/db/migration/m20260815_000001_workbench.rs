use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Workbench::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Workbench::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Workbench::Name).string().not_null())
                    .col(
                        ColumnDef::new(Workbench::Position)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Workbench::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Workbench::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // ID 1 is the compatibility workbench used by every pre-workbench
        // opened_tab row and by older clients that call the legacy tab API.
        manager
            .get_connection()
            .execute_unprepared(
                "INSERT OR IGNORE INTO workbench \
                 (id, name, position, created_at, updated_at) \
                 VALUES (1, 'Main', 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            )
            .await?;

        // SQLite cannot add a foreign-key constraint with ADD COLUMN. The
        // service layer owns workbench deletion and removes its tabs in the
        // same transaction; DEFAULT 1 migrates every existing row safely.
        manager
            .alter_table(
                Table::alter()
                    .table(OpenedTab::Table)
                    .add_column(
                        ColumnDef::new(OpenedTab::WorkbenchId)
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_opened_tabs_workbench_position")
                    .table(OpenedTab::Table)
                    .col(OpenedTab::WorkbenchId)
                    .col(OpenedTab::Position)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_opened_tabs_workbench_position")
                    .table(OpenedTab::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(OpenedTab::Table)
                    .drop_column(OpenedTab::WorkbenchId)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(Workbench::Table).if_exists().to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Workbench {
    Table,
    Id,
    Name,
    Position,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum OpenedTab {
    Table,
    WorkbenchId,
    Position,
}
