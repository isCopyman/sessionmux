use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use sea_orm::EntityTrait;
use serde::{Deserialize, Serialize};

use crate::acp::manager::ConnectionManager;
use crate::acp::session_collaboration::{
    RoomPostSpec, RoomReadQuery, SessionAddress, SessionCollaborationAccess,
    SessionCollaborationConfig, SessionCollaborationRuntimeConfig, SessionListOutcome,
    SessionMessageDeliveryOutcome, SessionMessageSpec, SessionRoomEvent, SessionRoomListItem,
    SessionRoomListOutcome, SessionRoomMember, SessionRoomReadOutcome, SessionSendOutcome,
    MAX_SESSION_LIST_LIMIT, SEND_MESSAGE_ROOM_HINT,
};
use crate::app_error::AppCommandError;
use crate::db::service::{
    app_metadata_service, collaboration_interrupt_service, collaboration_room_service,
    collaboration_service, conversation_service, folder_service, prompt_queue_service,
};
use crate::db::AppDatabase;
use crate::models::{
    AddCollaborationRoomMembersInput, CollaborationChanged, CollaborationDeliveryHint,
    CollaborationDeliveryState, CollaborationFeed, CollaborationInterruptResult,
    CollaborationInterruptState, CollaborationInvocationPolicy, CollaborationRoomDetail,
    CollaborationRoomSummary, CollaborationSendResult, CollaborationTimelineProjection,
    CollaborationUnreadOverview, CreateCollaborationRoomInput, InterruptCollaborationInput,
    PostRoomMessageInput, RoomChanged, RoomPostResult, RoomTimeline,
    SendAndInterruptCollaborationInput, SendAndInterruptCollaborationResult,
    SendCollaborationMessageInput,
};
use crate::models::prompt_queue::PromptQueueSource;
use crate::prompt_queue::PromptQueueHandle;
use crate::session_dispatcher::connection_is_live;
use crate::web::event_bridge::{
    emit_event, EventEmitter, COLLABORATION_CHANGED_EVENT, PROMPT_QUEUE_CHANGED_EVENT,
    ROOM_CHANGED_EVENT, SESSION_COLLABORATION_SETTINGS_CHANGED_EVENT,
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
    manager: ConnectionManager,
    config: SessionCollaborationRuntimeConfig,
}

