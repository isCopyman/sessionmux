//! Session Timer CRUD commands (Session Triggers RFC P2). Firing is owned by
//! the engine (`crate::session_timer`); these only mutate rows, broadcast the
//! invalidation event, and wake the engine so a just-created due timer
//! doesn't wait for the next sweep.

use crate::app_error::AppCommandError;
use crate::db::service::session_timer_service;
use crate::models::session_timer::{
    CreateSessionTimerInput, SessionTimerInfo, UpdateSessionTimerInput,
};
use crate::session_timer::SessionTimerHandle;
use crate::web::event_bridge::{
    emit_event, EventEmitter, SessionTimerChanged, SESSION_TIMER_CHANGED_EVENT,
};

fn publish(emitter: &EventEmitter, conversation_ids: Vec<i32>) {
    emit_event(
        emitter,
        SESSION_TIMER_CHANGED_EVENT,
        SessionTimerChanged { conversation_ids },
    );
}

pub async fn session_timer_list_core(
    conn: &sea_orm::DatabaseConnection,
    conversation_id: i32,
) -> Result<Vec<SessionTimerInfo>, AppCommandError> {
    session_timer_service::list(conn, conversation_id)
        .await
        .map_err(AppCommandError::from)
}

pub async fn session_timer_create_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    runtime: &SessionTimerHandle,
    input: CreateSessionTimerInput,
) -> Result<SessionTimerInfo, AppCommandError> {
    let conversation_id = input.conversation_id;
    let timer = session_timer_service::create(conn, input)
        .await
        .map_err(AppCommandError::from)?;
    publish(emitter, vec![conversation_id]);
    runtime.wake(conversation_id);
    Ok(timer)
}

pub async fn session_timer_update_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    runtime: &SessionTimerHandle,
    conversation_id: i32,
    id: String,
    input: UpdateSessionTimerInput,
) -> Result<SessionTimerInfo, AppCommandError> {
    let existing = session_timer_service::find(conn, &id)
        .await
        .map_err(AppCommandError::from)?;
    if existing.conversation_id != conversation_id {
        return Err(AppCommandError::invalid_input(
            "Timer does not belong to this Session",
        ));
    }
    let timer = session_timer_service::update(conn, &id, input)
        .await
        .map_err(AppCommandError::from)?;
    publish(emitter, vec![conversation_id]);
    runtime.wake(conversation_id);
    Ok(timer)
}

pub async fn session_timer_delete_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    conversation_id: i32,
    id: String,
) -> Result<(), AppCommandError> {
    let existing = session_timer_service::find(conn, &id)
        .await
        .map_err(AppCommandError::from)?;
    if existing.conversation_id != conversation_id {
        return Err(AppCommandError::invalid_input(
            "Timer does not belong to this Session",
        ));
    }
    session_timer_service::delete(conn, &id)
        .await
        .map_err(AppCommandError::from)?;
    publish(emitter, vec![conversation_id]);
    Ok(())
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn session_timer_list(
    conversation_id: i32,
    db: tauri::State<'_, crate::db::AppDatabase>,
) -> Result<Vec<SessionTimerInfo>, AppCommandError> {
    session_timer_list_core(&db.conn, conversation_id).await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn session_timer_create(
    input: CreateSessionTimerInput,
    db: tauri::State<'_, crate::db::AppDatabase>,
    runtime: tauri::State<'_, SessionTimerHandle>,
    app: tauri::AppHandle,
) -> Result<SessionTimerInfo, AppCommandError> {
    session_timer_create_core(&db.conn, &EventEmitter::Tauri(app), &runtime, input).await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn session_timer_update(
    conversation_id: i32,
    id: String,
    input: UpdateSessionTimerInput,
    db: tauri::State<'_, crate::db::AppDatabase>,
    runtime: tauri::State<'_, SessionTimerHandle>,
    app: tauri::AppHandle,
) -> Result<SessionTimerInfo, AppCommandError> {
    session_timer_update_core(
        &db.conn,
        &EventEmitter::Tauri(app),
        &runtime,
        conversation_id,
        id,
        input,
    )
    .await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn session_timer_delete(
    conversation_id: i32,
    id: String,
    db: tauri::State<'_, crate::db::AppDatabase>,
    app: tauri::AppHandle,
) -> Result<(), AppCommandError> {
    session_timer_delete_core(&db.conn, &EventEmitter::Tauri(app), conversation_id, id).await
}
