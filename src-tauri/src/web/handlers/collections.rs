use std::sync::Arc;

use axum::{extract::Extension, Json};
use serde::Deserialize;

use crate::app_error::AppCommandError;
use crate::app_state::AppState;
use crate::commands::collections;
use crate::models::{CollectionInfo, ConversationCollectionRef};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationIdsParams {
    pub conversation_ids: Vec<i32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCollectionParams {
    pub name: String,
    pub parent_id: Option<i32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameCollectionParams {
    pub id: i32,
    pub name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveCollectionParams {
    pub id: i32,
    pub parent_id: Option<i32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteCollectionParams {
    pub id: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignConversationsParams {
    pub conversation_ids: Vec<i32>,
    pub collection_id: Option<i32>,
}

pub async fn list_collections(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<Vec<CollectionInfo>>, AppCommandError> {
    Ok(Json(
        collections::list_collections_core(&state.db.conn).await?,
    ))
}

pub async fn list_conversation_collection_refs(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<ConversationIdsParams>,
) -> Result<Json<Vec<ConversationCollectionRef>>, AppCommandError> {
    Ok(Json(
        collections::list_conversation_collection_refs_core(
            &state.db.conn,
            params.conversation_ids,
        )
        .await?,
    ))
}

pub async fn create_collection(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<CreateCollectionParams>,
) -> Result<Json<CollectionInfo>, AppCommandError> {
    Ok(Json(
        collections::create_collection_core(&state.db.conn, params.name, params.parent_id).await?,
    ))
}

pub async fn rename_collection(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<RenameCollectionParams>,
) -> Result<Json<CollectionInfo>, AppCommandError> {
    Ok(Json(
        collections::rename_collection_core(&state.db.conn, params.id, params.name).await?,
    ))
}

pub async fn move_collection(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<MoveCollectionParams>,
) -> Result<Json<CollectionInfo>, AppCommandError> {
    Ok(Json(
        collections::move_collection_core(&state.db.conn, params.id, params.parent_id).await?,
    ))
}

pub async fn delete_collection(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<DeleteCollectionParams>,
) -> Result<Json<()>, AppCommandError> {
    collections::delete_collection_core(&state.db.conn, params.id).await?;
    Ok(Json(()))
}

pub async fn assign_conversations_to_collection(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<AssignConversationsParams>,
) -> Result<Json<Vec<ConversationCollectionRef>>, AppCommandError> {
    Ok(Json(
        collections::assign_conversations_to_collection_core(
            &state.db.conn,
            params.conversation_ids,
            params.collection_id,
        )
        .await?,
    ))
}
