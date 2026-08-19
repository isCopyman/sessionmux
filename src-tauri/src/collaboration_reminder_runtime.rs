//! Periodic mailbox reminder sweep.
//!
//! First delivery already carries the body. This sweep nags only after the
//! unread or read-awaiting-reply clock elapses. Inject via native steering
//! when that channel is live; otherwise enqueue attention on the same
//! PromptQueue used by the first notice. Idle and closed Sessions are
//! started so the Agent can receive it.

use std::time::Duration;

use sea_orm::DatabaseConnection;

use crate::acp::collaboration_reminder::{
    choose_reminder_lane, reminder_digest_text_with_letters, reminder_runtime,
    CollaborationReminderLane, ReminderAudience, ReminderRuntime, ReminderTargetState,
    REMINDER_SCAN_SECS,
};
use crate::acp::manager::ConnectionManager;
use crate::acp::types::PromptInputBlock;
use crate::commands::prompt_queue::prompt_queue_enqueue_core;
use crate::db::entities::conversation;
use crate::db::service::{collaboration_service, prompt_queue_service};
use crate::models::{CollaborationChanged, EnqueuePromptQueueItem, PromptQueueDraft};
use crate::prompt_queue::{active_connection_for_row, PromptQueueHandle};
use crate::web::event_bridge::{
    emit_event, EventEmitter, COLLABORATION_CHANGED_EVENT, PROMPT_QUEUE_CHANGED_EVENT,
};
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
                if let Err(error) = collaboration_service::record_successful_reminder(
                    conn,
                    target.conversation_id,
                    target.reminder_repeat_count,
                )
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
    let digest = reminder_digest_text_with_letters(
        target.overdue_unread,
        target.overdue_reply,
        &target.letters,
    );
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
                (
                    reminder_runtime(state.status.clone(), state.turn_in_flight),
                    state.native_steering_available,
                    Some(connection_id),
                )
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
            // One steer per turn, both directions: if a letter steer already
            // injected the full envelope into this turn, a reminder digest on
            // top would repeat it — downgrade to the durable queue. Otherwise
            // atomically claim the slot so a letter steer later in the same
            // turn stands down. The flag resets on TurnComplete.
            let Some(state) = manager.get_state(&connection_id).await else {
                return Ok(false);
            };
            {
                let mut guard = state.write().await;
                if guard.collaboration_steered_this_turn {
                    drop(guard);
                    return enqueue_mailbox_attention(conn, prompt_queue, emitter, target, digest)
                        .await;
                }
                guard.collaboration_steered_this_turn = true;
            }
            let outcome = manager
                .try_submit_native_feedback(&connection_id, digest)
                .await;
            match outcome {
                Ok(Some(_)) => Ok(true),
                // Definitely not injected: release this turn's steer slot.
                Ok(None) => {
                    state.write().await.collaboration_steered_this_turn = false;
                    Ok(false)
                }
                // Uncertain: keep the slot claimed rather than risk a double
                // injection later in the same turn.
                Err(error) => Err(error.to_string()),
            }
        }
        CollaborationReminderLane::IdleStart
        | CollaborationReminderLane::QueueAfterTurn
        | CollaborationReminderLane::EnsureRuntime => {
            enqueue_mailbox_attention(conn, prompt_queue, emitter, target, digest).await
        }
        CollaborationReminderLane::HumanOverlay | CollaborationReminderLane::Silent => Ok(false),
    }
}

fn publish_attention(emitter: &EventEmitter, conversation_id: i32) {
    emit_event(
        emitter,
        COLLABORATION_CHANGED_EVENT,
        CollaborationChanged {
            conversation_ids: vec![conversation_id],
        },
    );
}

