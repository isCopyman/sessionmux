use crate::app_error::AppCommandError;
use crate::db::service::{collaboration_service, prompt_queue_service};
#[cfg(feature = "tauri-runtime")]
use crate::db::AppDatabase;
use crate::models::{
    CollaborationChanged, EnqueuePromptQueueItem, PromptQueueDraft, PromptQueueSnapshot,
};
use crate::prompt_queue::PromptQueueHandle;
use crate::web::event_bridge::{
    emit_event, EventEmitter, COLLABORATION_CHANGED_EVENT, PROMPT_QUEUE_CHANGED_EVENT,
};

fn publish(emitter: &EventEmitter, snapshot: &PromptQueueSnapshot) {
    emit_event(emitter, PROMPT_QUEUE_CHANGED_EVENT, snapshot);
}

pub async fn prompt_queue_get_core(
    conn: &sea_orm::DatabaseConnection,
    conversation_id: i32,
) -> Result<PromptQueueSnapshot, AppCommandError> {
    prompt_queue_service::snapshot(conn, conversation_id)
        .await
        .map_err(AppCommandError::from)
}

pub async fn prompt_queue_enqueue_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    runtime: &PromptQueueHandle,
    input: EnqueuePromptQueueItem,
) -> Result<PromptQueueSnapshot, AppCommandError> {
    let conversation_id = input.conversation_id;
    let snapshot = prompt_queue_service::enqueue(conn, input).await?;
    publish(emitter, &snapshot);
    runtime.wake(conversation_id);
    Ok(snapshot)
}

pub async fn prompt_queue_edit_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    conversation_id: i32,
    id: String,
    draft: PromptQueueDraft,
    expected_revision: i64,
) -> Result<PromptQueueSnapshot, AppCommandError> {
    let snapshot =
        prompt_queue_service::edit(conn, conversation_id, &id, draft, expected_revision).await?;
    publish(emitter, &snapshot);
    Ok(snapshot)
}

pub async fn prompt_queue_delete_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    conversation_id: i32,
    id: String,
    expected_revision: i64,
) -> Result<PromptQueueSnapshot, AppCommandError> {
    let snapshot =
        prompt_queue_service::delete(conn, conversation_id, &id, expected_revision).await?;
    publish(emitter, &snapshot);
    Ok(snapshot)
}

pub async fn prompt_queue_reorder_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    conversation_id: i32,
    ordered_ids: Vec<String>,
    expected_revision: i64,
) -> Result<PromptQueueSnapshot, AppCommandError> {
    let snapshot =
        prompt_queue_service::reorder(conn, conversation_id, ordered_ids, expected_revision)
            .await?;
    publish(emitter, &snapshot);
    Ok(snapshot)
}

pub async fn prompt_queue_resume_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    runtime: &PromptQueueHandle,
    conversation_id: i32,
    expected_revision: i64,
) -> Result<PromptQueueSnapshot, AppCommandError> {
    let snapshot =
        prompt_queue_service::resume_queue(conn, conversation_id, expected_revision).await?;
    publish(emitter, &snapshot);
    runtime.wake(conversation_id);
    Ok(snapshot)
}

pub async fn prompt_queue_pause_manual_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    conversation_id: i32,
    expected_revision: i64,
) -> Result<PromptQueueSnapshot, AppCommandError> {
    let snapshot = prompt_queue_service::pause_queue_for_manual_review(
        conn,
        conversation_id,
        expected_revision,
    )
    .await?;
    publish(emitter, &snapshot);
    Ok(snapshot)
}

pub async fn prompt_queue_release_one_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    runtime: &PromptQueueHandle,
    conversation_id: i32,
    id: String,
    expected_revision: i64,
) -> Result<PromptQueueSnapshot, AppCommandError> {
    let snapshot = prompt_queue_service::release_one_manual_item(
        conn,
        conversation_id,
        &id,
        expected_revision,
    )
    .await?;
    publish(emitter, &snapshot);
    runtime.wake(conversation_id);
    Ok(snapshot)
}

