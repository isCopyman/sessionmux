use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Nullable is intentional for compatibility with Collections created by
        // the first global-Collection build. New UI writes a root folder; legacy
        // rows remain visible until the user places them under a path.
        manager
            .alter_table(
                Table::alter()
                    .table(Collection::Table)
                    .add_column(ColumnDef::new(Collection::RootFolderId).integer().null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_collection_root_parent_position")
                    .table(Collection::Table)
                    .col(Collection::RootFolderId)
                    .col(Collection::ParentId)
                    .col(Collection::Position)
                    .to_owned(),
            )
            .await?;

        // Safely adopt old Collections that directly contain Sessions from one
        // repository family. Worktree folders already point at their canonical
        // repository through folder.parent_id. Mixed/empty legacy Collections
        // stay NULL instead of being guessed into the wrong path.
        manager
            .get_connection()
            .execute_unprepared(
                "UPDATE collection AS target \
                 SET root_folder_id = ( \
                   SELECT MIN(COALESCE(f.parent_id, c.folder_id)) \
                   FROM collection_conversation cc \
                   JOIN conversation c ON c.id = cc.conversation_id \
                   JOIN folder f ON f.id = c.folder_id \
                   WHERE cc.collection_id = target.id \
                   HAVING COUNT(DISTINCT COALESCE(f.parent_id, c.folder_id)) = 1 \
                 ) \
                 WHERE target.root_folder_id IS NULL",
            )
            .await?;

        // Once a rooted ancestor is known, its existing descendants belong to
        // that same taxonomy. The recursive CTE handles arbitrary depth without
        // rewriting any Session folder/cwd.
        manager
            .get_connection()
            .execute_unprepared(
                "WITH RECURSIVE rooted(id, root_folder_id) AS ( \
                   SELECT id, root_folder_id FROM collection \
                   WHERE root_folder_id IS NOT NULL \
                   UNION ALL \
                   SELECT child.id, rooted.root_folder_id \
                   FROM collection child \
                   JOIN rooted ON child.parent_id = rooted.id \
                   WHERE child.root_folder_id IS NULL \
                 ) \
                 UPDATE collection \
                 SET root_folder_id = ( \
                   SELECT rooted.root_folder_id FROM rooted \
                   WHERE rooted.id = collection.id LIMIT 1 \
                 ) \
                 WHERE root_folder_id IS NULL \
                   AND id IN (SELECT id FROM rooted)",
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_collection_root_parent_position")
                    .table(Collection::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Collection::Table)
                    .drop_column(Collection::RootFolderId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Collection {
    Table,
    ParentId,
    Position,
    RootFolderId,
}
