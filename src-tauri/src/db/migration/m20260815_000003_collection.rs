use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Collection::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Collection::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Collection::ParentId).integer().null())
                    .col(ColumnDef::new(Collection::Name).string().not_null())
                    .col(
                        ColumnDef::new(Collection::Position)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Collection::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Collection::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_collection_parent")
                            .from(Collection::Table, Collection::ParentId)
                            .to(Collection::Table, Collection::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_collection_parent_position")
                    .table(Collection::Table)
                    .col(Collection::ParentId)
                    .col(Collection::Position)
                    .to_owned(),
            )
            .await?;

        // conversation_id is the primary key: a Session can have zero or one
        // semantic home, while shortcuts such as Recent and Workbench remain
        // independent views rather than additional Collection memberships.
        manager
            .create_table(
                Table::create()
                    .table(CollectionConversation::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CollectionConversation::ConversationId)
                            .integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(CollectionConversation::CollectionId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CollectionConversation::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(CollectionConversation::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_collection_conversation_collection")
                            .from(
                                CollectionConversation::Table,
                                CollectionConversation::CollectionId,
                            )
                            .to(Collection::Table, Collection::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_collection_conversation_conversation")
                            .from(
                                CollectionConversation::Table,
                                CollectionConversation::ConversationId,
                            )
                            .to(Conversation::Table, Conversation::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_collection_conversation_collection")
                    .table(CollectionConversation::Table)
                    .col(CollectionConversation::CollectionId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(CollectionConversation::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(Collection::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Collection {
    Table,
    Id,
    ParentId,
    Name,
    Position,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum CollectionConversation {
    Table,
    ConversationId,
    CollectionId,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Conversation {
    Table,
    Id,
}