impl DbSessionCollaboration {
    pub fn new(
        db: Arc<AppDatabase>,
        emitter: EventEmitter,
        prompt_queue: PromptQueueHandle,
        manager: ConnectionManager,
        config: SessionCollaborationRuntimeConfig,
    ) -> Self {
        Self {
            db,
            emitter,
            prompt_queue,
            manager,
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
    // Do not pause closed targets for human confirmation. The dispatcher
    // starts or resumes a closed Session, then steers or starts a turn the
    // same way a human follow-up would.
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


/// No live Harness means there is no "next turn" to wait for. Normal mail
/// still has to reach the Agent, so the dispatcher must resume the Session.
async fn target_is_closed(
    conn: &sea_orm::DatabaseConnection,
    manager: Option<&ConnectionManager>,
    conversation_id: i32,
) -> bool {
    let Some(manager) = manager else {
        return false;
    };
    let Ok(Some(row)) = crate::db::entities::conversation::Entity::find_by_id(conversation_id)
        .one(conn)
        .await
    else {
        return false;
    };
    let Some((_, state)) = crate::prompt_queue::active_connection_for_row(manager, &row).await
    else {
        return true;
    };
    let status = state.read().await.status.clone();
    !connection_is_live(&status)
}

async fn wake_closed_session_for_normal_letter(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    prompt_queue: &PromptQueueHandle,
    event_id: &str,
    delivery_id: &str,
    target_conversation_id: i32,
) {
    let item_id = format!("closed-normal:{delivery_id}");
    match prompt_queue_service::enqueue_origin(
        conn,
        target_conversation_id,
        &item_id,
        event_id,
        &item_id,
        PromptQueueSource::Collaboration,
    )
    .await
    {
        Ok(true) => {
            publish(emitter, vec![target_conversation_id]);
            if let Ok(snapshot) =
                prompt_queue_service::snapshot(conn, target_conversation_id).await
            {
                emit_event(emitter, PROMPT_QUEUE_CHANGED_EVENT, snapshot);
            }
        }
        Ok(false) => {}
        Err(err) => tracing::warn!(
            "[collaboration] could not queue normal letter for closed Session {target_conversation_id}: {err}"
        ),
    }
    prompt_queue.wake(target_conversation_id);
}

async fn dispatch_persisted_deliveries(
    conn: &sea_orm::DatabaseConnection,
    manager: Option<&ConnectionManager>,
    emitter: &EventEmitter,
    prompt_queue: &PromptQueueHandle,
    high: bool,
    event_id: &str,
    deliveries: &[crate::models::CollaborationDeliveryView],
) {
    for delivery in deliveries.iter().filter(|delivery| {
        delivery.state != CollaborationDeliveryState::Failed
            && delivery.state != CollaborationDeliveryState::Dismissed
    }) {
        let target = delivery.target.conversation_id;
        // Persist skipped the prompt queue (archived Room @, or a failed
        // enqueue). High-priority dispatch must not invent a turn that was
        // never queued. Ordinary high / @ never auto-interrupts: wake so
        // idle starts a turn, busy+steer injects at most once this turn,
        // otherwise the letter waits for TurnComplete. Humans still use
        // "stop and send" for an explicit interrupt.
        if high && delivery.queue_item_id.is_none() {
            continue;
        }
        if high {
            prompt_queue.wake(target);
        } else if target_is_closed(conn, manager, target).await {
            wake_closed_session_for_normal_letter(
                conn,
                emitter,
                prompt_queue,
                event_id,
                &delivery.id,
                target,
            )
            .await;
        }
    }
}

pub async fn collaboration_send_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    prompt_queue: &PromptQueueHandle,
    manager: Option<&ConnectionManager>,
    mut input: SendCollaborationMessageInput,
) -> Result<CollaborationSendResult, AppCommandError> {
    // Persist first. `priority=high` notifies now: steer if the busy target
    // supports it, otherwise wait for the current turn to finish and flush
    // queued envelopes together. `priority=normal` waits for the next
    // ordinary turn when the Session is live, and resumes a closed Session
    // so the letter can still arrive. Both are Agent mail. Humans who want
    // to stop the current turn use collaboration_send_interrupt_core.
    let high = input.invocation_policy == CollaborationInvocationPolicy::InvokeWhenIdle;
    if high {
        input.delivery_hint = CollaborationDeliveryHint::SteerIfSupported;
    }
    let result = persist_collaboration_message(conn, prompt_queue, input).await?;
    publish_persisted_message(emitter, &result);
    if !result.deduplicated {
        dispatch_persisted_deliveries(
            conn,
            manager,
            emitter,
            prompt_queue,
            high,
            &result.event_id,
            &result.deliveries,
        )
        .await;
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
        if spec.room_id.is_some() || spec.mention_all {
            return SessionSendOutcome::rejected(Some(source_session_id), SEND_MESSAGE_ROOM_HINT);
        }
        let priority = spec.resolved_priority();
        let result = collaboration_send_core(
            &self.db.conn,
            &self.emitter,
            &self.prompt_queue,
            Some(&self.manager),
            SendCollaborationMessageInput {
                source_conversation_id: source_session_id,
                target_conversation_ids: spec.target_session_ids,
                subject: spec.title,
                body: spec.content,
                client_dedupe_id: spec.client_dedupe_id,
                invocation_policy: priority.invocation_policy(),
                delivery_hint: if spec.steer_if_supported {
                    CollaborationDeliveryHint::SteerIfSupported
                } else {
                    CollaborationDeliveryHint::Default
                },
                expects_reply: spec.expects_reply,
                urgency: priority.urgency(),
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
                room_id: None,
                note: None,
            },
            Err(err) => SessionSendOutcome::rejected(Some(source_session_id), err.to_string()),
        }
    }

    async fn list_inbox(
        &self,
        caller_session_id: i32,
        scope: crate::acp::session_collaboration::SessionMailboxScope,
        filter: crate::acp::session_collaboration::SessionInboxFilter,
        peer_session_id: Option<i32>,
        limit: u32,
    ) -> crate::acp::session_collaboration::SessionInboxOutcome {
        use crate::acp::session_collaboration::{
            SessionInboxItem, SessionInboxOutcome, MAX_INBOX_LIMIT,
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
            scope,
            filter,
            peer_session_id,
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
        let outbound = scope == crate::acp::session_collaboration::SessionMailboxScope::Sent;
        SessionInboxOutcome {
            available: true,
            caller_session_id: Some(caller_session_id),
            scope: Some(scope),
            unread_count: feed.unread_count,
            awaiting_reply_count,
            truncated: items.len() as u32 >= limit.clamp(1, MAX_INBOX_LIMIT),
            items: items
                .into_iter()
                .map(|item| {
                    // from_* carries the OTHER party: sender for inbox, recipient for sent.
                    let peer = if outbound { item.target } else { item.source };
                    SessionInboxItem {
                        event_id: item.event_id,
                        delivery_id: item.id,
                        direction: if outbound { "outbound" } else { "inbound" }.to_string(),
                        from_session_id: peer.conversation_id,
                        from_title: peer.title,
                        from_agent_type: peer.agent_type,
                        title: crate::acp::session_collaboration::letter_title(
                            &item.subject,
                            &item.body,
                        ),
                        preview: crate::acp::session_collaboration::letter_title(
                            &item.subject,
                            &item.body,
                        ),
                        unread: item.agent_received_at.is_none(),
                        expects_reply: item.expects_reply,
                        obligation_state: item.obligation_state.as_str().to_string(),
                        created_at: item.created_at,
                    }
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
            title: Some(crate::acp::session_collaboration::letter_title(
                &item.subject,
                &item.body,
            )),
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

    async fn list_rooms(
        &self,
        caller_session_id: i32,
        query: Option<String>,
        limit: u32,
    ) -> SessionRoomListOutcome {
        if !self.config.is_enabled().await {
            return SessionRoomListOutcome::unavailable(
                Some(caller_session_id),
                "Session collaboration is disabled in Codeg settings.",
            );
        }
        let rooms = match collaboration_room_service::list_for_member(
            &self.db.conn,
            caller_session_id,
        )
        .await
        {
            Ok(rooms) => rooms,
            Err(err) => {
                return SessionRoomListOutcome::unavailable(Some(caller_session_id), err.to_string())
            }
        };
        let query = query
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase());
        let filtered: Vec<_> = rooms
            .into_iter()
            .filter(|room| {
                query.as_ref().is_none_or(|needle| {
                    room.title.to_ascii_lowercase().contains(needle)
                        || room.id.to_ascii_lowercase().contains(needle)
                })
            })
            .collect();
        let truncated = filtered.len() as u32 > limit;
        let rooms = filtered
            .into_iter()
            .take(limit as usize)
            .map(|room| SessionRoomListItem {
                room_id: room.id,
                title: room.title,
                member_count: room.member_count,
                unread_count: room.unread_count,
                mention_unread_count: room.mention_unread_count,
                last_event_at: room.last_event_at,
            })
            .collect();
        SessionRoomListOutcome {
            available: true,
            caller_session_id: Some(caller_session_id),
            rooms,
            truncated,
            note: None,
        }
    }

    async fn read_room(
        &self,
        caller_session_id: i32,
        query: RoomReadQuery,
    ) -> SessionRoomReadOutcome {
        if !self.config.is_enabled().await {
            return SessionRoomReadOutcome::unavailable(
                Some(caller_session_id),
                "Session collaboration is disabled in Codeg settings.",
            );
        }
        if let Err(err) = collaboration_room_service::require_member(
            &self.db.conn,
            &query.room_id,
            caller_session_id,
        )
        .await
        {
            return SessionRoomReadOutcome::unavailable(Some(caller_session_id), err.to_string());
        }
        let detail = match collaboration_room_service::get(&self.db.conn, &query.room_id).await {
            Ok(detail) => detail,
            Err(err) => {
                return SessionRoomReadOutcome::unavailable(Some(caller_session_id), err.to_string())
            }
        };
        let mode = if query.unread {
            collaboration_room_service::RoomTimelineMode::Unread {
                conversation_id: caller_session_id,
            }
        } else if let Some(before) = query.before_event_id.as_deref() {
            collaboration_room_service::RoomTimelineMode::Before { event_id: before }
        } else {
            collaboration_room_service::RoomTimelineMode::Recent
        };
        let timeline = match collaboration_room_service::timeline_with(
            &self.db.conn,
            &query.room_id,
            Some(query.limit),
            mode,
        )
        .await
        {
            Ok(timeline) => timeline,
            Err(err) => {
                return SessionRoomReadOutcome::unavailable(Some(caller_session_id), err.to_string())
            }
        };
        let event_ids: Vec<String> = timeline.events.iter().map(|event| event.id.clone()).collect();
        if let Err(err) = collaboration_room_service::consume_room_window(
            &self.db.conn,
            &query.room_id,
            caller_session_id,
            &event_ids,
            query.unread,
        )
        .await
        {
            return SessionRoomReadOutcome::unavailable(Some(caller_session_id), err.to_string());
        }
        let note = if timeline.truncated {
            if query.unread {
                Some("More unread posts remain. Call read_room again with unread=true.".to_string())
            } else if let Some(first) = timeline.events.first() {
                Some(format!(
                    "Older posts remain. Call read_room with before_event_id={}",
                    first.id
                ))
            } else {
                None
            }
        } else {
            None
        };
        SessionRoomReadOutcome {
            available: true,
            caller_session_id: Some(caller_session_id),
            room_id: Some(detail.id),
            title: Some(detail.title),
            members: detail
                .members
                .into_iter()
                .map(|member| SessionRoomMember {
                    session_id: member.conversation_id,
                    title: member.title,
                    agent_type: member.agent_type,
                    role: member.role,
                })
                .collect(),
            events: timeline
                .events
                .into_iter()
                .map(|event| SessionRoomEvent {
                    event_id: event.id,
                    from_session_id: event.source.conversation_id,
                    from_title: event.source.title,
                    title: crate::acp::session_collaboration::letter_title(
                        &event.subject,
                        &event.body,
                    ),
                    body: event.body,
                    reply_to_event_id: event.reply_to_event_id,
                    mention_session_ids: event.mention_conversation_ids,
                    mention_human: event.mention_human,
                    from_author_kind: event.author_kind.as_str().to_string(),
                    created_at: event.created_at,
                })
                .collect(),
            truncated: timeline.truncated,
            note,
        }
    }

    async fn post_room(&self, source_session_id: i32, spec: RoomPostSpec) -> SessionSendOutcome {
        if !self.config.is_enabled().await {
            return SessionSendOutcome::rejected(
                Some(source_session_id),
                "Session collaboration is disabled in Codeg settings.",
            );
        }
        let priority = spec.resolved_priority();
        let result = collaboration_room_post_core(
            &self.db.conn,
            &self.emitter,
            &self.prompt_queue,
            Some(&self.manager),
            PostRoomMessageInput {
                room_id: spec.room_id,
                source_conversation_id: source_session_id,
                target_conversation_ids: spec.mention_session_ids,
                mention_all: spec.mention_all,
                body: spec.content,
                client_dedupe_id: spec.client_dedupe_id,
                invocation_policy: priority.invocation_policy(),
                delivery_hint: if priority
                    == crate::acp::session_collaboration::SessionMessagePriority::High
                {
                    CollaborationDeliveryHint::SteerIfSupported
                } else {
                    CollaborationDeliveryHint::Default
                },
                expects_reply: spec.expects_reply,
                urgency: priority.urgency(),
                reply_to_event_id: spec.reply_to_event_id,
                mention_human: spec.mention_human,
                author_kind: crate::models::CollaborationAuthorKind::Session,
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
                room_id: Some(result.room_id.clone()),
                note: Some(format!("Posted to Room {}", result.room_id)),
            },
            Err(err) => SessionSendOutcome::rejected(Some(source_session_id), err.to_string()),
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
    manager: tauri::State<'_, ConnectionManager>,
    app: tauri::AppHandle,
) -> Result<CollaborationSendResult, AppCommandError> {
    collaboration_send_core(
        &db.conn,
        &EventEmitter::Tauri(app),
        &prompt_queue,
        Some(&manager),
        input,
    )
    .await
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

fn publish_room(emitter: &EventEmitter, room_id: &str, workbench_id: i32) {
    emit_event(
        emitter,
        ROOM_CHANGED_EVENT,
        RoomChanged {
            room_id: room_id.to_string(),
            workbench_id,
        },
    );
}

pub async fn collaboration_room_create_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    input: CreateCollaborationRoomInput,
) -> Result<CollaborationRoomDetail, AppCommandError> {
    let room = collaboration_room_service::create(conn, input).await?;
    publish_room(emitter, &room.id, room.workbench_id);
    Ok(room)
}

pub async fn collaboration_room_list_core(
    conn: &sea_orm::DatabaseConnection,
    workbench_id: i32,
) -> Result<Vec<CollaborationRoomSummary>, AppCommandError> {
    collaboration_room_service::list_for_workbench(conn, workbench_id)
        .await
        .map_err(AppCommandError::from)
}

pub async fn collaboration_room_get_core(
    conn: &sea_orm::DatabaseConnection,
    room_id: &str,
) -> Result<CollaborationRoomDetail, AppCommandError> {
    collaboration_room_service::get(conn, room_id)
        .await
        .map_err(AppCommandError::from)
}

pub async fn collaboration_room_add_members_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    input: AddCollaborationRoomMembersInput,
) -> Result<CollaborationRoomDetail, AppCommandError> {
    let room = collaboration_room_service::add_members(conn, input).await?;
    publish_room(emitter, &room.id, room.workbench_id);
    Ok(room)
}

pub async fn collaboration_room_remove_member_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    room_id: &str,
    conversation_id: i32,
) -> Result<CollaborationRoomDetail, AppCommandError> {
    let room = collaboration_room_service::remove_member(conn, room_id, conversation_id).await?;
    publish_room(emitter, &room.id, room.workbench_id);
    Ok(room)
}

pub async fn collaboration_room_rename_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    room_id: &str,
    title: &str,
) -> Result<CollaborationRoomDetail, AppCommandError> {
    let room = collaboration_room_service::rename(conn, room_id, title).await?;
    publish_room(emitter, &room.id, room.workbench_id);
    Ok(room)
}

pub async fn collaboration_room_assign_collection_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    room_ids: Vec<String>,
    collection_id: Option<i32>,
    root_folder_id: Option<i32>,
) -> Result<Vec<CollaborationRoomSummary>, AppCommandError> {
    let rooms = collaboration_room_service::assign_to_collection(
        conn,
        room_ids,
        collection_id,
        root_folder_id,
    )
    .await?;
    for room in &rooms {
        publish_room(emitter, &room.id, room.workbench_id);
    }
    Ok(rooms)
}

pub async fn collaboration_room_mark_seen_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    room_id: &str,
    conversation_id: Option<i32>,
) -> Result<CollaborationRoomDetail, AppCommandError> {
    let room = collaboration_room_service::mark_seen(conn, room_id, conversation_id).await?;
    publish_room(emitter, &room.id, room.workbench_id);
    Ok(room)
}

pub async fn collaboration_room_timeline_core(
    conn: &sea_orm::DatabaseConnection,
    room_id: &str,
    limit: Option<u32>,
    before_event_id: Option<&str>,
) -> Result<RoomTimeline, AppCommandError> {
    let mode = match before_event_id {
        Some(event_id) => collaboration_room_service::RoomTimelineMode::Before { event_id },
        None => collaboration_room_service::RoomTimelineMode::Recent,
    };
    collaboration_room_service::timeline_with(conn, room_id, limit, mode)
        .await
        .map_err(AppCommandError::from)
}

pub async fn collaboration_room_post_core(
    conn: &sea_orm::DatabaseConnection,
    emitter: &EventEmitter,
    prompt_queue: &PromptQueueHandle,
    manager: Option<&ConnectionManager>,
    mut input: PostRoomMessageInput,
) -> Result<RoomPostResult, AppCommandError> {
    let high = input.invocation_policy == CollaborationInvocationPolicy::InvokeWhenIdle;
    if high {
        input.delivery_hint = CollaborationDeliveryHint::SteerIfSupported;
    }
    let result = collaboration_service::post_room(conn, input).await?;
    if let Ok(detail) = collaboration_room_service::get(conn, &result.room_id).await {
        publish_room(emitter, &result.room_id, detail.workbench_id);
    }
    if !result.deduplicated {
        publish(emitter, result.affected_conversation_ids.clone());
        dispatch_persisted_deliveries(
            conn,
            manager,
            emitter,
            prompt_queue,
            high,
            &result.event_id,
            &result.deliveries,
        )
        .await;
    }
    Ok(result)
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn collaboration_room_create(
    input: CreateCollaborationRoomInput,
    db: tauri::State<'_, AppDatabase>,
    app: tauri::AppHandle,
) -> Result<CollaborationRoomDetail, AppCommandError> {
    collaboration_room_create_core(&db.conn, &EventEmitter::Tauri(app), input).await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn collaboration_room_list(
    workbench_id: i32,
    db: tauri::State<'_, AppDatabase>,
) -> Result<Vec<CollaborationRoomSummary>, AppCommandError> {
    collaboration_room_list_core(&db.conn, workbench_id).await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn collaboration_room_get(
    room_id: String,
    db: tauri::State<'_, AppDatabase>,
) -> Result<CollaborationRoomDetail, AppCommandError> {
    collaboration_room_get_core(&db.conn, &room_id).await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn collaboration_room_add_members(
    input: AddCollaborationRoomMembersInput,
    db: tauri::State<'_, AppDatabase>,
    app: tauri::AppHandle,
) -> Result<CollaborationRoomDetail, AppCommandError> {
    collaboration_room_add_members_core(&db.conn, &EventEmitter::Tauri(app), input).await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn collaboration_room_remove_member(
    room_id: String,
    conversation_id: i32,
    db: tauri::State<'_, AppDatabase>,
    app: tauri::AppHandle,
) -> Result<CollaborationRoomDetail, AppCommandError> {
    collaboration_room_remove_member_core(
        &db.conn,
        &EventEmitter::Tauri(app),
        &room_id,
        conversation_id,
    )
    .await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn collaboration_room_rename(
    room_id: String,
    title: String,
    db: tauri::State<'_, AppDatabase>,
    app: tauri::AppHandle,
) -> Result<CollaborationRoomDetail, AppCommandError> {
    collaboration_room_rename_core(&db.conn, &EventEmitter::Tauri(app), &room_id, &title).await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn collaboration_room_assign_collection(
    room_ids: Vec<String>,
    collection_id: Option<i32>,
    root_folder_id: Option<i32>,
    db: tauri::State<'_, AppDatabase>,
    app: tauri::AppHandle,
) -> Result<Vec<CollaborationRoomSummary>, AppCommandError> {
    collaboration_room_assign_collection_core(
        &db.conn,
        &EventEmitter::Tauri(app),
        room_ids,
        collection_id,
        root_folder_id,
    )
    .await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn collaboration_room_mark_seen(
    room_id: String,
    conversation_id: Option<i32>,
    db: tauri::State<'_, AppDatabase>,
    app: tauri::AppHandle,
) -> Result<CollaborationRoomDetail, AppCommandError> {
    collaboration_room_mark_seen_core(
        &db.conn,
        &EventEmitter::Tauri(app),
        &room_id,
        conversation_id,
    )
    .await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn collaboration_room_timeline(
    room_id: String,
    limit: Option<u32>,
    before_event_id: Option<String>,
    db: tauri::State<'_, AppDatabase>,
) -> Result<RoomTimeline, AppCommandError> {
    collaboration_room_timeline_core(
        &db.conn,
        &room_id,
        limit,
        before_event_id.as_deref(),
    )
    .await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn collaboration_room_post(
    input: PostRoomMessageInput,
    db: tauri::State<'_, AppDatabase>,
    prompt_queue: tauri::State<'_, PromptQueueHandle>,
    manager: tauri::State<'_, ConnectionManager>,
    app: tauri::AppHandle,
) -> Result<RoomPostResult, AppCommandError> {
    collaboration_room_post_core(
        &db.conn,
        &EventEmitter::Tauri(app),
        &prompt_queue,
        Some(&manager),
        input,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::connection::ConnectionCommand;
    use crate::acp::internal_bus::EventBusMetrics;
    use crate::acp::session_collaboration::{
        SessionInboxFilter, SessionMessageDeliveryMode, RoomReadQuery,
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
            ConnectionManager::new(),
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
            None,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                subject: "Test letter".into(),
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
            None,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                subject: "Test letter".into(),
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
            None,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                subject: "Test letter".into(),
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
                    subject: "Test letter".into(),
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
                    subject: "Test letter".into(),
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
    async fn letter_priority_high_queues_now_and_normal_waits_for_next_turn() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-letter-priority").await;
        let source = seed_conversation(&db, folder, AgentType::Codex).await;
        let high_target = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        let normal_target = seed_conversation(&db, folder, AgentType::Gemini).await;
        let access = enabled_agent_access(&db, EventEmitter::Noop).await;

        let high = access
            .send_message(
                source,
                SessionMessageSpec {
                    target_session_ids: vec![high_target],
                    title: "Need this now".into(),
                    content: "please look".into(),
                    delivery_mode: SessionMessageDeliveryMode::Queue,
                    priority: Some(crate::acp::session_collaboration::SessionMessagePriority::High),
                    steer_if_supported: false,
                    expects_reply: false,
                    reply_to_event_id: None,
                    client_dedupe_id: "priority-high".into(),
                    room_id: None,
                    mention_all: false,
                },
            )
            .await;
        assert!(high.accepted);
        assert_eq!(high.deliveries[0].state, "queued");
        assert_eq!(
            prompt_queue_service::snapshot(&db.conn, high_target)
                .await
                .unwrap()
                .items
                .len(),
            1
        );
        let high_inbound = collaboration_service::feed(&db.conn, high_target, None)
            .await
            .unwrap()
            .inbound
            .remove(0);
        assert_eq!(
            high_inbound.delivery_hint,
            CollaborationDeliveryHint::SteerIfSupported
        );

        let normal = access
            .send_message(
                source,
                SessionMessageSpec {
                    target_session_ids: vec![normal_target],
                    title: "When you have a moment".into(),
                    content: "no rush".into(),
                    delivery_mode: SessionMessageDeliveryMode::DeliverOnly,
                    priority: Some(
                        crate::acp::session_collaboration::SessionMessagePriority::Normal,
                    ),
                    steer_if_supported: false,
                    expects_reply: false,
                    reply_to_event_id: None,
                    client_dedupe_id: "priority-normal".into(),
                    room_id: None,
                    mention_all: false,
                },
            )
            .await;
        assert!(normal.accepted);
        assert_eq!(
            prompt_queue_service::snapshot(&db.conn, normal_target)
                .await
                .unwrap()
                .items
                .len(),
            1
        );
        let inbound = collaboration_service::feed(&db.conn, normal_target, None)
            .await
            .unwrap()
            .inbound
            .remove(0);
        assert_eq!(
            inbound.invocation_policy,
            CollaborationInvocationPolicy::StoreOnly
        );
        assert_eq!(inbound.state, CollaborationDeliveryState::Queued);
    }

    #[tokio::test]
    async fn normal_priority_does_not_wake_a_live_idle_session() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-normal-live").await;
        let source = seed_conversation(&db, folder, AgentType::Codex).await;
        let target = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        let (manager, prompt_queue, mut commands) = live_queue(&db, folder, target, false).await;

        let result = collaboration_send_core(
            &db.conn,
            &EventEmitter::Noop,
            &prompt_queue,
            Some(&manager),
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                subject: "When you have a moment".into(),
                body: "no rush".into(),
                client_dedupe_id: "normal-live".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .expect("send normal");

        assert_eq!(result.deliveries[0].state, CollaborationDeliveryState::Pending);
        assert!(prompt_queue_service::snapshot(&db.conn, target)
            .await
            .unwrap()
            .items
            .is_empty());
        assert!(
            commands.try_recv().is_err(),
            "live idle Session must not get a turn for normal mail"
        );
    }

    #[tokio::test]
    async fn high_priority_does_not_cancel_when_native_steer_exists() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-high-steer").await;
        let source = seed_conversation(&db, folder, AgentType::Codex).await;
        let target = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        let (manager, prompt_queue, mut commands) = live_queue(&db, folder, target, true).await;
        manager
            .get_state("interrupt-target")
            .await
            .expect("state")
            .write()
            .await
            .native_steering_available = true;

        collaboration_send_core(
            &db.conn,
            &EventEmitter::Noop,
            &prompt_queue,
            Some(&manager),
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                subject: "Need this now".into(),
                body: "please look".into(),
                client_dedupe_id: "high-steer".into(),
                invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Urgent,
                reply_to_event_id: None,
            },
        )
        .await
        .expect("send high");

        assert!(
            commands.try_recv().is_err(),
            "native steer must not stop the current turn"
        );
        assert_eq!(
            collaboration_service::feed(&db.conn, target, None)
                .await
                .unwrap()
                .inbound[0]
                .delivery_hint,
            CollaborationDeliveryHint::SteerIfSupported
        );
    }

    #[tokio::test]
    async fn high_priority_stays_queued_when_busy_without_steer() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-high-interrupt").await;
        let source = seed_conversation(&db, folder, AgentType::Codex).await;
        let target = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        let (manager, prompt_queue, mut commands) = live_queue(&db, folder, target, true).await;

        let result = collaboration_send_core(
            &db.conn,
            &EventEmitter::Noop,
            &prompt_queue,
            Some(&manager),
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                subject: "Need this now".into(),
                body: "please look".into(),
                client_dedupe_id: "high-interrupt".into(),
                invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Urgent,
                reply_to_event_id: None,
            },
        )
        .await
        .expect("send high");

        assert!(
            commands.try_recv().is_err(),
            "ordinary high mail must not auto-interrupt a busy turn"
        );
        assert_eq!(
            result.deliveries[0].state,
            CollaborationDeliveryState::Queued
        );
        let snapshot = prompt_queue_service::snapshot(&db.conn, target)
            .await
            .unwrap();
        assert!(snapshot.paused_reason.is_none());
        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(snapshot.items[0].state, PromptQueueItemState::Queued);
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
            title: "Test letter".into(),
            content: "check the argument".into(),
            delivery_mode: SessionMessageDeliveryMode::Queue,
            priority: Default::default(),
            steer_if_supported: true,
            expects_reply: true,
            reply_to_event_id: None,
            client_dedupe_id: "mcp:agent-send".into(),
            room_id: None,
            mention_all: false,
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
                        title: "Test letter".into(),
                        content: format!("reply at depth {depth}"),
                        delivery_mode: SessionMessageDeliveryMode::DeliverOnly,
                        priority: Default::default(),
                        steer_if_supported: false,
                        expects_reply: true,
                        reply_to_event_id: reply_to_event_id.clone(),
                        client_dedupe_id: format!("mcp:reply-depth-{depth}"),
                        room_id: None,
                        mention_all: false,
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
            ConnectionManager::new(),
            SessionCollaborationRuntimeConfig::new(),
        );
        let result = access
            .send_message(
                source,
                SessionMessageSpec {
                    target_session_ids: vec![target],
                    title: "Test letter".into(),
                    content: "must not land".into(),
                    delivery_mode: SessionMessageDeliveryMode::Queue,
                    priority: Default::default(),
                    steer_if_supported: false,
                    expects_reply: true,
                    reply_to_event_id: None,
                    client_dedupe_id: "mcp:disabled".into(),
                    room_id: None,
                    mention_all: false,
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
                subject: "Test letter".into(),
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
            None,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                subject: "Review claim 3".into(),
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
            .list_inbox(
                target,
                crate::acp::session_collaboration::SessionMailboxScope::Inbox,
                SessionInboxFilter::Open,
                None,
                20,
            )
            .await;
        assert!(listed.available);
        assert_eq!(listed.unread_count, 1);
        assert_eq!(listed.awaiting_reply_count, 1);
        assert_eq!(listed.items.len(), 1);
        assert_eq!(listed.items[0].event_id, sent.event_id);
        assert!(listed.items[0].unread);
        assert!(listed.items[0].expects_reply);
        assert_eq!(listed.items[0].title, "Review claim 3");
        assert_eq!(listed.items[0].preview, "Review claim 3");
        assert!(!listed.items[0].preview.contains("methods section"));

        let opened = access.read_message(target, sent.event_id.clone()).await;
        assert!(opened.available);
        assert_eq!(opened.title.as_deref(), Some("Review claim 3"));
        assert_eq!(
            opened.body.as_deref(),
            Some("please review claim 3 in the methods section")
        );
        assert!(!opened.unread);

        let unread = access
            .list_inbox(
                target,
                crate::acp::session_collaboration::SessionMailboxScope::Inbox,
                SessionInboxFilter::Unread,
                None,
                20,
            )
            .await;
        assert_eq!(unread.unread_count, 0);
        assert!(unread.items.is_empty());
    }

    #[tokio::test]
    async fn send_message_rejects_room_fields_and_post_room_writes_the_timeline() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-agent-room-tools").await;
        let source = seed_conversation(&db, folder, AgentType::Codex).await;
        let peer = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        let outsider = seed_conversation(&db, folder, AgentType::Gemini).await;
        let access = enabled_agent_access(&db, EventEmitter::Noop).await;
        let room = collaboration_room_service::create(
            &db.conn,
            CreateCollaborationRoomInput {
                workbench_id: 1,
                title: "Plan".into(),
                member_conversation_ids: vec![source, peer],
                created_by_conversation_id: source,
                collection_id: None,
                root_folder_id: None,
            },
        )
        .await
        .expect("create room");

        let rejected = access
            .send_message(
                source,
                SessionMessageSpec {
                    target_session_ids: vec![peer],
                    title: "Wrong tool".into(),
                    content: "should not become a room post".into(),
                    delivery_mode: SessionMessageDeliveryMode::Queue,
                    priority: Default::default(),
                    steer_if_supported: false,
                    expects_reply: false,
                    reply_to_event_id: None,
                    client_dedupe_id: "legacy-room-send".into(),
                    room_id: Some(room.id.clone()),
                    mention_all: false,
                },
            )
            .await;
        assert!(!rejected.accepted);
        assert!(rejected
            .note
            .as_deref()
            .unwrap_or("")
            .contains("post_room"));

        let listed = access.list_rooms(source, None, 50).await;
        assert!(listed.available);
        assert_eq!(listed.rooms.len(), 1);
        assert_eq!(listed.rooms[0].room_id, room.id);
        assert!(access.list_rooms(outsider, None, 50).await.rooms.is_empty());

        let posted = access
            .post_room(
                source,
                RoomPostSpec {
                    room_id: room.id.clone(),
                    content: "please look at the plan".into(),
                    mention_session_ids: vec![peer],
                    mention_all: false,
                    mention_human: false,
                    priority: Some(
                        crate::acp::session_collaboration::SessionMessagePriority::High,
                    ),
                    expects_reply: false,
                    reply_to_event_id: None,
                    client_dedupe_id: "mcp:post-room".into(),
                },
            )
            .await;
        assert!(posted.accepted, "{:?}", posted.note);
        assert_eq!(posted.room_id.as_deref(), Some(room.id.as_str()));
        assert_eq!(posted.deliveries.len(), 1);
        assert_eq!(posted.deliveries[0].target_session_id, peer);

        let timeline = access
            .read_room(
                source,
                RoomReadQuery {
                    room_id: room.id.clone(),
                    limit: 50,
                    unread: false,
                    before_event_id: None,
                },
            )
            .await;
        assert!(timeline.available);
        assert_eq!(timeline.events.len(), 1);
        assert_eq!(timeline.events[0].body, "please look at the plan");
        assert_eq!(timeline.events[0].mention_session_ids, vec![peer]);

        let peer_rooms = access.list_rooms(peer, None, 50).await;
        assert_eq!(peer_rooms.rooms[0].unread_count, 1);
        assert_eq!(peer_rooms.rooms[0].mention_unread_count, 1);

        let catch_up = access
            .read_room(
                peer,
                RoomReadQuery {
                    room_id: room.id.clone(),
                    limit: 50,
                    unread: true,
                    before_event_id: None,
                },
            )
            .await;
        assert!(catch_up.available);
        assert_eq!(catch_up.events.len(), 1);
        let peer_rooms = access.list_rooms(peer, None, 50).await;
        assert_eq!(peer_rooms.rooms[0].unread_count, 0);
        assert_eq!(peer_rooms.rooms[0].mention_unread_count, 0);

        let hidden = access
            .read_room(
                outsider,
                RoomReadQuery {
                    room_id: room.id.clone(),
                    limit: 50,
                    unread: false,
                    before_event_id: None,
                },
            )
            .await;
        assert!(!hidden.available);

        let mail = access
            .list_inbox(
                peer,
                crate::acp::session_collaboration::SessionMailboxScope::Inbox,
                SessionInboxFilter::All,
                None,
                20,
            )
            .await;
        assert!(mail.items.is_empty());
    }
}
