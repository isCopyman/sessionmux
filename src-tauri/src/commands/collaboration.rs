use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use sea_orm::EntityTrait;
use serde::{Deserialize, Serialize};

use crate::acp::manager::ConnectionManager;
use crate::acp::session_collaboration::{
    SessionAddress, SessionCollaborationAccess, SessionCollaborationConfig,
    SessionCollaborationRuntimeConfig, SessionListOutcome, SessionMessageDeliveryOutcome,
    SessionMessageDeliveryMode, SessionMessageSpec, SessionSendOutcome, MAX_SESSION_LIST_LIMIT,
};
use crate::app_error::AppCommandError;
use crate::db::service::{
    app_metadata_service, collaboration_interrupt_service, collaboration_service,
    conversation_service, folder_service, prompt_queue_service,
};
use crate::db::AppDatabase;
use crate::models::{
    CollaborationChanged, CollaborationDeliveryHint, CollaborationDeliveryState, CollaborationFeed,
    CollaborationInterruptResult, CollaborationInterruptState, CollaborationInvocationPolicy,
    CollaborationSendResult, CollaborationTimelineProjection, CollaborationUnreadOverview,
    CollaborationUrgency, InterruptCollaborationInput, SendAndInterruptCollaborationInput,
    SendAndInterruptCollaborationResult, SendCollaborationMessageInput,
};
use crate::prompt_queue::PromptQueueHandle;
use crate::web::event_bridge::{
    emit_event, EventEmitter, COLLABORATION_CHANGED_EVENT, PROMPT_QUEUE_CHANGED_EVENT,
    SESSION_COLLABORATION_SETTINGS_CHANGED_EVENT,
};

pub const KEY_SESSION_COLLABORATION_ENABLED: &str =
    prompt_queue_service::SESSION_COLLABORATION_ENABLED_KEY;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionCollaborationSettings {
    pub enabled: bool,
}

impl Default for SessionCollaborationSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl SessionCollaborationSettings {
    fn into_runtime_config(self) -> SessionCollaborationConfig {
        SessionCollaborationConfig {
            enabled: self.enabled,
        }
    }
}

/// Production Host Core for the managed-agent collaboration tools. The caller
/// id is supplied by the listener after token resolution; this type never
/// accepts a source id from MCP arguments.
pub struct DbSessionCollaboration {
    db: Arc<AppDatabase>,
    emitter: EventEmitter,
    prompt_queue: PromptQueueHandle,
    config: SessionCollaborationRuntimeConfig,
}

impl DbSessionCollaboration {
    pub fn new(
        db: Arc<AppDatabase>,
        emitter: EventEmitter,
        prompt_queue: PromptQueueHandle,
        config: SessionCollaborationRuntimeConfig,
    ) -> Self {
        Self {
            db,
            emitter,
            prompt_queue,
            config,
        }
    }
}

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

async fn persist_collaboration_message(
    conn: &sea_orm::DatabaseConnection,
    _prompt_queue: &PromptQueueHandle,
    input: SendCollaborationMessageInput,
) -> Result<CollaborationSendResult, AppCommandError> {
    // Do not pause closed targets for human confirmation. The durable queue
    // waits for a live connection, then steers or starts a turn the same way
    // a human follow-up would.
    collaboration_service::send_with_initially_inactive_targets(
        conn,
        input,
        &std::collections::HashSet::new(),
    )
    .await
    .map_err(AppCommandError::from)
}

fn publish_persisted_message(emitter: &EventEmitter, result: &CollaborationSendResult) {
    if !result.deduplicated {
        publish(emitter, result.affected_conversation_ids.clone());
    }
}

pub async fn collaboration_send_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    prompt_queue: &PromptQueueHandle,
    input: SendCollaborationMessageInput,
) -> Result<CollaborationSendResult, AppCommandError> {
    // Persist first. Only invoke_when_idle enters the Session dispatcher;
    // store_only remains visible in the mailbox until a later natural turn or
    // explicit Agent read. Closed Sessions are never cold-started here.
    let should_wake = input.invocation_policy == CollaborationInvocationPolicy::InvokeWhenIdle;
    let result = persist_collaboration_message(conn, prompt_queue, input).await?;
    publish_persisted_message(emitter, &result);
    if should_wake && !result.deduplicated {
        for delivery in result.deliveries.iter().filter(|delivery| {
            delivery.state != CollaborationDeliveryState::Failed
                && delivery.state != CollaborationDeliveryState::Dismissed
        }) {
            prompt_queue.wake(delivery.target.conversation_id);
        }
    }
    Ok(result)
}