async fn enqueue_mailbox_attention(
    conn: &DatabaseConnection,
    prompt_queue: &PromptQueueHandle,
    emitter: &EventEmitter,
    target: &collaboration_service::ReminderTargetSnapshot,
    digest: String,
) -> Result<bool, String> {
    prompt_queue.wake(target.conversation_id);
    publish_attention(emitter, target.conversation_id);
    if prompt_queue_service::has_pending_mailbox_attention(conn, target.conversation_id)
        .await
        .map_err(|error| error.to_string())?
    {
        return Ok(false);
    }
    if target.letters.len() == 1 {
        let letter = &target.letters[0];
        let item_id = uuid::Uuid::new_v4().to_string();
        let inserted = prompt_queue_service::enqueue_origin(
            conn,
            target.conversation_id,
            &item_id,
            &letter.event_id,
            &format!(
                "mailbox-attention:{}:{}",
                letter.event_id, target.reminder_repeat_count
            ),
            crate::models::PromptQueueSource::Reminder,
        )
        .await
        .map_err(|error| error.to_string())?;
        if inserted {
            // No second `prompt_queue.wake` here: the unconditional call at the
            // top of this function already woke `target.conversation_id`, and
            // nothing between there and here changes which conversation needs
            // waking.
            if let Ok(snapshot) = prompt_queue_service::snapshot(conn, target.conversation_id).await
            {
                emit_event(emitter, PROMPT_QUEUE_CHANGED_EVENT, snapshot);
            }
        }
        return Ok(inserted);
    }
    let item = EnqueuePromptQueueItem {
        conversation_id: target.conversation_id,
        id: format!("mailbox-attention-{}", uuid::Uuid::new_v4()),
        client_dedupe_id: format!(
            "mailbox-attention:{}:{}",
            target.conversation_id, target.reminder_repeat_count
        ),
        draft: PromptQueueDraft {
            blocks: vec![PromptInputBlock::Text {
                text: digest.clone(),
            }],
            display_text: digest,
        },
        mode_id: None,
        source: crate::models::PromptQueueSource::Reminder,
    };
    prompt_queue_enqueue_core(conn, emitter, prompt_queue, item)
        .await
        .map_err(|error| error.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::AgentType;
    use std::path::PathBuf;

    /// A turn that already swallowed one letter steer must not get a reminder
    /// digest steered in on top: the reminder downgrades to the durable queue
    /// and leaves the steer slot claimed.
    #[tokio::test]
    async fn reminder_steer_downgrades_to_queue_when_the_turn_was_already_steered() {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/codeg-reminder-steer").await;
        let conversation_id = seed_conversation(&db, folder_id, AgentType::Codex).await;
        let manager = ConnectionManager::new();
        let mut commands = manager
            .insert_test_connection_live(
                "conn-steered",
                AgentType::Codex,
                Some(PathBuf::from("/tmp/codeg-reminder-steer")),
                EventEmitter::Noop,
            )
            .await;
        let state = manager
            .get_state("conn-steered")
            .await
            .expect("connection state");
        {
            let mut guard = state.write().await;
            guard.conversation_id = Some(conversation_id);
            guard.folder_id = Some(folder_id);
            guard.turn_in_flight = true;
            guard.native_steering_available = true;
            guard.collaboration_steered_this_turn = true;
        }

        let target = collaboration_service::ReminderTargetSnapshot {
            conversation_id,
            overdue_unread: 1,
            overdue_reply: 0,
            reminder_repeat_count: 0,
            reminder_last_at: None,
            letters: Vec::new(),
        };
        let prompt_queue = PromptQueueHandle::disconnected_for_test();
        let delivered = dispatch_target(
            &db.conn,
            &manager,
            &prompt_queue,
            &EventEmitter::Noop,
            &target,
        )
        .await
        .expect("dispatch");

        assert!(delivered, "the digest still reaches the Session, queued");
        let snapshot = prompt_queue_service::snapshot(&db.conn, conversation_id)
            .await
            .expect("queue snapshot");
        assert_eq!(
            snapshot.items.len(),
            1,
            "an already-steered turn queues the digest instead of a second injection"
        );
        assert!(
            commands.try_recv().is_err(),
            "no native steer command may leave for the Harness"
        );
        assert!(
            state.read().await.collaboration_steered_this_turn,
            "the claimed slot stays claimed until TurnComplete"
        );
    }
}
