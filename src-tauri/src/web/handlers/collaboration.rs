use std::sync::Arc;

use axum::{extract::Extension, Json};
use serde::Deserialize;

use crate::app_error::AppCommandError;
use crate::app_state::AppState;
use crate::commands::collaboration;
use crate::commands::collaboration::SessionCollaborationSettings;
use crate::models::{
    AddCollaborationRoomMembersInput, CollaborationFeed, CollaborationInterruptResult,
    CollaborationRoomDetail, CollaborationRoomSummary, CollaborationSendResult,
    CollaborationTimelineProjection, CollaborationUnreadOverview, CreateCollaborationRoomInput,
    InterruptCollaborationInput, PostRoomMessageInput, RoomPostResult, RoomTimeline,
    SendAndInterruptCollaborationInput, SendAndInterruptCollaborationResult,
    SendCollaborationMessageInput,
};

#[derive(Deserialize)]
pub struct SendParams {
    pub input: SendCollaborationMessageInput,
}

#[derive(Deserialize)]
pub struct InterruptParams {
    pub input: InterruptCollaborationInput,
}

#[derive(Deserialize)]
pub struct SendInterruptParams {
    pub input: SendAndInterruptCollaborationInput,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedParams {
    pub conversation_id: i32,
    pub limit: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkSeenParams {
    pub conversation_id: i32,
    pub delivery_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DismissParams {
    pub conversation_id: i32,
    pub delivery_id: String,
}

pub async fn get_settings(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<SessionCollaborationSettings>, AppCommandError> {
    Ok(Json(
        collaboration::load_session_collaboration_settings(&state.db.conn).await,
    ))
}

#[derive(Deserialize)]
pub struct SetSettingsParams {
    pub settings: SessionCollaborationSettings,
}

pub async fn set_settings(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<SetSettingsParams>,
) -> Result<Json<SessionCollaborationSettings>, AppCommandError> {
    Ok(Json(
        collaboration::set_session_collaboration_settings_core(
            &state.db.conn,
            &state.session_collaboration_config,
            &state.emitter,
            &state.prompt_queue,
            params.settings,
        )
        .await?,
    ))
}

pub async fn send(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<SendParams>,
) -> Result<Json<CollaborationSendResult>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_send_core(
            &state.db.conn,
            &state.emitter,
            &state.prompt_queue,
            Some(&state.connection_manager),
            params.input,
        )
        .await?,
    ))
}

pub async fn interrupt(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<InterruptParams>,
) -> Result<Json<CollaborationInterruptResult>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_interrupt_core(
            &state.db.conn,
            &state.connection_manager,
            &state.emitter,
            &state.prompt_queue,
            params.input,
        )
        .await?,
    ))
}

pub async fn send_interrupt(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<SendInterruptParams>,
) -> Result<Json<SendAndInterruptCollaborationResult>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_send_interrupt_core(
            &state.db.conn,
            &state.connection_manager,
            &state.emitter,
            &state.prompt_queue,
            params.input,
        )
        .await?,
    ))
}

pub async fn feed(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<FeedParams>,
) -> Result<Json<CollaborationFeed>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_feed_core(
            &state.db.conn,
            params.conversation_id,
            params.limit,
        )
        .await?,
    ))
}

pub async fn timeline_projection(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<FeedParams>,
) -> Result<Json<CollaborationTimelineProjection>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_timeline_projection_core(
            &state.db.conn,
            params.conversation_id,
        )
        .await?,
    ))
}

pub async fn unread_overview(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<CollaborationUnreadOverview>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_unread_overview_core(&state.db.conn).await?,
    ))
}

pub async fn mark_seen(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<MarkSeenParams>,
) -> Result<Json<CollaborationFeed>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_mark_seen_core(
            &state.db.conn,
            &state.emitter,
            params.conversation_id,
            params.delivery_ids,
        )
        .await?,
    ))
}

pub async fn resolve(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<DismissParams>,
) -> Result<Json<CollaborationFeed>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_resolve_core(
            &state.db.conn,
            &state.emitter,
            params.conversation_id,
            params.delivery_id,
        )
        .await?,
    ))
}