pub async fn prompt_queue_retry_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    runtime: &PromptQueueHandle,
    conversation_id: i32,
    id: String,
    expected_revision: i64,
) -> Result<PromptQueueSnapshot, AppCommandError> {
    let before = prompt_queue_service::snapshot(conn, conversation_id).await?;
    let retry_item = before
        .items
        .iter()
        .find(|item| item.id == id)
        .ok_or_else(|| AppCommandError::configuration_invalid("Queued prompt was not found"))?;
    let origin_event_id = retry_item.origin_event_id.clone();
    if retry_item.paused_reason.as_deref()
        == Some(collaboration_service::INACTIVE_TARGET_CONFIRMATION_REASON)
        && !runtime
            .is_session_runtime_active(conn, conversation_id)
            .await?
    {
        return Err(AppCommandError::configuration_invalid(
            "Open the target Session before starting this delivered message",
        ));
    }
    let snapshot =
        prompt_queue_service::retry_item(conn, conversation_id, &id, expected_revision).await?;
    publish(emitter, &snapshot);
    if let Some(event_id) = origin_event_id.as_deref() {
        let conversation_ids =
            collaboration_service::origin_participants(conn, conversation_id, event_id).await?;
        if !conversation_ids.is_empty() {
            emit_event(
                emitter,
                COLLABORATION_CHANGED_EVENT,
                CollaborationChanged { conversation_ids },
            );
        }
    }
    runtime.wake(conversation_id);
    Ok(snapshot)
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn prompt_queue_get(
    conversation_id: i32,
    db: tauri::State<'_, AppDatabase>,
) -> Result<PromptQueueSnapshot, AppCommandError> {
    prompt_queue_get_core(&db.conn, conversation_id).await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn prompt_queue_enqueue(
    input: EnqueuePromptQueueItem,
    db: tauri::State<'_, AppDatabase>,
    runtime: tauri::State<'_, PromptQueueHandle>,
    app: tauri::AppHandle,
) -> Result<PromptQueueSnapshot, AppCommandError> {
    prompt_queue_enqueue_core(&db.conn, &EventEmitter::Tauri(app), &runtime, input).await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn prompt_queue_edit(
    conversation_id: i32,
    id: String,
    draft: PromptQueueDraft,
    expected_revision: i64,
    db: tauri::State<'_, AppDatabase>,
    app: tauri::AppHandle,
) -> Result<PromptQueueSnapshot, AppCommandError> {
    prompt_queue_edit_core(
        &db.conn,
        &EventEmitter::Tauri(app),
        conversation_id,
        id,
        draft,
        expected_revision,
    )
    .await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn prompt_queue_delete(
    conversation_id: i32,
    id: String,
    expected_revision: i64,
    db: tauri::State<'_, AppDatabase>,
    app: tauri::AppHandle,
) -> Result<PromptQueueSnapshot, AppCommandError> {
    prompt_queue_delete_core(
        &db.conn,
        &EventEmitter::Tauri(app),
        conversation_id,
        id,
        expected_revision,
    )
    .await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn prompt_queue_reorder(
    conversation_id: i32,
    ordered_ids: Vec<String>,
    expected_revision: i64,
    db: tauri::State<'_, AppDatabase>,
    app: tauri::AppHandle,
) -> Result<PromptQueueSnapshot, AppCommandError> {
    prompt_queue_reorder_core(
        &db.conn,
        &EventEmitter::Tauri(app),
        conversation_id,
        ordered_ids,
        expected_revision,
    )
    .await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn prompt_queue_resume(
    conversation_id: i32,
    expected_revision: i64,
    db: tauri::State<'_, AppDatabase>,
    runtime: tauri::State<'_, PromptQueueHandle>,
    app: tauri::AppHandle,
) -> Result<PromptQueueSnapshot, AppCommandError> {
    prompt_queue_resume_core(
        &db.conn,
        &EventEmitter::Tauri(app),
        &runtime,
        conversation_id,
        expected_revision,
    )
    .await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn prompt_queue_pause_manual(
    conversation_id: i32,
    expected_revision: i64,
    db: tauri::State<'_, AppDatabase>,
    app: tauri::AppHandle,
) -> Result<PromptQueueSnapshot, AppCommandError> {
    prompt_queue_pause_manual_core(
        &db.conn,
        &EventEmitter::Tauri(app),
        conversation_id,
        expected_revision,
    )
    .await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn prompt_queue_release_one(
    conversation_id: i32,
    id: String,
    expected_revision: i64,
    db: tauri::State<'_, AppDatabase>,
    runtime: tauri::State<'_, PromptQueueHandle>,
    app: tauri::AppHandle,
) -> Result<PromptQueueSnapshot, AppCommandError> {
    prompt_queue_release_one_core(
        &db.conn,
        &EventEmitter::Tauri(app),
        &runtime,
        conversation_id,
        id,
        expected_revision,
    )
    .await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn prompt_queue_retry(
    conversation_id: i32,
    id: String,
    expected_revision: i64,
    db: tauri::State<'_, AppDatabase>,
    runtime: tauri::State<'_, PromptQueueHandle>,
    app: tauri::AppHandle,
) -> Result<PromptQueueSnapshot, AppCommandError> {
    prompt_queue_retry_core(
        &db.conn,
        &EventEmitter::Tauri(app),
        &runtime,
        conversation_id,
        id,
        expected_revision,
    )
    .await
}
