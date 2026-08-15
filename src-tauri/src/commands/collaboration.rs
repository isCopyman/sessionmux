use crate::app_error::AppCommandError;
use crate::db::service::collaboration_service;
#[cfg(feature = "tauri-runtime")]
use crate::db::AppDatabase;
use crate::models::{
    CollaborationChanged, CollaborationFeed, CollaborationSendResult, SendCollaborationMessageInput,
};
use crate::web::event_bridge::{emit_event, EventEmitter, COLLABORATION_CHANGED_EVENT};

fn publish(emitter: &EventEmitter, conversation_ids: Vec<i32>) {
    if conversation_ids.is_empty() {
        return;
    }
    emit_event(
        emitter,
        COLLABORATION_CHANGED_EVENT,
        CollaborationChanged { conversation_ids },
    );
}

pub async fn collaboration_send_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    input: SendCollaborationMessageInput,
) -> Result<CollaborationSendResult, AppCommandError> {
    let result = collaboration_service::send(conn, input).await?;
    if !result.deduplicated {
        publish(emitter, result.affected_conversation_ids.clone());
    }
    Ok(result)
}

pub async fn collaboration_feed_core(
    conn: &sea_orm::DatabaseConnection,
    conversation_id: i32,
    limit: Option<u32>,
) -> Result<CollaborationFeed, AppCommandError> {
    collaboration_service::feed(conn, conversation_id, limit)
        .await
        .map_err(AppCommandError::from)
}

pub async fn collaboration_mark_seen_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    conversation_id: i32,
    delivery_ids: Vec<String>,
) -> Result<CollaborationFeed, AppCommandError> {
    let result = collaboration_service::mark_seen(conn, conversation_id, delivery_ids).await?;
    publish(emitter, result.affected_conversation_ids);
    Ok(result.feed)
}

pub async fn collaboration_dismiss_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    conversation_id: i32,
    delivery_id: String,
) -> Result<CollaborationFeed, AppCommandError> {
    let result = collaboration_service::dismiss(conn, conversation_id, &delivery_id).await?;
    publish(emitter, result.affected_conversation_ids);
    Ok(result.feed)
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn collaboration_send(
    input: SendCollaborationMessageInput,
    db: tauri::State<'_, AppDatabase>,
    app: tauri::AppHandle,
) -> Result<CollaborationSendResult, AppCommandError> {
    collaboration_send_core(&db.conn, &EventEmitter::Tauri(app), input).await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn collaboration_feed(
    conversation_id: i32,
    limit: Option<u32>,
    db: tauri::State<'_, AppDatabase>,
) -> Result<CollaborationFeed, AppCommandError> {
    collaboration_feed_core(&db.conn, conversation_id, limit).await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn collaboration_mark_seen(
    conversation_id: i32,
    delivery_ids: Vec<String>,
    db: tauri::State<'_, AppDatabase>,
    app: tauri::AppHandle,
) -> Result<CollaborationFeed, AppCommandError> {
    collaboration_mark_seen_core(
        &db.conn,
        &EventEmitter::Tauri(app),
        conversation_id,
        delivery_ids,
    )
    .await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn collaboration_dismiss(
    conversation_id: i32,
    delivery_id: String,
    db: tauri::State<'_, AppDatabase>,
    app: tauri::AppHandle,
) -> Result<CollaborationFeed, AppCommandError> {
    collaboration_dismiss_core(
        &db.conn,
        &EventEmitter::Tauri(app),
        conversation_id,
        delivery_id,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::{
        AgentType, CollaborationDeliveryHint, CollaborationInvocationPolicy, CollaborationUrgency,
    };
    use crate::web::event_bridge::WebEventBroadcaster;
    use std::sync::Arc;

    #[tokio::test]
    async fn send_broadcasts_one_targeted_invalidation_after_commit() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-collaboration-command").await;
        let source = seed_conversation(&db, folder, AgentType::Codex).await;
        let target = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        let broadcaster = Arc::new(WebEventBroadcaster::new());
        let mut receiver = broadcaster.subscribe();
        let emitter = EventEmitter::test_web_only(broadcaster);

        let result = collaboration_send_core(
            &db.conn,
            &emitter,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                body: "review".to_string(),
                client_dedupe_id: "command-send".to_string(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .expect("send");
        assert!(!result.deduplicated);

        let event = receiver.recv().await.expect("broadcast");
        assert_eq!(event.channel, COLLABORATION_CHANGED_EVENT);
        let changed: CollaborationChanged =
            serde_json::from_value(event.payload.as_ref().clone()).expect("payload");
        assert_eq!(changed.conversation_ids, vec![source, target]);

        // The idempotent replay is read-only and must not produce a second
        // invalidation that could make every open WebView refetch needlessly.
        let replay = collaboration_send_core(
            &db.conn,
            &emitter,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                body: "review".to_string(),
                client_dedupe_id: "command-send".to_string(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .expect("dedupe");
        assert!(replay.deduplicated);
        assert!(receiver.try_recv().is_err());
    }
}
