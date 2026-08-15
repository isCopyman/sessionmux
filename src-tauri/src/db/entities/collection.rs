use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "collection")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Canonical repository/folder root that owns this semantic tree. NULL is
    /// reserved for Collections created before path-rooted organization.
    pub root_folder_id: Option<i32>,
    pub parent_id: Option<i32>,
    pub name: String,
    pub position: i32,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