pub async fn collaboration_send_interrupt_core(
    conn: &sea_orm::DatabaseConnection,
    manager: &ConnectionManager,
    emitter: &EventEmitter,
    prompt_queue: &PromptQueueHandle,
    mut input: SendAndInterruptCollaborationInput,
) -> Result<SendAndInterruptCollaborationResult, AppCommandError> {
    if !prompt_queue_service::collaboration_dispatch_enabled(conn).await? {
        return Err(crate::db::error::DbError::Validation(
            "Session collaboration is disabled in Codeg settings".to_string(),
        )
        .into());
    }
    if input.message.target_conversation_ids.len() != 1 {
        return Err(crate::db::error::DbError::Validation(
            "Stop and send requires exactly one target Session".to_string(),
        )
        .into());
    }
    input.message.invocation_policy = CollaborationInvocationPolicy::InvokeWhenIdle;
    input.message.delivery_hint = CollaborationDeliveryHint::Default;
    let target_conversation_id = input.message.target_conversation_ids[0];

    // Do not wake the queue between persistence and interrupt preparation. If
    // the target was idle, a two-request renderer flow could otherwise start
    // this new message and then accidentally cancel that very turn.
    let message = persist_collaboration_message(conn, prompt_queue, input.message).await?;
    publish_persisted_message(emitter, &message);
    emit_event(
        emitter,
        PROMPT_QUEUE_CHANGED_EVENT,
        prompt_queue_service::snapshot(conn, target_conversation_id).await?,
    );
    let interrupt = collaboration_interrupt_core(
        conn,
        manager,
        emitter,
        prompt_queue,
        InterruptCollaborationInput {
            event_id: message.event_id.clone(),
            target_conversation_id,
            client_dedupe_id: input.interrupt_client_dedupe_id,
            reason: input.reason,
        },
    )
    .await;
    match interrupt {
        Ok(interrupt) => Ok(SendAndInterruptCollaborationResult {
            message,
            interrupt: Some(interrupt),
            interrupt_error: None,
        }),
        Err(err) => Ok(SendAndInterruptCollaborationResult {
            message,
            interrupt: None,
            interrupt_error: Some(err.to_string()),
        }),
    }
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

pub async fn collaboration_timeline_projection_core(
    conn: &sea_orm::DatabaseConnection,
    conversation_id: i32,
) -> Result<CollaborationTimelineProjection, AppCommandError> {
    collaboration_service::timeline_projection(conn, conversation_id)
        .await
        .map_err(AppCommandError::from)
}

pub async fn collaboration_unread_overview_core(
    conn: &sea_orm::DatabaseConnection,
) -> Result<CollaborationUnreadOverview, AppCommandError> {
    collaboration_service::unread_overview(conn)
        .await
        .map_err(AppCommandError::from)
}

pub async fn collaboration_interrupt_core(
    conn: &sea_orm::DatabaseConnection,
    manager: &ConnectionManager,
    emitter: &EventEmitter,
    prompt_queue: &PromptQueueHandle,
    input: InterruptCollaborationInput,
) -> Result<CollaborationInterruptResult, AppCommandError> {
    if !prompt_queue_service::collaboration_dispatch_enabled(conn).await? {
        return Err(crate::db::error::DbError::Validation(
            "Session collaboration is disabled in Codeg settings".to_string(),
        )
        .into());
    }
    let target_conversation_id = input.target_conversation_id;
    let row = crate::db::entities::conversation::Entity::find_by_id(target_conversation_id)
        .one(conn)
        .await
        .map_err(crate::db::error::DbError::from)?
        .ok_or_else(|| {
            crate::db::error::DbError::NotFound(format!("Conversation {target_conversation_id}"))
        })?;
    if row.deleted_at.is_some() {
        return Err(crate::db::error::DbError::NotFound(format!(
            "Conversation {target_conversation_id}"
        ))
        .into());
    }
    let Some((connection_id, state)) =
        crate::prompt_queue::active_connection_for_row(manager, &row).await
    else {
        return Err(crate::db::error::DbError::Validation(
            "Target Session is not active; the durable message remains queued for confirmation"
                .to_string(),
        )
        .into());
    };
    let (connected, turn_active) = {
        let state = state.read().await;
        (
            state.status == crate::acp::types::ConnectionStatus::Connected,
            state.turn_in_flight,
        )
    };
    if !connected {
        return Err(crate::db::error::DbError::Validation(
            "Target Session is not connected; the durable message remains queued".to_string(),
        )
        .into());
    }

    let prepared =
        collaboration_interrupt_service::prepare(conn, input, &connection_id, turn_active).await?;
    let event_id = prepared.operation.event_id.clone();
    let mut operation = prepared.operation;
    emit_event(
        emitter,
        PROMPT_QUEUE_CHANGED_EVENT,
        prompt_queue_service::snapshot(conn, target_conversation_id).await?,
    );
    publish(
        emitter,
        collaboration_service::origin_participants(conn, target_conversation_id, &event_id).await?,
    );

    if prepared.should_cancel {
        let still_running = match manager.get_state(&connection_id).await {
            Some(state) => state.read().await.turn_in_flight,
            None => false,
        };
        operation = if !still_running {
            // The old turn ended between the initial runtime snapshot and the
            // cancellation boundary. Record that terminal observation before
            // releasing the owned queue pause; never send Cancel to an idle
            // runtime where it could affect a later prompt.
            collaboration_interrupt_service::observe_terminal(conn, target_conversation_id).await?;
            collaboration_interrupt_service::mark_cancel_enqueued(conn, &operation.id).await?
        } else {
            match manager.cancel(conn, &connection_id).await {
                Ok(()) => {
                    collaboration_interrupt_service::mark_cancel_enqueued(conn, &operation.id)
                        .await?
                }
                Err(err) => {
                    collaboration_interrupt_service::mark_cancel_failed(
                        conn,
                        &operation.id,
                        &err.to_string(),
                    )
                    .await?
                }
            }
        };
        emit_event(
            emitter,
            PROMPT_QUEUE_CHANGED_EVENT,
            prompt_queue_service::snapshot(conn, target_conversation_id).await?,
        );
        publish(
            emitter,
            collaboration_service::origin_participants(conn, target_conversation_id, &event_id)
                .await?,
        );
    }
    if matches!(
        operation.state,
        CollaborationInterruptState::Ready | CollaborationInterruptState::Dispatching
    ) {
        prompt_queue.wake(target_conversation_id);
    }
    Ok(CollaborationInterruptResult {
        operation,
        deduplicated: prepared.deduplicated,
    })
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

pub async fn collaboration_resolve_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    conversation_id: i32,
    delivery_id: String,
) -> Result<CollaborationFeed, AppCommandError> {
    let result =
        collaboration_service::resolve_obligation(conn, conversation_id, &delivery_id).await?;
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

pub async fn collaboration_restore_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    conversation_id: i32,
    delivery_id: String,
) -> Result<CollaborationFeed, AppCommandError> {
    let result = collaboration_service::restore(conn, conversation_id, &delivery_id).await?;
    publish(emitter, result.affected_conversation_ids);
    Ok(result.feed)
}

fn delivery_state_name(state: CollaborationDeliveryState) -> String {
    match state {
        CollaborationDeliveryState::Pending => "pending",
        CollaborationDeliveryState::Queued => "queued",
        CollaborationDeliveryState::Embedding => "embedding",
        CollaborationDeliveryState::Embedded => "embedded",
        CollaborationDeliveryState::Dismissed => "dismissed",
        CollaborationDeliveryState::Failed => "failed",
    }
    .to_string()
}

#[async_trait]
impl SessionCollaborationAccess for DbSessionCollaboration {
    async fn list_sessions(
        &self,
        caller_session_id: i32,
        query: Option<String>,
        limit: u32,
    ) -> SessionListOutcome {
        if !self.config.is_enabled().await {
            return SessionListOutcome::unavailable(
                Some(caller_session_id),
                "Session collaboration is disabled in Codeg settings.",
            );
        }

        let mut rows = match conversation_service::list_all(
            &self.db.conn,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .await
        {
            Ok(rows) => rows,
            Err(err) => {
                return SessionListOutcome::unavailable(
                    Some(caller_session_id),
                    format!("Could not list Sessions: {err}"),
                )
            }
        };
        let archived = match conversation_service::list_all(
            &self.db.conn,
            None,
            None,
            None,
            None,
            None,
            true,
            false,
        )
        .await
        {
            Ok(rows) => rows,
            Err(err) => {
                return SessionListOutcome::unavailable(
                    Some(caller_session_id),
                    format!("Could not list archived Sessions: {err}"),
                )
            }
        };
        rows.extend(archived);

        // A draft without an underlying Harness session is not a stable
        // address. The caller itself is omitted because self-delivery is
        // rejected by the collaboration core.
        let caller_exists = rows.iter().any(|row| row.id == caller_session_id);
        if !caller_exists {
            return SessionListOutcome::unavailable(
                Some(caller_session_id),
                "The calling Session is no longer available in Codeg.",
            );
        }

        let folders = match folder_service::list_all_folder_details(&self.db.conn).await {
            Ok(folders) => folders,
            Err(err) => {
                return SessionListOutcome::unavailable(
                    Some(caller_session_id),
                    format!("Could not resolve Session workspaces: {err}"),
                )
            }
        };
        let folders: HashMap<i32, (String, String)> = folders
            .into_iter()
            .map(|folder| (folder.id, (folder.name, folder.path)))
            .collect();
        let query = query
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty());
        let limit = limit.clamp(1, MAX_SESSION_LIST_LIMIT) as usize;
        let mut sessions = rows
            .into_iter()
            .filter(|row| row.id != caller_session_id && row.external_id.is_some())
            .filter_map(|row| {
                let agent_type = row.agent_type.as_wire().into_owned();
                let (workspace_name, workspace_path) = folders
                    .get(&row.folder_id)
                    .map(|(name, path)| (Some(name.clone()), Some(path.clone())))
                    .unwrap_or((None, None));
                if let Some(query) = query.as_deref() {
                    let searchable = format!(
                        "{} {} {} {} {}",
                        row.id,
                        row.title.as_deref().unwrap_or_default(),
                        agent_type,
                        workspace_name.as_deref().unwrap_or_default(),
                        workspace_path.as_deref().unwrap_or_default(),
                    )
                    .to_lowercase();
                    if !searchable.contains(query) {
                        return None;
                    }
                }
                Some(SessionAddress {
                    session_id: row.id,
                    title: row.title,
                    agent_type,
                    status: row.status,
                    model: row.model,
                    workspace_name,
                    workspace_path,
                    updated_at: row.updated_at,
                    archived: row.archived_at.is_some(),
                })
            })
            .collect::<Vec<_>>();
        sessions.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| b.session_id.cmp(&a.session_id))
        });
        let truncated = sessions.len() > limit;
        sessions.truncate(limit);
        SessionListOutcome {
            available: true,
            caller_session_id: Some(caller_session_id),
            sessions,
            truncated,
            note: None,
        }
    }

    async fn send_message(
        &self,
        source_session_id: i32,
        spec: SessionMessageSpec,
    ) -> SessionSendOutcome {
        // Host Core re-check: tools/list filtering in the companion is UX, not
        // a security boundary. An old or replaced companion cannot write after
        // the operator disables collaboration.
        if !self.config.is_enabled().await {
            return SessionSendOutcome::rejected(
                Some(source_session_id),
                "Session collaboration is disabled in Codeg settings.",
            );
        }
        let result = collaboration_send_core(
            &self.db.conn,
            &self.emitter,
            &self.prompt_queue,
            SendCollaborationMessageInput {
                source_conversation_id: source_session_id,
                target_conversation_ids: spec.target_session_ids,
                body: spec.content,
                client_dedupe_id: spec.client_dedupe_id,
                invocation_policy: match spec.delivery_mode {
                    SessionMessageDeliveryMode::DeliverOnly => {
                        CollaborationInvocationPolicy::StoreOnly
                    }
                    SessionMessageDeliveryMode::Queue => {
                        CollaborationInvocationPolicy::InvokeWhenIdle
                    }
                },
                delivery_hint: if spec.steer_if_supported {
                    CollaborationDeliveryHint::SteerIfSupported
                } else {
                    CollaborationDeliveryHint::Default
                },
                expects_reply: spec.expects_reply,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: spec.reply_to_event_id,
            },
        )
        .await;
        match result {
            Ok(result) => SessionSendOutcome {
                accepted: true,
                source_session_id: Some(source_session_id),
                event_id: Some(result.event_id),
                deliveries: result
                    .deliveries
                    .into_iter()
                    .map(|delivery| SessionMessageDeliveryOutcome {
                        target_session_id: delivery.target.conversation_id,
                        target_title: delivery.target.title,
                        target_agent_type: delivery.target.agent_type,
                        state: delivery_state_name(delivery.state),
                        error: delivery.error,
                    })
                    .collect(),
                deduplicated: result.deduplicated,
                note: None,
            },
            Err(err) => SessionSendOutcome::rejected(Some(source_session_id), err.to_string()),
        }
    }

    async fn list_inbox(
        &self,
        caller_session_id: i32,
        filter: crate::acp::session_collaboration::SessionInboxFilter,
        limit: u32,
    ) -> crate::acp::session_collaboration::SessionInboxOutcome {
        use crate::acp::session_collaboration::{
            inbox_preview, SessionInboxItem, SessionInboxOutcome, MAX_INBOX_LIMIT,
        };
        if !self.config.is_enabled().await {
            return SessionInboxOutcome::unavailable(
                Some(caller_session_id),
                "Session collaboration is disabled in Codeg settings.",
            );
        }
        let items = match collaboration_service::list_inbox(
            &self.db.conn,
            caller_session_id,
            filter,
            limit.clamp(1, MAX_INBOX_LIMIT),
        )
        .await
        {
            Ok(items) => items,
            Err(err) => {
                return SessionInboxOutcome::unavailable(Some(caller_session_id), err.to_string())
            }
        };
        let feed = match collaboration_service::feed(&self.db.conn, caller_session_id, None).await {
            Ok(feed) => feed,
            Err(err) => {
                return SessionInboxOutcome::unavailable(Some(caller_session_id), err.to_string())
            }
        };
        let awaiting_reply_count = feed
            .inbound
            .iter()
            .filter(|item| {
                item.obligation_state == crate::models::CollaborationObligationState::AwaitingReply
            })
            .count() as u32;
        SessionInboxOutcome {
            available: true,
            caller_session_id: Some(caller_session_id),
            unread_count: feed.unread_count,
            awaiting_reply_count,
            truncated: items.len() as u32 >= limit.clamp(1, MAX_INBOX_LIMIT),
            items: items
                .into_iter()
                .map(|item| SessionInboxItem {
                    event_id: item.event_id,
                    delivery_id: item.id,
                    from_session_id: item.source.conversation_id,
                    from_title: item.source.title,
                    from_agent_type: item.source.agent_type,
                    preview: inbox_preview(&item.body),
                    unread: item.agent_received_at.is_none(),
                    expects_reply: item.expects_reply,
                    obligation_state: item.obligation_state.as_str().to_string(),
                    created_at: item.created_at,
                })
                .collect(),
            note: None,
        }
    }

    async fn read_message(
        &self,
        caller_session_id: i32,
        event_id: String,
    ) -> crate::acp::session_collaboration::SessionMessageReadOutcome {
        use crate::acp::session_collaboration::SessionMessageReadOutcome;
        if !self.config.is_enabled().await {
            return SessionMessageReadOutcome::unavailable(
                Some(caller_session_id),
                "Session collaboration is disabled in Codeg settings.",
            );
        }
        let item = match collaboration_service::get_inbound_message(
            &self.db.conn,
            caller_session_id,
            &event_id,
        )
        .await
        {
            Ok(item) => item,
            Err(err) => {
                return SessionMessageReadOutcome::unavailable(
                    Some(caller_session_id),
                    err.to_string(),
                )
            }
        };
        let marked = if let Ok(changed) =
            collaboration_service::mark_agent_read(&self.db.conn, caller_session_id, &event_id)
                .await
        {
            publish(&self.emitter, changed.affected_conversation_ids);
            true
        } else {
            false
        };
        SessionMessageReadOutcome {
            available: true,
            caller_session_id: Some(caller_session_id),
            event_id: Some(item.event_id),
            delivery_id: Some(item.id),
            from_session_id: Some(item.source.conversation_id),
            from_title: item.source.title,
            from_agent_type: item.source.agent_type,
            body: Some(item.body),
            expects_reply: item.expects_reply,
            reply_to_event_id: item.reply_to_event_id,
            unread: if marked {
                false
            } else {
                item.agent_received_at.is_none()
            },
            obligation_state: Some(item.obligation_state.as_str().to_string()),
            created_at: Some(item.created_at),
            note: None,
        }
    }
}

