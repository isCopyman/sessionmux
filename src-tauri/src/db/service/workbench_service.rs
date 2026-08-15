use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, DatabaseConnection, EntityTrait,
    IntoActiveModel, PaginatorTrait, QueryFilter, QueryOrder, Set, TransactionTrait,
};
use std::collections::{HashMap, HashSet};

use crate::db::entities::{opened_tab, workbench};
use crate::db::error::DbError;
use crate::models::{ConversationWorkbenchRef, WorkbenchInfo};

fn to_info(model: workbench::Model) -> WorkbenchInfo {
    WorkbenchInfo {
        id: model.id,
        name: model.name,
        position: model.position,
        is_pinned: model.is_pinned,
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
        .order_by_desc(workbench::Column::IsPinned)
        .order_by_asc(workbench::Column::Position)
        .order_by_asc(workbench::Column::Id)
        .all(conn)
        .await?;
    Ok(rows.into_iter().map(to_info).collect())
}

pub async fn list_conversation_refs(
    conn: &DatabaseConnection,
    conversation_ids: Vec<i32>,
) -> Result<Vec<ConversationWorkbenchRef>, DbError> {
    let conversation_ids: HashSet<i32> = conversation_ids.into_iter().collect();
    if conversation_ids.is_empty() {
        return Ok(Vec::new());
    }
    if conversation_ids.len() > 2_000 {
        return Err(DbError::Validation(
            "At most 2000 conversation ids can be queried at once".into(),
        ));
    }

    let tabs = opened_tab::Entity::find()
        .filter(opened_tab::Column::ConversationId.is_in(conversation_ids))
        .all(conn)
        .await?;
    if tabs.is_empty() {
        return Ok(Vec::new());
    }

    let workbench_ids: HashSet<i32> = tabs.iter().map(|tab| tab.workbench_id).collect();
    let workbenches = workbench::Entity::find()
        .filter(workbench::Column::Id.is_in(workbench_ids))
        .all(conn)
        .await?;
    let workbench_by_id: HashMap<i32, workbench::Model> = workbenches
        .into_iter()
        .map(|item| (item.id, item))
        .collect();

    // Defensive de-duplication: a corrupt or legacy snapshot may contain the
    // same conversation twice, but Session Center should still show one badge
    // per workbench.
    let mut seen = HashSet::new();
    let mut refs = Vec::new();
    for tab in tabs {
        let Some(conversation_id) = tab.conversation_id else {
            continue;
        };
        if !seen.insert((conversation_id, tab.workbench_id)) {
            continue;
        }
        let Some(workbench) = workbench_by_id.get(&tab.workbench_id) else {
            continue;
        };
        refs.push(ConversationWorkbenchRef {
            conversation_id,
            workbench_id: workbench.id,
            workbench_name: workbench.name.clone(),
            workbench_position: workbench.position,
        });
    }
    refs.sort_by_key(|item| {
        (
            item.conversation_id,
            item.workbench_position,
            item.workbench_id,
        )
    });
    Ok(refs)
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
        is_pinned: Set(false),
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

pub async fn set_pinned(
    conn: &DatabaseConnection,
    id: i32,
    is_pinned: bool,
) -> Result<WorkbenchInfo, DbError> {
    let row = workbench::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("Workbench {id}")))?;
    if row.is_pinned == is_pinned {
        return Ok(to_info(row));
    }
    let mut active = row.into_active_model();
    active.is_pinned = Set(is_pinned);
    active.updated_at = Set(Utc::now());
    Ok(to_info(active.update(conn).await?))
}

pub async fn duplicate(
    conn: &DatabaseConnection,
    source_id: i32,
    requested_name: Option<String>,
) -> Result<WorkbenchInfo, DbError> {
    let txn = conn.begin().await?;
    let source = workbench::Entity::find_by_id(source_id)
        .one(&txn)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("Workbench {source_id}")))?;
    let max_position = workbench::Entity::find()
        .order_by_desc(workbench::Column::Position)
        .one(&txn)
        .await?
        .map(|row| row.position)
        .unwrap_or(-1);
    let default_name = format!("{} Copy", source.name);
    let name = normalize_name(requested_name.as_deref().unwrap_or(&default_name))?;
    let now = Utc::now();
    let created = workbench::ActiveModel {
        id: NotSet,
        name: Set(name),
        position: Set(max_position + 1),
        is_pinned: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&txn)
    .await?;

    let tabs = opened_tab::Entity::find()
        .filter(opened_tab::Column::WorkbenchId.eq(source_id))
        .order_by_asc(opened_tab::Column::Position)
        .order_by_asc(opened_tab::Column::Id)
        .all(&txn)
        .await?;
    for tab in tabs {
        opened_tab::ActiveModel {
            id: NotSet,
            workbench_id: Set(created.id),
            folder_id: Set(tab.folder_id),
            conversation_id: Set(tab.conversation_id),
            agent_type: Set(tab.agent_type),
            position: Set(tab.position),
            is_active: Set(tab.is_active),
            is_pinned: Set(tab.is_pinned),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&txn)
        .await?;
    }
    txn.commit().await?;
    Ok(to_info(created))
}

pub async fn reorder(
    conn: &DatabaseConnection,
    ordered_ids: Vec<i32>,
) -> Result<Vec<WorkbenchInfo>, DbError> {
    let txn = conn.begin().await?;
    let rows = workbench::Entity::find().all(&txn).await?;
    let existing: HashSet<i32> = rows.iter().map(|row| row.id).collect();
    let requested: HashSet<i32> = ordered_ids.iter().copied().collect();
    if ordered_ids.len() != rows.len()
        || requested.len() != ordered_ids.len()
        || requested != existing
    {
        return Err(DbError::Validation(
            "Workbench order must contain every workbench exactly once".into(),
        ));
    }

    let now = Utc::now();
    for (position, id) in ordered_ids.into_iter().enumerate() {
        let row = rows
            .iter()
            .find(|row| row.id == id)
            .expect("validated workbench id");
        if row.position == position as i32 {
            continue;
        }
        let mut active = row.clone().into_active_model();
        active.position = Set(position as i32);
        active.updated_at = Set(now);
        active.update(&txn).await?;
    }
    txn.commit().await?;
    list(conn).await
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
