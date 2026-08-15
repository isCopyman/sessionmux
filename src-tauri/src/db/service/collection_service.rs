use chrono::Utc;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, IntoActiveModel, PaginatorTrait, QueryFilter, QueryOrder, Set, TransactionTrait,
};
use std::collections::HashSet;

use crate::db::entities::{collection, collection_conversation, conversation};
use crate::db::error::DbError;
use crate::models::{CollectionInfo, ConversationCollectionRef};

fn to_info(model: collection::Model) -> CollectionInfo {
    CollectionInfo {
        id: model.id,
        parent_id: model.parent_id,
        name: model.name,
        position: model.position,
        created_at: model.created_at,
        updated_at: model.updated_at,
    }
}

fn normalize_name(name: &str) -> Result<String, DbError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(DbError::Validation(
            "Collection name cannot be empty".into(),
        ));
    }
    if name.chars().count() > 80 {
        return Err(DbError::Validation(
            "Collection name cannot exceed 80 characters".into(),
        ));
    }
    Ok(name.to_string())
}

async fn require_collection<C: ConnectionTrait>(conn: &C, id: i32) -> Result<(), DbError> {
    if collection::Entity::find_by_id(id)
        .one(conn)
        .await?
        .is_none()
    {
        return Err(DbError::NotFound(format!("Collection {id}")));
    }
    Ok(())
}

async fn next_position<C: ConnectionTrait>(
    conn: &C,
    parent_id: Option<i32>,
) -> Result<i32, DbError> {
    let mut query = collection::Entity::find().order_by_desc(collection::Column::Position);
    query = match parent_id {
        Some(id) => query.filter(collection::Column::ParentId.eq(id)),
        None => query.filter(collection::Column::ParentId.is_null()),
    };
    Ok(query
        .one(conn)
        .await?
        .map(|row| row.position + 1)
        .unwrap_or(0))
}

pub async fn list(conn: &DatabaseConnection) -> Result<Vec<CollectionInfo>, DbError> {
    let rows = collection::Entity::find()
        .order_by_asc(collection::Column::ParentId)
        .order_by_asc(collection::Column::Position)
        .order_by_asc(collection::Column::Id)
        .all(conn)
        .await?;
    Ok(rows.into_iter().map(to_info).collect())
}

pub async fn create(
    conn: &DatabaseConnection,
    name: String,
    parent_id: Option<i32>,
) -> Result<CollectionInfo, DbError> {
    let name = normalize_name(&name)?;
    if let Some(id) = parent_id {
        require_collection(conn, id).await?;
    }
    let now = Utc::now();
    let model = collection::ActiveModel {
        id: NotSet,
        parent_id: Set(parent_id),
        name: Set(name),
        position: Set(next_position(conn, parent_id).await?),
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
) -> Result<CollectionInfo, DbError> {
    let name = normalize_name(&name)?;
    let row = collection::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("Collection {id}")))?;
    let mut active = row.into_active_model();
    active.name = Set(name);
    active.updated_at = Set(Utc::now());
    Ok(to_info(active.update(conn).await?))
}

pub async fn move_to(
    conn: &DatabaseConnection,
    id: i32,
    parent_id: Option<i32>,
) -> Result<CollectionInfo, DbError> {
    if parent_id == Some(id) {
        return Err(DbError::Validation(
            "A Collection cannot contain itself".into(),
        ));
    }
    let row = collection::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("Collection {id}")))?;
    if row.parent_id == parent_id {
        return Ok(to_info(row));
    }

    // Walk upward from the requested parent. Seeing `id` means the move would
    // make a descendant the new parent and create a cycle in the visible tree.
    let mut cursor = parent_id;
    while let Some(candidate) = cursor {
        if candidate == id {
            return Err(DbError::Validation(
                "A Collection cannot be moved into its descendant".into(),
            ));
        }
        let parent = collection::Entity::find_by_id(candidate)
            .one(conn)
            .await?
            .ok_or_else(|| DbError::NotFound(format!("Collection {candidate}")))?;
        cursor = parent.parent_id;
    }

    let position = next_position(conn, parent_id).await?;
    let mut active = row.into_active_model();
    active.parent_id = Set(parent_id);
    active.position = Set(position);
    active.updated_at = Set(Utc::now());
    Ok(to_info(active.update(conn).await?))
}

pub async fn delete(conn: &DatabaseConnection, id: i32) -> Result<(), DbError> {
    let txn = conn.begin().await?;
    let row = collection::Entity::find_by_id(id)
        .one(&txn)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("Collection {id}")))?;

    // Preserve the hierarchy without inventing a destructive cascade: direct
    // children move one level up, while Sessions in the deleted Collection
    // become Unclassified. No conversation row or running agent is touched.
    collection::Entity::update_many()
        .col_expr(collection::Column::ParentId, Expr::value(row.parent_id))
        .col_expr(collection::Column::UpdatedAt, Expr::value(Utc::now()))
        .filter(collection::Column::ParentId.eq(id))
        .exec(&txn)
        .await?;
    collection_conversation::Entity::delete_many()
        .filter(collection_conversation::Column::CollectionId.eq(id))
        .exec(&txn)
        .await?;
    collection::Entity::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;
    Ok(())
}

pub async fn list_conversation_refs(
    conn: &DatabaseConnection,
    conversation_ids: Vec<i32>,
) -> Result<Vec<ConversationCollectionRef>, DbError> {
    let ids: HashSet<i32> = conversation_ids.into_iter().collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    if ids.len() > 10_000 {
        return Err(DbError::Validation(
            "At most 10000 conversation ids can be queried at once".into(),
        ));
    }
    let mut refs: Vec<ConversationCollectionRef> = collection_conversation::Entity::find()
        .filter(collection_conversation::Column::ConversationId.is_in(ids))
        .all(conn)
        .await?
        .into_iter()
        .map(|row| ConversationCollectionRef {
            conversation_id: row.conversation_id,
            collection_id: row.collection_id,
        })
        .collect();
    refs.sort_by_key(|item| item.conversation_id);
    Ok(refs)
}

pub async fn assign_conversations(
    conn: &DatabaseConnection,
    conversation_ids: Vec<i32>,
    collection_id: Option<i32>,
) -> Result<Vec<ConversationCollectionRef>, DbError> {
    let ids: HashSet<i32> = conversation_ids.into_iter().collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    if ids.len() > 2_000 {
        return Err(DbError::Validation(
            "At most 2000 conversations can be moved at once".into(),
        ));
    }

    let txn = conn.begin().await?;
    if let Some(id) = collection_id {
        require_collection(&txn, id).await?;
    }
    let live_count = conversation::Entity::find()
        .filter(conversation::Column::Id.is_in(ids.clone()))
        .filter(conversation::Column::DeletedAt.is_null())
        .count(&txn)
        .await?;
    if live_count != ids.len() as u64 {
        return Err(DbError::Validation(
            "Every selected Session must still exist".into(),
        ));
    }

    collection_conversation::Entity::delete_many()
        .filter(collection_conversation::Column::ConversationId.is_in(ids.clone()))
        .exec(&txn)
        .await?;
    if let Some(collection_id) = collection_id {
        let now = Utc::now();
        for conversation_id in ids.iter().copied() {
            collection_conversation::ActiveModel {
                conversation_id: Set(conversation_id),
                collection_id: Set(collection_id),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&txn)
            .await?;
        }
    }
    txn.commit().await?;
    list_conversation_refs(conn, ids.into_iter().collect()).await
}