pub async fn load_session_collaboration_settings(
    conn: &sea_orm::DatabaseConnection,
) -> SessionCollaborationSettings {
    let mut settings = SessionCollaborationSettings::default();
    if let Ok(Some(raw)) =
        app_metadata_service::get_value(conn, KEY_SESSION_COLLABORATION_ENABLED).await
    {
        if let Ok(value) = raw.parse::<bool>() {
            settings.enabled = value;
        }
    }
    settings
}

pub async fn apply_persisted_session_collaboration_config(
    conn: &sea_orm::DatabaseConnection,
    config: &SessionCollaborationRuntimeConfig,
) {
    let settings = load_session_collaboration_settings(conn).await;
    config.set(settings.into_runtime_config()).await;
}

pub async fn set_session_collaboration_settings_core(
    conn: &sea_orm::DatabaseConnection,
    config: &SessionCollaborationRuntimeConfig,
    emitter: &EventEmitter,
    prompt_queue: &PromptQueueHandle,
    desired: SessionCollaborationSettings,
) -> Result<SessionCollaborationSettings, AppCommandError> {
    app_metadata_service::upsert_value(
        conn,
        KEY_SESSION_COLLABORATION_ENABLED,
        &desired.enabled.to_string(),
    )
    .await?;
    config.set(desired.clone().into_runtime_config()).await;
    let changes =
        prompt_queue_service::reconcile_collaboration_dispatch_policy(conn, desired.enabled)
            .await?;
    let mut affected_conversation_ids = std::collections::BTreeSet::new();
    for change in changes {
        let target_conversation_id = change.snapshot.conversation_id;
        emit_event(emitter, PROMPT_QUEUE_CHANGED_EVENT, change.snapshot);
        for event_id in change.origin_event_ids {
            affected_conversation_ids.extend(
                collaboration_service::origin_participants(conn, target_conversation_id, &event_id)
                    .await?,
            );
        }
        if desired.enabled {
            prompt_queue.wake(target_conversation_id);
        }
    }
    publish(emitter, affected_conversation_ids.into_iter().collect());
    emit_event(
        emitter,
        SESSION_COLLABORATION_SETTINGS_CHANGED_EVENT,
        &desired,
    );
    Ok(desired)
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn get_session_collaboration_settings(
    #[cfg(feature = "tauri-runtime")] db: tauri::State<'_, AppDatabase>,
) -> Result<SessionCollaborationSettings, AppCommandError> {
    #[cfg(feature = "tauri-runtime")]
    {
        Ok(load_session_collaboration_settings(&db.conn).await)
    }
    #[cfg(not(feature = "tauri-runtime"))]
    {
        Err(AppCommandError::configuration_invalid("tauri-only command"))
    }
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn set_session_collaboration_settings(
    #[cfg(feature = "tauri-runtime")] app: tauri::AppHandle,
    #[cfg(feature = "tauri-runtime")] db: tauri::State<'_, AppDatabase>,
    #[cfg(feature = "tauri-runtime")] config: tauri::State<'_, SessionCollaborationRuntimeConfig>,
    #[cfg(feature = "tauri-runtime")] prompt_queue: tauri::State<'_, PromptQueueHandle>,
    settings: SessionCollaborationSettings,
) -> Result<SessionCollaborationSettings, AppCommandError> {
    #[cfg(feature = "tauri-runtime")]
    {
        set_session_collaboration_settings_core(
            &db.conn,
            &config,
            &EventEmitter::Tauri(app),
            &prompt_queue,
            settings,
        )
        .await
    }
    #[cfg(not(feature = "tauri-runtime"))]
    {
        let _ = settings;
        Err(AppCommandError::configuration_invalid("tauri-only command"))
    }
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn collaboration_send(
    input: SendCollaborationMessageInput,
    db: tauri::State<'_, AppDatabase>,
    prompt_queue: tauri::State<'_, PromptQueueHandle>,
    app: tauri::AppHandle,
) -> Result<CollaborationSendResult, AppCommandError> {
    collaboration_send_core(&db.conn, &EventEmitter::Tauri(app), &prompt_queue, input).await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn collaboration_interrupt(
    input: InterruptCollaborationInput,
    db: tauri::State<'_, AppDatabase>,
    manager: tauri::State<'_, ConnectionManager>,
    prompt_queue: tauri::State<'_, PromptQueueHandle>,
    app: tauri::AppHandle,
) -> Result<CollaborationInterruptResult, AppCommandError> {
    collaboration_interrupt_core(
        &db.conn,
        &manager,
        &EventEmitter::Tauri(app),
        &prompt_queue,
        input,
    )
    .await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn collaboration_send_interrupt(
    input: SendAndInterruptCollaborationInput,
    db: tauri::State<'_, AppDatabase>,
    manager: tauri::State<'_, ConnectionManager>,
    prompt_queue: tauri::State<'_, PromptQueueHandle>,
    app: tauri::AppHandle,
) -> Result<SendAndInterruptCollaborationResult, AppCommandError> {
    collaboration_send_interrupt_core(
        &db.conn,
        &manager,
        &EventEmitter::Tauri(app),
        &prompt_queue,
        input,
    )
    .await
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
pub async fn collaboration_timeline_projection(
    conversation_id: i32,
    db: tauri::State<'_, AppDatabase>,
) -> Result<CollaborationTimelineProjection, AppCommandError> {
    collaboration_timeline_projection_core(&db.conn, conversation_id).await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn collaboration_unread_overview(
    db: tauri::State<'_, AppDatabase>,
) -> Result<CollaborationUnreadOverview, AppCommandError> {
    collaboration_unread_overview_core(&db.conn).await
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
pub async fn collaboration_resolve(
    conversation_id: i32,
    delivery_id: String,
    db: tauri::State<'_, AppDatabase>,
    app: tauri::AppHandle,
) -> Result<CollaborationFeed, AppCommandError> {
    collaboration_resolve_core(
        &db.conn,
        &EventEmitter::Tauri(app),
        conversation_id,
        delivery_id,
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

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn collaboration_restore(
    conversation_id: i32,
    delivery_id: String,
    db: tauri::State<'_, AppDatabase>,
    app: tauri::AppHandle,
) -> Result<CollaborationFeed, AppCommandError> {
    collaboration_restore_core(
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
    use crate::acp::connection::ConnectionCommand;
    use crate::acp::internal_bus::EventBusMetrics;
    use crate::acp::session_collaboration::{
        SessionInboxFilter, SessionMessageDeliveryMode,
    };
    use crate::acp::InternalEventBus;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::{
        AgentType, CollaborationDeliveryHint, CollaborationInvocationPolicy, CollaborationUrgency,
        PromptQueueItemState,
    };
    use crate::web::event_bridge::WebEventBroadcaster;
    use sea_orm::{ConnectionTrait, DbBackend, Statement};
    use std::path::PathBuf;
    use std::sync::Arc;

    async fn live_queue(
        db: &AppDatabase,
        folder: i32,
        target: i32,
        busy: bool,
    ) -> (
        ConnectionManager,
        PromptQueueHandle,
        tokio::sync::mpsc::Receiver<ConnectionCommand>,
    ) {
        let manager = ConnectionManager::new();
        let commands = manager
            .insert_test_connection_live(
                "interrupt-target",
                AgentType::ClaudeCode,
                Some(PathBuf::from("/tmp/codeg-interrupt-command")),
                EventEmitter::Noop,
            )
            .await;
        let state = manager
            .get_state("interrupt-target")
            .await
            .expect("connection state");
        let mut state = state.write().await;
        state.folder_id = Some(folder);
        state.conversation_id = Some(target);
        state.turn_in_flight = busy;
        drop(state);
        let bus = Arc::new(InternalEventBus::new(Arc::new(EventBusMetrics::default())));
        let (prompt_queue, _task) = crate::prompt_queue::build_prompt_queue_runtime(
            db.conn.clone(),
            manager.clone_ref(),
            EventEmitter::Noop,
            bus,
        );
        (manager, prompt_queue, commands)
    }

    async fn enabled_agent_access(
        db: &AppDatabase,
        emitter: EventEmitter,
    ) -> DbSessionCollaboration {
        let config = SessionCollaborationRuntimeConfig::new();
        config
            .set(SessionCollaborationConfig { enabled: true })
            .await;
        DbSessionCollaboration::new(
            Arc::new(AppDatabase {
                conn: db.conn.clone(),
            }),
            emitter,
            PromptQueueHandle::disconnected_for_test(),
            config,
        )
    }

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
            &PromptQueueHandle::disconnected_for_test(),
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
            &PromptQueueHandle::disconnected_for_test(),
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

    #[tokio::test]
    async fn inactive_target_queues_until_the_session_reconnects() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-inactive-collaboration-target").await;
        let source = seed_conversation(&db, folder, AgentType::Codex).await;
        let target = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        let result = collaboration_send_core(
            &db.conn,
            &EventEmitter::Noop,
            &PromptQueueHandle::disconnected_for_test(),
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                body: "review after I open your session".to_string(),
                client_dedupe_id: "inactive-target-confirmation".to_string(),
                invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .expect("queue without cold-starting target");

        let delivery = &result.deliveries[0];
        assert_eq!(delivery.state, CollaborationDeliveryState::Queued);
        let queue = prompt_queue_service::snapshot(&db.conn, target)
            .await
            .expect("queue snapshot");
        assert_eq!(queue.items.len(), 1);
        assert_eq!(queue.items[0].state, PromptQueueItemState::Queued);
        assert!(queue.items[0].paused_reason.is_none());
    }

    #[tokio::test]
    async fn atomic_stop_and_send_cancels_old_busy_turn_after_persisting() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-interrupt-command").await;
        let source = seed_conversation(&db, folder, AgentType::Codex).await;
        let target = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        let (manager, prompt_queue, mut commands) = live_queue(&db, folder, target, true).await;

        let result = collaboration_send_interrupt_core(
            &db.conn,
            &manager,
            &EventEmitter::Noop,
            &prompt_queue,
            SendAndInterruptCollaborationInput {
                message: SendCollaborationMessageInput {
                    source_conversation_id: source,
                    target_conversation_ids: vec![target],
                    body: "change direction".into(),
                    client_dedupe_id: "atomic-interrupt-message".into(),
                    invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                    delivery_hint: CollaborationDeliveryHint::SteerIfSupported,
                    expects_reply: false,
                    urgency: CollaborationUrgency::Normal,
                    reply_to_event_id: None,
                },
                interrupt_client_dedupe_id: "atomic-interrupt-operation".into(),
                reason: "user requested stop and send".into(),
            },
        )
        .await
        .expect("atomic stop and send");

        assert!(matches!(
            commands.recv().await,
            Some(ConnectionCommand::Cancel)
        ));
        assert_eq!(
            result.interrupt.as_ref().unwrap().operation.state,
            CollaborationInterruptState::WaitingForTerminal
        );
        assert_eq!(
            result.message.deliveries[0].state,
            CollaborationDeliveryState::Queued
        );
        assert!(prompt_queue_service::snapshot(&db.conn, target)
            .await
            .unwrap()
            .paused_reason
            .is_some());
    }

    #[tokio::test]
    async fn atomic_stop_and_send_never_cancels_the_new_message_when_target_is_idle() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-interrupt-command").await;
        let source = seed_conversation(&db, folder, AgentType::Codex).await;
        let target = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        let (manager, prompt_queue, mut commands) = live_queue(&db, folder, target, false).await;

        let result = collaboration_send_interrupt_core(
            &db.conn,
            &manager,
            &EventEmitter::Noop,
            &prompt_queue,
            SendAndInterruptCollaborationInput {
                message: SendCollaborationMessageInput {
                    source_conversation_id: source,
                    target_conversation_ids: vec![target],
                    body: "run next".into(),
                    client_dedupe_id: "idle-interrupt-message".into(),
                    invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                    delivery_hint: CollaborationDeliveryHint::Default,
                    expects_reply: false,
                    urgency: CollaborationUrgency::Normal,
                    reply_to_event_id: None,
                },
                interrupt_client_dedupe_id: "idle-interrupt-operation".into(),
                reason: "user requested stop and send".into(),
            },
        )
        .await
        .expect("idle stop and send");

        assert_eq!(
            result.interrupt.as_ref().unwrap().operation.state,
            CollaborationInterruptState::Ready
        );
        assert!(
            commands.try_recv().is_err(),
            "idle target must not receive Cancel"
        );
        assert!(prompt_queue_service::snapshot(&db.conn, target)
            .await
            .unwrap()
            .paused_reason
            .is_none());
    }

    #[tokio::test]
    async fn agent_address_book_excludes_self_and_unsent_drafts() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-agent-address-book").await;
        let source = seed_conversation(&db, folder, AgentType::Codex).await;
        let target = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        let draft = seed_conversation(&db, folder, AgentType::Gemini).await;
        conversation_service::update_external_id(&db.conn, source, "codex-source".into())
            .await
            .unwrap();
        conversation_service::update_external_id(&db.conn, target, "claude-target".into())
            .await
            .unwrap();
        let access = enabled_agent_access(&db, EventEmitter::Noop).await;

        let result = access.list_sessions(source, None, 50).await;
        assert!(result.available);
        assert_eq!(result.caller_session_id, Some(source));
        assert_eq!(result.sessions.len(), 1);
        assert_eq!(result.sessions[0].session_id, target);
        assert_eq!(result.sessions[0].agent_type, "claude_code");
        assert_ne!(result.sessions[0].session_id, draft);
    }

    #[tokio::test]
    async fn agent_send_reuses_event_without_enqueuing() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-agent-send").await;
        let source = seed_conversation(&db, folder, AgentType::Codex).await;
        let target = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        let access = enabled_agent_access(&db, EventEmitter::Noop).await;
        let spec = SessionMessageSpec {
            target_session_ids: vec![target],
            content: "check the argument".into(),
            delivery_mode: SessionMessageDeliveryMode::Queue,
            steer_if_supported: true,
            expects_reply: true,
            reply_to_event_id: None,
            client_dedupe_id: "mcp:agent-send".into(),
        };

        let sent = access.send_message(source, spec.clone()).await;
        assert!(sent.accepted);
        assert_eq!(sent.deliveries[0].state, "queued");
        let event_id = sent.event_id.clone().unwrap();
        let queued = prompt_queue_service::snapshot(&db.conn, target)
            .await
            .unwrap();
        assert_eq!(queued.items.len(), 1);
        let inbound = collaboration_service::feed(&db.conn, target, None)
            .await
            .unwrap()
            .inbound
            .remove(0);
        assert_eq!(inbound.event_id, event_id);
        assert!(inbound.expects_reply);
        assert_eq!(
            inbound.delivery_hint,
            CollaborationDeliveryHint::SteerIfSupported
        );

        let replay = access.send_message(source, spec).await;
        assert!(replay.accepted && replay.deduplicated);
        assert_eq!(replay.event_id, Some(event_id));
        assert_eq!(
            prompt_queue_service::snapshot(&db.conn, target)
                .await
                .unwrap()
                .items
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn agent_reply_chain_preserves_each_explicit_reply_obligation() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-agent-reply-budget").await;
        let first = seed_conversation(&db, folder, AgentType::Codex).await;
        let second = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        let access = enabled_agent_access(&db, EventEmitter::Noop).await;
        let mut source = first;
        let mut target = second;
        let mut reply_to_event_id = None;

        for depth in 0..=collaboration_service::MAX_AGENT_REPLY_CHAIN_DEPTH {
            let sent = access
                .send_message(
                    source,
                    SessionMessageSpec {
                        target_session_ids: vec![target],
                        content: format!("reply at depth {depth}"),
                        delivery_mode: SessionMessageDeliveryMode::DeliverOnly,
                        steer_if_supported: false,
                        expects_reply: true,
                        reply_to_event_id: reply_to_event_id.clone(),
                        client_dedupe_id: format!("mcp:reply-depth-{depth}"),
                    },
                )
                .await;
            assert!(sent.accepted, "depth {depth}: {:?}", sent.note);
            let event_id = sent.event_id.clone().expect("persisted event");
            let target_feed = collaboration_service::feed(&db.conn, target, None)
                .await
                .unwrap();
            let inbound = target_feed
                .inbound
                .iter()
                .find(|delivery| delivery.event_id == event_id)
                .expect("target sees the delivered event");
            assert!(inbound.expects_reply);
            assert_eq!(
                inbound.obligation_state,
                crate::models::CollaborationObligationState::AwaitingReply
            );
            assert!(sent.note.is_none());

            let row = db
                .conn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT chain_depth FROM collaboration_event WHERE id = ?",
                    vec![event_id.clone().into()],
                ))
                .await
                .unwrap()
                .unwrap();
            let stored_depth: i32 = row.try_get("", "chain_depth").unwrap();
            assert_eq!(stored_depth, depth);

            reply_to_event_id = Some(event_id);
            std::mem::swap(&mut source, &mut target);
        }
    }

    #[tokio::test]
    async fn disabled_host_core_rejects_agent_send_without_persisting() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-agent-send-disabled").await;
        let source = seed_conversation(&db, folder, AgentType::Codex).await;
        let target = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        let access = DbSessionCollaboration::new(
            Arc::new(AppDatabase {
                conn: db.conn.clone(),
            }),
            EventEmitter::Noop,
            PromptQueueHandle::disconnected_for_test(),
            SessionCollaborationRuntimeConfig::new(),
        );
        let result = access
            .send_message(
                source,
                SessionMessageSpec {
                    target_session_ids: vec![target],
                    content: "must not land".into(),
                    delivery_mode: SessionMessageDeliveryMode::Queue,
                    steer_if_supported: false,
                    expects_reply: true,
                    reply_to_event_id: None,
                    client_dedupe_id: "mcp:disabled".into(),
                },
            )
            .await;
        assert!(!result.accepted);
        assert!(collaboration_service::feed(&db.conn, source, Some(10))
            .await
            .unwrap()
            .outbound
            .is_empty());
    }

    #[tokio::test]
    async fn collaboration_tool_setting_defaults_on_and_applies_off_live() {
        let db = fresh_in_memory_db().await;
        assert!(load_session_collaboration_settings(&db.conn).await.enabled);
        let config = SessionCollaborationRuntimeConfig::new();
        let broadcaster = Arc::new(WebEventBroadcaster::new());
        let mut receiver = broadcaster.subscribe();
        let emitter = EventEmitter::test_web_only(broadcaster);

        let saved = set_session_collaboration_settings_core(
            &db.conn,
            &config,
            &emitter,
            &PromptQueueHandle::disconnected_for_test(),
            SessionCollaborationSettings { enabled: false },
        )
        .await
        .unwrap();
        assert!(!saved.enabled);
        assert!(!config.is_enabled().await);
        assert!(!load_session_collaboration_settings(&db.conn).await.enabled);
        let event = receiver.recv().await.unwrap();
        assert_eq!(event.channel, SESSION_COLLABORATION_SETTINGS_CHANGED_EVENT);
    }

    #[tokio::test]
    async fn collaboration_setting_freezes_and_restores_existing_dispatches() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-collaboration-policy").await;
        let source = seed_conversation(&db, folder, AgentType::Codex).await;
        let target = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        collaboration_service::send(
            &db.conn,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                body: "wait until collaboration is enabled again".into(),
                client_dedupe_id: "setting-policy-transition".into(),
                invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .expect("seed queued collaboration");
        let config = SessionCollaborationRuntimeConfig::new();
        config
            .set(SessionCollaborationConfig { enabled: true })
            .await;
        let queue = PromptQueueHandle::disconnected_for_test();

        set_session_collaboration_settings_core(
            &db.conn,
            &config,
            &EventEmitter::Noop,
            &queue,
            SessionCollaborationSettings { enabled: false },
        )
        .await
        .expect("disable collaboration");
        let frozen = prompt_queue_service::snapshot(&db.conn, target)
            .await
            .expect("frozen queue");
        assert_eq!(frozen.items[0].state, PromptQueueItemState::Paused);
        assert_eq!(
            frozen.items[0].paused_reason.as_deref(),
            Some(prompt_queue_service::COLLABORATION_DISABLED_REASON)
        );

        set_session_collaboration_settings_core(
            &db.conn,
            &config,
            &EventEmitter::Noop,
            &queue,
            SessionCollaborationSettings { enabled: true },
        )
        .await
        .expect("enable collaboration");
        let restored = prompt_queue_service::snapshot(&db.conn, target)
            .await
            .expect("restored queue");
        assert_eq!(restored.items[0].state, PromptQueueItemState::Queued);
        assert!(restored.items[0].paused_reason.is_none());
    }

    #[tokio::test]
    async fn agent_inbox_lists_preview_and_read_marks_agent_received() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-agent-inbox").await;
        let source = seed_conversation(&db, folder, AgentType::Codex).await;
        let target = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        let access = enabled_agent_access(&db, EventEmitter::Noop).await;
        let sent = collaboration_send_core(
            &db.conn,
            &EventEmitter::Noop,
            &PromptQueueHandle::disconnected_for_test(),
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                body: "please review claim 3 in the methods section".to_string(),
                client_dedupe_id: "agent-inbox-letter".to_string(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: true,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .expect("send");

        let listed = access
            .list_inbox(target, SessionInboxFilter::Open, 20)
            .await;
        assert!(listed.available);
        assert_eq!(listed.unread_count, 1);
        assert_eq!(listed.awaiting_reply_count, 1);
        assert_eq!(listed.items.len(), 1);
        assert_eq!(listed.items[0].event_id, sent.event_id);
        assert!(listed.items[0].unread);
        assert!(listed.items[0].expects_reply);
        assert!(listed.items[0].preview.contains("please review claim 3"));

        let opened = access.read_message(target, sent.event_id.clone()).await;
        assert!(opened.available);
        assert_eq!(
            opened.body.as_deref(),
            Some("please review claim 3 in the methods section")
        );
        assert!(!opened.unread);

        let unread = access
            .list_inbox(target, SessionInboxFilter::Unread, 20)
            .await;
        assert_eq!(unread.unread_count, 0);
        assert!(unread.items.is_empty());
    }
}
