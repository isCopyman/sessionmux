use std::sync::Arc;

use axum::{extract::Extension, Json};
use serde::Deserialize;

use crate::app_error::AppCommandError;
use crate::app_state::AppState;
use crate::commands::prompt_queue;
use crate::models::{EnqueuePromptQueueItem, PromptQueueDraft, PromptQueueSnapshot};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationParams {
    pub conversation_id: i32,
}

#[derive(Deserialize)]
pub struct EnqueueParams {
    pub input: EnqueuePromptQueueItem,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditParams {
    pub conversation_id: i32,
    pub id: String,
    pub draft: PromptQueueDraft,
    pub expected_revision: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteOrRetryParams {
    pub conversation_id: i32,
    pub id: String,
    pub expected_revision: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorderParams {
    pub conversation_id: i32,
    pub ordered_ids: Vec<String>,
    pub expected_revision: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeParams {
    pub conversation_id: i32,
    pub expected_revision: i64,
}

pub async fn get(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<ConversationParams>,
) -> Result<Json<PromptQueueSnapshot>, AppCommandError> {
    Ok(Json(
        prompt_queue::prompt_queue_get_core(&state.db.conn, params.conversation_id).await?,
    ))
}

pub async fn enqueue(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<EnqueueParams>,
) -> Result<Json<PromptQueueSnapshot>, AppCommandError> {
    Ok(Json(
        prompt_queue::prompt_queue_enqueue_core(
            &state.db.conn,
            &state.emitter,
            &state.prompt_queue,
            params.input,
        )
        .await?,
    ))
}

pub async fn edit(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<EditParams>,
) -> Result<Json<PromptQueueSnapshot>, AppCommandError> {
    Ok(Json(
        prompt_queue::prompt_queue_edit_core(
            &state.db.conn,
            &state.emitter,
            params.conversation_id,
            params.id,
            params.draft,
            params.expected_revision,
        )
        .await?,
    ))
}

pub async fn delete(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<DeleteOrRetryParams>,
) -> Result<Json<PromptQueueSnapshot>, AppCommandError> {
    Ok(Json(
        prompt_queue::prompt_queue_delete_core(
            &state.db.conn,
            &state.emitter,
            params.conversation_id,
            params.id,
            params.expected_revision,
        )
        .await?,
    ))
}

pub async fn reorder(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<ReorderParams>,
) -> Result<Json<PromptQueueSnapshot>, AppCommandError> {
    Ok(Json(
        prompt_queue::prompt_queue_reorder_core(
            &state.db.conn,
            &state.emitter,
            params.conversation_id,
            params.ordered_ids,
            params.expected_revision,
        )
        .await?,
    ))
}

pub async fn resume(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<ResumeParams>,
) -> Result<Json<PromptQueueSnapshot>, AppCommandError> {
    Ok(Json(
        prompt_queue::prompt_queue_resume_core(
            &state.db.conn,
            &state.emitter,
            &state.prompt_queue,
            params.conversation_id,
            params.expected_revision,
        )
        .await?,
    ))
}

pub async fn pause_manual(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<ResumeParams>,
) -> Result<Json<PromptQueueSnapshot>, AppCommandError> {
    Ok(Json(
        prompt_queue::prompt_queue_pause_manual_core(
            &state.db.conn,
            &state.emitter,
            params.conversation_id,
            params.expected_revision,
        )
        .await?,
    ))
}

pub async fn release_one(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<DeleteOrRetryParams>,
) -> Result<Json<PromptQueueSnapshot>, AppCommandError> {
    Ok(Json(
        prompt_queue::prompt_queue_release_one_core(
            &state.db.conn,
            &state.emitter,
            &state.prompt_queue,
            params.conversation_id,
            params.id,
            params.expected_revision,
        )
        .await?,
    ))
}

pub async fn retry(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<DeleteOrRetryParams>,
) -> Result<Json<PromptQueueSnapshot>, AppCommandError> {
    Ok(Json(
        prompt_queue::prompt_queue_retry_core(
            &state.db.conn,
            &state.emitter,
            &state.prompt_queue,
            params.conversation_id,
            params.id,
            params.expected_revision,
        )
        .await?,
    ))
}