pub async fn dismiss(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<DismissParams>,
) -> Result<Json<CollaborationFeed>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_dismiss_core(
            &state.db.conn,
            &state.emitter,
            params.conversation_id,
            params.delivery_id,
        )
        .await?,
    ))
}

pub async fn restore(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<DismissParams>,
) -> Result<Json<CollaborationFeed>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_restore_core(
            &state.db.conn,
            &state.emitter,
            params.conversation_id,
            params.delivery_id,
        )
        .await?,
    ))
}

#[derive(Deserialize)]
pub struct CreateRoomParams {
    pub input: CreateCollaborationRoomInput,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomListParams {
    pub workbench_id: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomIdParams {
    pub room_id: String,
}

#[derive(Deserialize)]
pub struct AddRoomMembersParams {
    pub input: AddCollaborationRoomMembersInput,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveRoomMemberParams {
    pub room_id: String,
    pub conversation_id: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameRoomParams {
    pub room_id: String,
    pub title: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignRoomCollectionParams {
    pub room_ids: Vec<String>,
    pub collection_id: Option<i32>,
    pub root_folder_id: Option<i32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkRoomSeenParams {
    pub room_id: String,
    pub conversation_id: Option<i32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomTimelineParams {
    pub room_id: String,
    pub limit: Option<u32>,
    pub before_event_id: Option<String>,
}

#[derive(Deserialize)]
pub struct PostRoomParams {
    pub input: PostRoomMessageInput,
}

pub async fn room_create(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<CreateRoomParams>,
) -> Result<Json<CollaborationRoomDetail>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_room_create_core(&state.db.conn, &state.emitter, params.input)
            .await?,
    ))
}

pub async fn room_list(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<RoomListParams>,
) -> Result<Json<Vec<CollaborationRoomSummary>>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_room_list_core(&state.db.conn, params.workbench_id).await?,
    ))
}

pub async fn room_get(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<RoomIdParams>,
) -> Result<Json<CollaborationRoomDetail>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_room_get_core(&state.db.conn, &params.room_id).await?,
    ))
}

pub async fn room_add_members(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<AddRoomMembersParams>,
) -> Result<Json<CollaborationRoomDetail>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_room_add_members_core(
            &state.db.conn,
            &state.emitter,
            params.input,
        )
        .await?,
    ))
}

pub async fn room_remove_member(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<RemoveRoomMemberParams>,
) -> Result<Json<CollaborationRoomDetail>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_room_remove_member_core(
            &state.db.conn,
            &state.emitter,
            &params.room_id,
            params.conversation_id,
        )
        .await?,
    ))
}

pub async fn room_rename(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<RenameRoomParams>,
) -> Result<Json<CollaborationRoomDetail>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_room_rename_core(
            &state.db.conn,
            &state.emitter,
            &params.room_id,
            &params.title,
        )
        .await?,
    ))
}

pub async fn room_delete(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<RoomIdParams>,
) -> Result<Json<()>, AppCommandError> {
    collaboration::collaboration_room_delete_core(&state.db.conn, &state.emitter, &params.room_id)
        .await?;
    Ok(Json(()))
}

pub async fn room_assign_collection(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<AssignRoomCollectionParams>,
) -> Result<Json<Vec<CollaborationRoomSummary>>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_room_assign_collection_core(
            &state.db.conn,
            &state.emitter,
            params.room_ids,
            params.collection_id,
            params.root_folder_id,
        )
        .await?,
    ))
}

pub async fn room_mark_seen(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<MarkRoomSeenParams>,
) -> Result<Json<CollaborationRoomDetail>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_room_mark_seen_core(
            &state.db.conn,
            &state.emitter,
            &params.room_id,
            params.conversation_id,
        )
        .await?,
    ))
}

pub async fn room_timeline(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<RoomTimelineParams>,
) -> Result<Json<RoomTimeline>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_room_timeline_core(
            &state.db.conn,
            &params.room_id,
            params.limit,
            params.before_event_id.as_deref(),
        )
        .await?,
    ))
}

pub async fn room_post(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<PostRoomParams>,
) -> Result<Json<RoomPostResult>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_room_post_core(
            &state.db.conn,
            &state.emitter,
            &state.prompt_queue,
            Some(&state.connection_manager),
            params.input,
        )
        .await?,
    ))
}
