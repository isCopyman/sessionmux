use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, DatabaseConnection, EntityTrait,
    IntoActiveModel, PaginatorTrait, QueryFilter, QueryOrder, Set, TransactionTrait,
};

use crate::db::entities::{opened_tab, workbench};
use crate::db::error::DbError;
use crate::models::WorkbenchInfo;

fn to_info(model: workbench::Model) -> WorkbenchInfo {
    WorkbenchInfo {
        id: model.id,
        name: model.name,
        position: model.position,
        created_at: model.created_at,
        updated_at: model.updated_at,
    }
}

fn normalize_name(name: &str) -> Result<String, DbError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(DbError::Validation("Workbench name cannot be empty".into()));
    }
    if name.chars().count() > 80 {
        return Err(DbError::Validation(
            "Workbench name cannot exceed 80 characters".into(),
        ));
    }
    Ok(name.to_string())
}

pub async fn list(conn: &DatabaseConnection) -> Result<Vec<WorkbenchInfo>, DbError> {
    let rows = workbench::Entity::find()
        .order_by_asc(workbench::Column::Position)
        .order_by_asc(workbench::Column::Id)
        .all(conn)
        .await?;
    Ok(rows.into_iter().map(to_info).collect())
}

pub async fn create(
    conn: &DatabaseConnection,
    requested_name: Option<String>,
) -> Result<WorkbenchInfo, DbError> {
    let max_position = workbench::Entity::find()
        .order_by_desc(workbench::Column::Position)
        .one(conn)
        .await?
        .map(|row| row.position)
        .unwrap_or(-1);
    let default_name = format!("Workbench {}", max_position + 2);
    let name = normalize_name(requested_name.as_deref().unwrap_or(&default_name))?;
    let now = Utc::now();
    let model = workbench::ActiveModel {
        id: NotSet,
        name: Set(name),
        position: Set(max_position + 1),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(conn)
    .await?;
    Ok(to_info(model))
}

pub async fn rename(
    conn: &DatabaseConnection,
    id: i32,
    name: String,
) -> Result<WorkbenchInfo, DbError> {
    let name = normalize_name(&name)?;
    let row = workbench::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("Workbench {id}")))?;
    let mut active = row.into_active_model();
    active.name = Set(name);
    active.updated_at = Set(Utc::now());
    Ok(to_info(active.update(conn).await?))
}

pub async fn delete(conn: &DatabaseConnection, id: i32) -> Result<(), DbError> {
    let txn = conn.begin().await?;
    let count = workbench::Entity::find().count(&txn).await?;
    if count <= 1 {
        return Err(DbError::Validation(
            "The last workbench cannot be deleted".into(),
        ));
    }
    if workbench::Entity::find_by_id(id).one(&txn).await?.is_none() {
        return Err(DbError::NotFound(format!("Workbench {id}")));
    }
    opened_tab::Entity::delete_many()
        .filter(opened_tab::Column::WorkbenchId.eq(id))
        .exec(&txn)
        .await?;
    workbench::Entity::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;
    Ok(())
}
