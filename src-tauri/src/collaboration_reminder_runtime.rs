//! Periodic mailbox reminder sweep.
//!
//! New unread mail is due immediately. Inject via native steering when that
//! channel is live; otherwise cancel the current turn and send the digest.
//! Idle Sessions get a host-authored digest turn. Closed Sessions wait.

use std::time::Duration;

use sea_orm::DatabaseConnection;

use crate::acp::collaboration_reminder::{
    choose_reminder_lane, reminder_digest_text, CollaborationReminderLane, ReminderAudience,
    ReminderRuntime, ReminderTargetState, REMINDER_SCAN_SECS,
};
use crate::acp::manager::ConnectionManager;
use crate::acp::types::{ConnectionStatus, PromptInputBlock};
use crate::commands::prompt_queue::prompt_queue_enqueue_core;
use crate::db::entities::conversation;
use crate::db::service::collaboration_service;
use crate::models::{EnqueuePromptQueueItem, PromptQueueDraft};
use crate::prompt_queue::{active_connection_for_row, PromptQueueHandle};
use crate::web::event_bridge::EventEmitter;
use sea_orm::EntityTrait;

pub async fn reminder_sweep_task(
    conn: DatabaseConnection,
    manager: ConnectionManager,
    prompt_queue: PromptQueueHandle,
    emitter: EventEmitter,
) {
    let mut ticker = tokio::time::interval(Duration::from_secs(REMINDER_SCAN_SECS));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticker.tick().await;
    loop {
        ticker.tick().await;
        if let Err(error) = sweep_once(&conn, &manager, &prompt_queue, &emitter).await {
            tracing::warn!("[collaboration-reminder] sweep failed: {error}");
        }
    }
}

pub async fn sweep_once(
    conn: &DatabaseConnection,
    manager: &ConnectionManager,
    prompt_queue: &PromptQueueHandle,
    emitter: &EventEmitter,
) -> Result<(), String> {
    if let Err(error) = collaboration_service::reset_idle_reminder_cursors(conn).await {
        tracing::debug!("[collaboration-reminder] cursor reset skipped: {error}");
    }
    let targets = collaboration_service::list_overdue_reminder_targets(conn)
        .await
        .map_err(|error| error.to_string())?;
    for target in targets {
        match dispatch_target(conn, manager, prompt_queue, emitter, &target).await {
            Ok(true) => {
                if let Err(error) =
                    collaboration_service::record_successful_reminder(conn, target.conversation_id)
                        .await
                {
                    tracing::warn!(
                        "[collaboration-reminder] could not record reminder for {}: {error}",
                        target.conversation_id
                    );
                }
            }
            Ok(false) => {}
            Err(error) => tracing::warn!(
                "[collaboration-reminder] Session {} skipped: {error}",
                target.conversation_id
            ),
        }
    }
    Ok(())
}

async fn dispatch_target(
    conn: &DatabaseConnection,
    manager: &ConnectionManager,
    prompt_queue: &PromptQueueHandle,
    emitter: &EventEmitter,
    target: &collaboration_service::ReminderTargetSnapshot,
) -> Result<bool, String> {
    let digest = reminder_digest_text(target.overdue_unread, target.overdue_reply);
    if digest.is_empty() {
        return Ok(false);
    }
    let Some(row) = conversation::Entity::find_by_id(target.conversation_id)
        .one(conn)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(false);
    };
    let (runtime, native_steering, connection_id) =
        match active_connection_for_row(manager, &row).await {
            Some((connection_id, state)) => {
                let state = state.read().await;
                let runtime = if state.status != ConnectionStatus::Connected {
                    ReminderRuntime::Missing
                } else if state.turn_in_flight {
                    ReminderRuntime::ConnectedBusy
                } else {
                    ReminderRuntime::ConnectedIdle
                };
                (runtime, state.native_steering_available, Some(connection_id))
            }
            None => (ReminderRuntime::Missing, false, None),
        };
    let lane = choose_reminder_lane(ReminderTargetState {
        audience: ReminderAudience::AgentSession,
        runtime,
        has_unread: target.overdue_unread > 0,
        has_awaiting_reply: target.overdue_reply > 0,
        native_steering,
    });
    match lane {
        CollaborationReminderLane::InjectSteer => {
            let Some(connection_id) = connection_id else {
                return Ok(false);
            };
            match manager
                .try_submit_native_feedback(&connection_id, digest)
                .await
            {
                Ok(Some(_)) => Ok(true),
                Ok(None) => Ok(false),
                Err(error) => Err(error.to_string()),
            }
        }
        CollaborationReminderLane::IdleStart
        | CollaborationReminderLane::ForceInterruptSend => {
            let item = EnqueuePromptQueueItem {
                conversation_id: target.conversation_id,
                id: format!("mailbox-reminder-{}", uuid::Uuid::new_v4()),
                client_dedupe_id: format!(
                    "mailbox-reminder:{}:{}",
                    target.conversation_id, target.reminder_repeat_count
                ),
                draft: PromptQueueDraft {
                    blocks: vec![PromptInputBlock::Text {
                        text: digest.clone(),
                    }],
                    display_text: digest,
                },
                mode_id: None,
            };
            prompt_queue_enqueue_core(conn, emitter, prompt_queue, item)
                .await
                .map_err(|error| error.to_string())?;
            if lane == CollaborationReminderLane::ForceInterruptSend {
                if let Some(connection_id) = connection_id {
                    if let Err(error) = manager.cancel(conn, &connection_id).await {
                        tracing::warn!(
                            "[collaboration-reminder] cancel for {} failed: {error}",
                            target.conversation_id
                        );
                    }
                }
            }
            Ok(true)
        }
        CollaborationReminderLane::HoldClosed
        | CollaborationReminderLane::HumanOverlay
        | CollaborationReminderLane::Silent => Ok(false),
    }
}
