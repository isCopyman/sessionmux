//! Backend-authoritative follow-up queue for one persistent Session.
//!
//! The frontend may render the queue in several Workbenches/windows, but it
//! never owns dispatch. One process worker claims the FIFO head, hands it to
//! the already-running Harness through `ConnectionManager::send_prompt_linked`,
//! and removes it only after that call accepts the prompt. When a deliverable
//! item exists and the Session is closed, the dispatcher starts or resumes it.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::Duration;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use tokio::sync::{broadcast, mpsc};

use crate::acp::error::AcpError;
use crate::acp::manager::ConnectionManager;
use crate::acp::types::{AcpEvent, ConnectionStatus};
use crate::acp::InternalEventBus;
use crate::db::entities::{conversation, folder};
use crate::db::service::{
    collaboration_interrupt_service, collaboration_service, prompt_queue_service,
};
use crate::db::AppDatabase;
use crate::models::{
    AgentType, CollaborationChanged, CollaborationDeliveryHint, PromptQueueSnapshot,
};
use crate::parsers::path_eq_for_matching;
use crate::session_dispatcher::{
    connection_is_live, conversation_can_auto_start, EnsureGate, SessionDispatchConfig,
};
use crate::web::event_bridge::{
    emit_event, EventEmitter, COLLABORATION_CHANGED_EVENT, PROMPT_QUEUE_CHANGED_EVENT,
};

const CLAIM_LEASE_SECS: i64 = 30;
const DISPATCH_LEASE_SECS: i64 = 300;
const LEASE_SWEEP_SECS: u64 = 15;
const ACCEPT_RETRY_DELAYS_MS: [u64; 3] = [25, 75, 200];

pub struct PromptQueueHandle {
    wake_tx: mpsc::UnboundedSender<i32>,
    manager: ConnectionManager,
}

impl Clone for PromptQueueHandle {
    fn clone(&self) -> Self {
        Self {
            wake_tx: self.wake_tx.clone(),
            manager: self.manager.clone_ref(),
        }
    }
}

impl PromptQueueHandle {
    pub fn wake(&self, conversation_id: i32) {
        let _ = self.wake_tx.send(conversation_id);
    }

    /// Whether this Session currently has a connected Harness runtime. Busy is
    /// still active: its collaboration delivery may safely wait in FIFO. A
    /// missing runtime means a cross-Session request must require confirmation
    /// instead of automatically running when the user later opens the Session.
    pub async fn is_session_runtime_active(
        &self,
        conn: &sea_orm::DatabaseConnection,
        conversation_id: i32,
    ) -> Result<bool, crate::db::error::DbError> {
        let Some(row) = conversation::Entity::find_by_id(conversation_id)
            .one(conn)
            .await?
        else {
            return Ok(false);
        };
        let Some((_, state)) = active_connection_for_row(&self.manager, &row).await else {
            return Ok(false);
        };
        let is_active = connection_is_live(&state.read().await.status);
        Ok(is_active)
    }

    /// A non-running handle for handler-only tests. Queue mutations remain
    /// durable; they simply do not dispatch until a real runtime is installed.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn disconnected_for_test() -> Self {
        let (wake_tx, wake_rx) = mpsc::unbounded_channel();
        drop(wake_rx);
        Self {
            wake_tx,
            manager: ConnectionManager::new(),
        }
    }
}

pub(crate) async fn active_connection_for_row(
    manager: &ConnectionManager,
    row: &conversation::Model,
) -> Option<(String, Arc<tokio::sync::RwLock<crate::acp::SessionState>>)> {
    let id = if let Some(id) = manager.find_connection_by_conversation_id(row.id).await {
        Some(id)
    } else if let (Some(external_id), Some(agent_type)) = (
        row.external_id.as_deref(),
        AgentType::from_wire(&row.agent_type),
    ) {
        manager
            .find_connection_by_external_id(external_id, agent_type)
            .await
    } else {
        None
    }?;
    let state = manager.get_state(&id).await?;
    Some((id, state))
}

fn emit_snapshot(emitter: &EventEmitter, snapshot: PromptQueueSnapshot) {
    emit_event(emitter, PROMPT_QUEUE_CHANGED_EVENT, snapshot);
}

pub fn build_prompt_queue_runtime(
    db_conn: sea_orm::DatabaseConnection,
    manager: ConnectionManager,
    emitter: EventEmitter,
    bus: Arc<InternalEventBus>,
) -> (PromptQueueHandle, impl Future<Output = ()> + Send + 'static) {
    build_prompt_queue_runtime_inner(db_conn, manager, emitter, bus, None, None)
}

pub fn build_prompt_queue_runtime_with_dispatch(
    db_conn: sea_orm::DatabaseConnection,
    manager: ConnectionManager,
    emitter: EventEmitter,
    bus: Arc<InternalEventBus>,
    dispatch: SessionDispatchConfig,
) -> (PromptQueueHandle, impl Future<Output = ()> + Send + 'static) {
    build_prompt_queue_runtime_inner(db_conn, manager, emitter, bus, Some(dispatch), None)
}

#[cfg(any(test, feature = "test-utils"))]
pub fn build_prompt_queue_runtime_with_ensure_hook(
    db_conn: sea_orm::DatabaseConnection,
    manager: ConnectionManager,
    emitter: EventEmitter,
    bus: Arc<InternalEventBus>,
    hook: Arc<dyn Fn(i32) + Send + Sync + 'static>,
) -> (PromptQueueHandle, impl Future<Output = ()> + Send + 'static) {
    build_prompt_queue_runtime_inner(db_conn, manager, emitter, bus, None, Some(hook))
}

fn build_prompt_queue_runtime_inner(
    db_conn: sea_orm::DatabaseConnection,
    manager: ConnectionManager,
    emitter: EventEmitter,
    bus: Arc<InternalEventBus>,
    dispatch: Option<SessionDispatchConfig>,
    ensure_hook: Option<Arc<dyn Fn(i32) + Send + Sync + 'static>>,
) -> (PromptQueueHandle, impl Future<Output = ()> + Send + 'static) {
    // Subscribe before returning the future, matching the lifecycle subscriber:
    // events emitted between construction and spawn are buffered, not lost.
    let bus_rx = bus.subscribe();
    let (wake_tx, wake_rx) = mpsc::unbounded_channel();
    let handle = PromptQueueHandle {
        wake_tx,
        manager: manager.clone_ref(),
    };
    let runtime = PromptQueueRuntime {
        db: AppDatabase { conn: db_conn },
        manager,
        emitter,
        worker_id: format!("prompt-queue-{}", uuid::Uuid::new_v4()),
        bus,
        bus_rx,
        wake_rx,
        dispatch,
        ensure_hook,
        ensure_gate: EnsureGate::default(),
    };
    (handle, async move { runtime.run().await })
}

struct PromptQueueRuntime {
    db: AppDatabase,
    manager: ConnectionManager,
    emitter: EventEmitter,
    worker_id: String,
    bus: Arc<InternalEventBus>,
    bus_rx: broadcast::Receiver<Arc<crate::acp::EventEnvelope>>,
    wake_rx: mpsc::UnboundedReceiver<i32>,
    dispatch: Option<SessionDispatchConfig>,
    ensure_hook: Option<Arc<dyn Fn(i32) + Send + Sync + 'static>>,
    ensure_gate: EnsureGate,
}

impl PromptQueueRuntime {
    async fn emit_interrupt_change(&self, conversation_id: i32, event_id: &str) {
        match prompt_queue_service::snapshot(&self.db.conn, conversation_id).await {
            Ok(snapshot) => emit_snapshot(&self.emitter, snapshot),
            Err(err) => tracing::error!(
                "[prompt-queue] interrupt snapshot failed for {conversation_id}: {err}"
            ),
        }
        match collaboration_service::origin_participants(&self.db.conn, conversation_id, event_id)
            .await
        {
            Ok(conversation_ids) if !conversation_ids.is_empty() => emit_event(
                &self.emitter,
                COLLABORATION_CHANGED_EVENT,
                CollaborationChanged { conversation_ids },
            ),
            Ok(_) => {}
            Err(err) => tracing::error!(
                "[prompt-queue] interrupt collaboration invalidation failed for {event_id}: {err}"
            ),
        }
    }

    async fn reconcile_interrupt_terminal(&self, conversation_id: i32) {
        match collaboration_interrupt_service::observe_terminal(&self.db.conn, conversation_id)
            .await
        {
            Ok(Some(observation)) => {
                self.emit_interrupt_change(conversation_id, &observation.event_id)
                    .await;
            }
            Ok(None) => {}
            Err(err) => tracing::error!(
                "[prompt-queue] interrupt terminal reconciliation failed for {conversation_id}: {err}"
            ),
        }
    }

    async fn auto_reply_completed_collaboration_turn(
        &self,
        conversation_id: i32,
        completed_message_id: Option<String>,
        assistant_text: Option<String>,
        ended_abnormally: bool,
    ) {
        if ended_abnormally {
            return;
        }
        let (Some(completed_message_id), Some(assistant_text)) =
            (completed_message_id, assistant_text)
        else {
            return;
        };
        match collaboration_service::auto_reply_for_completed_turn(
            &self.db.conn,
            conversation_id,
            &completed_message_id,
            &assistant_text,
        )
        .await
        {
            Ok(Some(reply)) => emit_event(
                &self.emitter,
                COLLABORATION_CHANGED_EVENT,
                CollaborationChanged {
                    conversation_ids: reply.affected_conversation_ids,
                },
            ),
            Ok(None) => {}
            Err(err) => tracing::error!(
                "[prompt-queue] automatic collaboration reply failed for {conversation_id}: {err}"
            ),
        }
    }

    async fn emit_origin_change(&self, item: &crate::models::prompt_queue::ClaimedPromptQueueItem) {
        let Some(event_id) = item.origin_event_id.as_deref() else {
            return;
        };
        match collaboration_service::origin_participants(
            &self.db.conn,
            item.conversation_id,
            event_id,
        )
        .await
        {
            Ok(conversation_ids) if !conversation_ids.is_empty() => emit_event(
                &self.emitter,
                COLLABORATION_CHANGED_EVENT,
                CollaborationChanged { conversation_ids },
            ),
            Ok(_) => {}
            Err(err) => tracing::error!(
                "[prompt-queue] collaboration invalidation failed for {event_id}: {err}"
            ),
        }
    }

    async fn run(mut self) {
        self.recover_and_scan().await;
        let mut sweep = tokio::time::interval(StdDuration::from_secs(LEASE_SWEEP_SECS));
        sweep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                wake = self.wake_rx.recv() => {
                    let Some(conversation_id) = wake else { break; };
                    self.process(conversation_id).await;
                }
                event = self.bus_rx.recv() => {
                    match event {
                        Ok(event) => self.on_acp_event(&event).await,
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            self.bus.metrics().lagged_count.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
                            self.recover_and_scan().await;
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = sweep.tick() => self.recover_and_scan().await,
            }
        }
    }

    async fn reconcile_collaboration_policy(&self) -> bool {
        let enabled =
            match prompt_queue_service::collaboration_dispatch_enabled(&self.db.conn).await {
                Ok(enabled) => enabled,
                Err(err) => {
                    tracing::error!(
                        "[prompt-queue] could not read Session collaboration policy: {err}"
                    );
                    return false;
                }
            };
        match prompt_queue_service::reconcile_collaboration_dispatch_policy(&self.db.conn, enabled)
            .await
        {
            Ok(changes) => {
                for change in changes {
                    let conversation_id = change.snapshot.conversation_id;
                    emit_snapshot(&self.emitter, change.snapshot);
                    let mut conversation_ids = std::collections::BTreeSet::new();
                    for event_id in change.origin_event_ids {
                        match collaboration_service::origin_participants(
                            &self.db.conn,
                            conversation_id,
                            &event_id,
                        )
                        .await
                        {
                            Ok(ids) => conversation_ids.extend(ids),
                            Err(err) => tracing::error!(
                                "[prompt-queue] policy invalidation failed for {event_id}: {err}"
                            ),
                        }
                    }
                    if !conversation_ids.is_empty() {
                        emit_event(
                            &self.emitter,
                            COLLABORATION_CHANGED_EVENT,
                            CollaborationChanged {
                                conversation_ids: conversation_ids.into_iter().collect(),
                            },
                        );
                    }
                }
            }
            Err(err) => {
                tracing::error!("[prompt-queue] collaboration policy reconciliation failed: {err}")
            }
        }
        enabled
    }

    async fn freeze_claim_if_collaboration_disabled(
        &self,
        item: &crate::models::prompt_queue::ClaimedPromptQueueItem,
    ) -> bool {
        if item.origin_event_id.is_none() {
            return false;
        }
        match prompt_queue_service::collaboration_dispatch_enabled(&self.db.conn).await {
            Ok(true) => false,
            Ok(false) => {
                let _ = self.reconcile_collaboration_policy().await;
                true
            }
            Err(err) => {
                tracing::error!(
                    "[prompt-queue] could not verify collaboration policy for {}: {err}",
                    item.id
                );
                // Fail closed for cross-Session work. The claim lease can be
                // recovered after the policy store becomes readable again;
                // sending despite an unknown operator policy cannot be undone.
                true
            }
        }
    }

    async fn recover_and_scan(&self) {
        self.reconcile_collaboration_policy().await;
        match collaboration_interrupt_service::recover_after_process_loss(
            &self.db.conn,
            chrono::Utc::now() - Duration::seconds(60),
        )
        .await
        {
            Ok(recoveries) => {
                for recovery in recoveries {
                    self.emit_interrupt_change(recovery.target_conversation_id, &recovery.event_id)
                        .await;
                }
            }
            Err(err) => tracing::error!("[prompt-queue] interrupt recovery failed: {err}"),
        }
        match collaboration_interrupt_service::waiting_target_ids(&self.db.conn).await {
            Ok(ids) => {
                for id in ids {
                    // A TurnComplete can be dropped while the Session state has
                    // already become idle. `process` rechecks that authoritative
                    // runtime flag and releases only the matching owned pause.
                    self.process(id).await;
                }
            }
            Err(err) => {
                tracing::error!("[prompt-queue] waiting interrupt reconciliation failed: {err}")
            }
        }
        match prompt_queue_service::recover_expired_claims(&self.db.conn).await {
            Ok(snapshots) => {
                for snapshot in snapshots {
                    let recovered_origins = snapshot
                        .items
                        .iter()
                        .filter_map(|item| item.origin_event_id.as_deref())
                        .collect::<Vec<_>>();
                    for event_id in recovered_origins {
                        match collaboration_service::origin_participants(
                            &self.db.conn,
                            snapshot.conversation_id,
                            event_id,
                        )
                        .await
                        {
                            Ok(conversation_ids) if !conversation_ids.is_empty() => emit_event(
                                &self.emitter,
                                COLLABORATION_CHANGED_EVENT,
                                CollaborationChanged { conversation_ids },
                            ),
                            Ok(_) => {}
                            Err(err) => tracing::error!(
                                "[prompt-queue] recovery invalidation failed for {event_id}: {err}"
                            ),
                        }
                    }
                    emit_snapshot(&self.emitter, snapshot);
                }
            }
            Err(err) => tracing::error!("[prompt-queue] lease recovery failed: {err}"),
        }
        match prompt_queue_service::pending_conversation_ids(&self.db.conn).await {
            Ok(ids) => {
                for id in ids {
                    self.process(id).await;
                }
            }
            Err(err) => tracing::error!("[prompt-queue] pending scan failed: {err}"),
        }
    }

    async fn on_acp_event(&self, event: &crate::acp::EventEnvelope) {
        let turn_completed = matches!(&event.payload, AcpEvent::TurnComplete { .. });
        let explicit = match &event.payload {
            AcpEvent::ConversationLinked {
                conversation_id, ..
            }
            | AcpEvent::ConversationStatusChanged {
                conversation_id, ..
            } => Some(*conversation_id),
            AcpEvent::TurnComplete { .. } | AcpEvent::SessionStarted { .. } => None,
            _ => return,
        };
        let mut completed_turn = None;
        let conversation_id = match explicit {
            Some(id) => Some(id),
            None => match self.manager.get_state(&event.connection_id).await {
                Some(state) => {
                    let state = state.read().await;
                    if turn_completed {
                        completed_turn = Some((
                            state.last_completed_user_message_id.clone(),
                            state.last_assistant_text.clone(),
                            state.last_turn_ended_abnormally,
                        ));
                    }
                    match state.conversation_id {
                        Some(id) => Some(id),
                        None => self.conversation_id_from_external(&state).await,
                    }
                }
                None => None,
            },
        };
        if let Some(id) = conversation_id {
            if turn_completed {
                if let Some((message_id, assistant_text, ended_abnormally)) = completed_turn {
                    self.auto_reply_completed_collaboration_turn(
                        id,
                        message_id,
                        assistant_text,
                        ended_abnormally,
                    )
                    .await;
                }
                self.reconcile_interrupt_terminal(id).await;
            }
            self.process(id).await;
        }
    }

    async fn active_connection(
        &self,
        row: &conversation::Model,
    ) -> Option<(String, Arc<tokio::sync::RwLock<crate::acp::SessionState>>)> {
        active_connection_for_row(&self.manager, row).await
    }

    async fn request_ensure_runtime(&self, row: &conversation::Model) {
        if self.dispatch.is_none() && self.ensure_hook.is_none() {
            return;
        }
        if !conversation_can_auto_start(row) {
            return;
        }
        match prompt_queue_service::has_queued_items(&self.db.conn, row.id).await {
            Ok(true) => {}
            Ok(false) => return,
            Err(err) => {
                tracing::warn!(
                    "[session-dispatcher] queued-item check failed for {}: {err}",
                    row.id
                );
                return;
            }
        }
        emit_event(
            &self.emitter,
            COLLABORATION_CHANGED_EVENT,
            CollaborationChanged {
                conversation_ids: vec![row.id],
            },
        );
        if !self.ensure_gate.try_begin(row.id) {
            return;
        }
        if let Some(hook) = &self.ensure_hook {
            hook(row.id);
            return;
        }
        let Some(dispatch) = self.dispatch.clone() else {
            return;
        };
        let db = AppDatabase {
            conn: self.db.conn.clone(),
        };
        let manager = self.manager.clone_ref();
        let emitter = self.emitter.clone();
        let row = row.clone();
        tokio::spawn(async move {
            match crate::session_dispatcher::ensure_session_runtime(
                &db,
                &manager,
                &emitter,
                &dispatch.data_dir,
                &row,
            )
            .await
            {
                Ok(outcome) => tracing::info!(
                    "[session-dispatcher] Session {} runtime {:?}",
                    row.id,
                    outcome
                ),
                Err(err) => tracing::warn!(
                    "[session-dispatcher] Session {} start failed: {err}",
                    row.id
                ),
            }
        });
    }

    async fn process(&self, conversation_id: i32) {
        let Some(row) = (match conversation::Entity::find_by_id(conversation_id)
            .one(&self.db.conn)
            .await
        {
            Ok(row) => row,
            Err(err) => {
                tracing::error!("[prompt-queue] conversation lookup failed: {err}");
                return;
            }
        }) else {
            return;
        };
        if row.deleted_at.is_some() {
            return;
        }
        let live = match self.active_connection(&row).await {
            Some((connection_id, state)) => {
                let status = state.read().await.status.clone();
                if connection_is_live(&status) {
                    Some((connection_id, state, status))
                } else {
                    None
                }
            }
            None => None,
        };
        let Some((connection_id, state, status)) = live else {
            self.request_ensure_runtime(&row).await;
            return;
        };
        let (turn_in_flight, native_steering_available) = {
            let state = state.read().await;
            (state.turn_in_flight, state.native_steering_available)
        };
        if status != ConnectionStatus::Connected {
            return;
        }
        if !turn_in_flight {
            // Covers a dropped TurnComplete and the first idle SessionStarted
            // after an application restart. The operation itself proves that
            // cancellation was previously requested; an idle runtime proves
            // the old turn is no longer executing.
            self.reconcile_interrupt_terminal(conversation_id).await;
        }
        if turn_in_flight {
            if native_steering_available {
                self.process_native_steer(&row, &connection_id).await;
            }
            return;
        }

        let claimed = match prompt_queue_service::claim_head(
            &self.db.conn,
            conversation_id,
            &self.worker_id,
            Duration::seconds(CLAIM_LEASE_SECS),
        )
        .await
        {
            Ok(Some((item, snapshot))) => {
                emit_snapshot(&self.emitter, snapshot);
                item
            }
            Ok(None) => {
                return;
            }
            Err(err) => {
                tracing::error!("[prompt-queue] claim failed for {conversation_id}: {err}");
                return;
            }
        };

        if self.freeze_claim_if_collaboration_disabled(&claimed).await {
            return;
        }

        // Re-check after the durable claim: a competing client may have begun a
        // turn in the small window between the idle snapshot and the claim.
        let (status, turn_in_flight, actual_cwd) =
            match self.manager.get_state(&connection_id).await {
                Some(state) => {
                    let state = state.read().await;
                    (
                        state.status.clone(),
                        state.turn_in_flight,
                        state
                            .working_dir
                            .as_ref()
                            .map(|path| path.to_string_lossy().into_owned()),
                    )
                }
                None => {
                    self.release_busy(&claimed).await;
                    return;
                }
            };
        if status != ConnectionStatus::Connected || turn_in_flight {
            self.release_busy(&claimed).await;
            return;
        }

        let expected_cwd = if let Some(origin) = row.origin_cwd.clone() {
            Some(origin)
        } else {
            match folder::Entity::find_by_id(row.folder_id)
                .one(&self.db.conn)
                .await
            {
                Ok(folder) => folder.map(|folder| folder.path),
                Err(err) => {
                    self.fail(
                        &claimed,
                        &format!("Could not resolve Session working directory: {err}"),
                    )
                    .await;
                    return;
                }
            }
        };
        let cwd_matches = match (actual_cwd.as_deref(), expected_cwd.as_deref()) {
            (Some(actual), Some(expected)) => path_eq_for_matching(actual, expected),
            (None, None) => true,
            _ => false,
        };
        if !cwd_matches {
            self.fail(
                &claimed,
                "The active Harness is running in a different working directory",
            )
            .await;
            return;
        }

        if let Some(mode_id) = claimed.mode_id.clone() {
            if let Err(err) = self.manager.set_mode(&connection_id, mode_id).await {
                self.fail(&claimed, &format!("Could not apply queued mode: {err}"))
                    .await;
                return;
            }
        }

        match prompt_queue_service::mark_dispatch_started(
            &self.db.conn,
            &claimed,
            Duration::seconds(DISPATCH_LEASE_SECS),
        )
        .await
        {
            Ok(true) => self.emit_origin_change(&claimed).await,
            Ok(false) => {
                if self.freeze_claim_if_collaboration_disabled(&claimed).await {
                    return;
                }
                tracing::warn!(
                    "[prompt-queue] claim {} expired before dispatch; prompt was not sent",
                    claimed.id
                );
                self.release_busy(&claimed).await;
                return;
            }
            Err(err) => {
                tracing::error!(
                    "[prompt-queue] could not mark dispatch for {}: {err}",
                    claimed.id
                );
                self.release_busy(&claimed).await;
                return;
            }
        }

        let outcome = self
            .manager
            .send_prompt_linked_with_message_id(
                &self.db,
                &connection_id,
                claimed.draft.blocks.clone(),
                Some(row.folder_id),
                Some(row.id),
                Some(claimed.id.clone()),
                claimed.origin_event_id.is_some(),
            )
            .await;
        match outcome {
            Ok(_) => self.accept_after_dispatch(&claimed).await,
            Err(AcpError::TurnInProgress) => self.release_busy(&claimed).await,
            Err(AcpError::DispatchUncertain) => {
                match prompt_queue_service::pause_dispatch_unknown(&self.db.conn, &claimed).await {
                    Ok(snapshot) => {
                        emit_snapshot(&self.emitter, snapshot);
                        self.emit_origin_change(&claimed).await;
                    }
                    Err(err) => tracing::error!(
                        "[prompt-queue] could not pause uncertain prompt {}: {err}",
                        claimed.id
                    ),
                }
            }
            Err(err) => self.fail(&claimed, &err.to_string()).await,
        }
    }

    async fn conversation_id_from_external(&self, state: &crate::acp::SessionState) -> Option<i32> {
        let external_id = state.external_id.as_deref()?;
        conversation::Entity::find()
            .filter(conversation::Column::ExternalId.eq(external_id))
            .filter(conversation::Column::AgentType.eq(state.agent_type.as_wire().as_ref()))
            .filter(conversation::Column::DeletedAt.is_null())
            .one(&self.db.conn)
            .await
            .ok()
            .flatten()
            .map(|row| row.id)
    }

    async fn process_native_steer(&self, row: &conversation::Model, connection_id: &str) {
        // A busy Session cannot run the class-ordered head, but a native steer
        // does not consume the turn slot, so the claim skips directly to the
        // first letter whose sender asked for it. Queued user drafts are not
        // bypassed for a turn: they still own the next idle dispatch.
        let claimed = match prompt_queue_service::claim_first_steerable(
            &self.db.conn,
            row.id,
            &self.worker_id,
            Duration::seconds(CLAIM_LEASE_SECS),
        )
        .await
        {
            Ok(Some((item, snapshot))) => {
                emit_snapshot(&self.emitter, snapshot);
                item
            }
            Ok(None) => return,
            Err(err) => {
                tracing::error!("[prompt-queue] steer claim failed for {}: {err}", row.id);
                return;
            }
        };
        if claimed.origin_event_id.is_none() {
            self.release_busy(&claimed).await;
            return;
        }
        if claimed.delivery_hint != Some(CollaborationDeliveryHint::SteerIfSupported) {
            self.release_busy(&claimed).await;
            return;
        }
        if self.freeze_claim_if_collaboration_disabled(&claimed).await {
            return;
        }

        let (status, turn_in_flight, native_steering_available, actual_cwd) =
            match self.manager.get_state(connection_id).await {
                Some(state) => {
                    let state = state.read().await;
                    (
                        state.status.clone(),
                        state.turn_in_flight,
                        state.native_steering_available,
                        state
                            .working_dir
                            .as_ref()
                            .map(|path| path.to_string_lossy().into_owned()),
                    )
                }
                None => {
                    self.release_busy(&claimed).await;
                    return;
                }
            };
        if status != ConnectionStatus::Connected || !turn_in_flight || !native_steering_available {
            self.release_busy(&claimed).await;
            return;
        }

        let expected_cwd = if let Some(origin) = row.origin_cwd.clone() {
            Some(origin)
        } else {
            match folder::Entity::find_by_id(row.folder_id)
                .one(&self.db.conn)
                .await
            {
                Ok(folder) => folder.map(|folder| folder.path),
                Err(err) => {
                    self.fail(
                        &claimed,
                        &format!("Could not resolve Session working directory: {err}"),
                    )
                    .await;
                    return;
                }
            }
        };
        let cwd_matches = match (actual_cwd.as_deref(), expected_cwd.as_deref()) {
            (Some(actual), Some(expected)) => path_eq_for_matching(actual, expected),
            (None, None) => true,
            _ => false,
        };
        if !cwd_matches {
            self.fail(
                &claimed,
                "The active Harness is running in a different working directory",
            )
            .await;
            return;
        }

        let steer_text = match claimed.draft.blocks.as_slice() {
            [crate::acp::types::PromptInputBlock::Text { text }] => text.clone(),
            _ => {
                self.release_busy(&claimed).await;
                return;
            }
        };
        match prompt_queue_service::mark_dispatch_started(
            &self.db.conn,
            &claimed,
            Duration::seconds(DISPATCH_LEASE_SECS),
        )
        .await
        {
            Ok(true) => self.emit_origin_change(&claimed).await,
            Ok(false) => {
                if !self.freeze_claim_if_collaboration_disabled(&claimed).await {
                    self.release_busy(&claimed).await;
                }
                return;
            }
            Err(err) => {
                tracing::error!(
                    "[prompt-queue] could not mark steer dispatch for {}: {err}",
                    claimed.id
                );
                self.release_busy(&claimed).await;
                return;
            }
        }

        match self
            .manager
            .try_submit_native_feedback(connection_id, steer_text)
            .await
        {
            Ok(Some(_)) => self.accept_after_dispatch(&claimed).await,
            Ok(None) => self.release_busy(&claimed).await,
            Err(err) => {
                tracing::error!(
                    "[prompt-queue] native steer outcome is uncertain for {}: {err}",
                    claimed.id
                );
                match prompt_queue_service::pause_dispatch_unknown(&self.db.conn, &claimed).await {
                    Ok(snapshot) => {
                        emit_snapshot(&self.emitter, snapshot);
                        self.emit_origin_change(&claimed).await;
                    }
                    Err(pause_err) => tracing::error!(
                        "[prompt-queue] could not pause uncertain steer {}: {pause_err}",
                        claimed.id
                    ),
                }
            }
        }
    }

    async fn accept_after_dispatch(
        &self,
        item: &crate::models::prompt_queue::ClaimedPromptQueueItem,
    ) {
        let mut last_error = None;
        for delay_ms in ACCEPT_RETRY_DELAYS_MS {
            match prompt_queue_service::accept_claim(&self.db.conn, item).await {
                Ok(snapshot) => {
                    emit_snapshot(&self.emitter, snapshot);
                    self.emit_origin_change(item).await;
                    return;
                }
                Err(err) => {
                    last_error = Some(err);
                    tokio::time::sleep(StdDuration::from_millis(delay_ms)).await;
                }
            }
        }
        tracing::error!(
            "[prompt-queue] Harness accepted {}, but queue acknowledgement failed after retries: {}",
            item.id,
            last_error
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "unknown database error".to_string())
        );
        match prompt_queue_service::pause_dispatch_unknown(&self.db.conn, item).await {
            Ok(snapshot) => {
                emit_snapshot(&self.emitter, snapshot);
                self.emit_origin_change(item).await;
            }
            Err(err) => tracing::error!(
                "[prompt-queue] could not persist unknown dispatch state for {}: {err}",
                item.id
            ),
        }
    }

    async fn release_busy(&self, item: &crate::models::prompt_queue::ClaimedPromptQueueItem) {
        match prompt_queue_service::release_claim_busy(&self.db.conn, item).await {
            Ok(snapshot) => {
                emit_snapshot(&self.emitter, snapshot);
                self.emit_origin_change(item).await;
            }
            Err(err) => tracing::error!("[prompt-queue] busy release failed: {err}"),
        }
    }

    async fn fail(&self, item: &crate::models::prompt_queue::ClaimedPromptQueueItem, reason: &str) {
        match prompt_queue_service::fail_claim(&self.db.conn, item, reason).await {
            Ok(snapshot) => {
                emit_snapshot(&self.emitter, snapshot);
                self.emit_origin_change(item).await;
            }
            Err(err) => tracing::error!("[prompt-queue] pause failed: {err}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::connection::{ConnectionCommand, SteerOutcome};
    use crate::acp::internal_bus::EventBusMetrics;
    use crate::acp::types::{EventEnvelope, PromptInputBlock};
    use crate::db::service::{
        collaboration_interrupt_service, collaboration_service, prompt_queue_service,
    };
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::{
        CollaborationDeliveryHint, CollaborationDeliveryState, CollaborationInterruptState,
        CollaborationInvocationPolicy, CollaborationUrgency, EnqueuePromptQueueItem,
        InterruptCollaborationInput, PromptQueueDraft, PromptQueueItemState,
        SendCollaborationMessageInput,
    };
    use std::path::PathBuf;

    fn input(conversation_id: i32, id: &str, text: &str) -> EnqueuePromptQueueItem {
        EnqueuePromptQueueItem {
            conversation_id,
            id: id.to_string(),
            client_dedupe_id: id.to_string(),
            draft: PromptQueueDraft {
                blocks: vec![PromptInputBlock::Text {
                    text: text.to_string(),
                }],
                display_text: text.to_string(),
            },
            mode_id: None,
            source: crate::models::PromptQueueSource::User,
        }
    }

    fn collaboration_input(
        source_conversation_id: i32,
        target_conversation_id: i32,
        dedupe: &str,
        body: &str,
    ) -> SendCollaborationMessageInput {
        SendCollaborationMessageInput {
            source_conversation_id,
            target_conversation_ids: vec![target_conversation_id],
            subject: "Test letter".into(),
            body: body.to_string(),
            client_dedupe_id: dedupe.to_string(),
            invocation_policy: CollaborationInvocationPolicy::InvokeWhenIdle,
            delivery_hint: CollaborationDeliveryHint::Default,
            expects_reply: false,
            urgency: CollaborationUrgency::Normal,
            reply_to_event_id: None,
        }
    }

    async fn wait_until<F, Fut>(mut predicate: F)
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = bool>,
    {
        tokio::time::timeout(StdDuration::from_secs(2), async {
            loop {
                if predicate().await {
                    return;
                }
                tokio::time::sleep(StdDuration::from_millis(10)).await;
            }
        })
        .await
        .expect("condition did not converge");
    }

    async fn setup(
        path: &str,
    ) -> (
        crate::db::AppDatabase,
        i32,
        i32,
        ConnectionManager,
        Arc<InternalEventBus>,
    ) {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, path).await;
        let conversation_id = seed_conversation(&db, folder_id, AgentType::Codex).await;
        let manager = ConnectionManager::new();
        let bus = Arc::new(InternalEventBus::new(Arc::new(EventBusMetrics::default())));
        (db, folder_id, conversation_id, manager, bus)
    }

    async fn bind_live_connection(
        manager: &ConnectionManager,
        connection_id: &str,
        working_dir: &str,
        folder_id: i32,
        conversation_id: i32,
    ) -> tokio::sync::mpsc::Receiver<ConnectionCommand> {
        let rx = manager
            .insert_test_connection_live(
                connection_id,
                AgentType::Codex,
                Some(PathBuf::from(working_dir)),
                EventEmitter::Noop,
            )
            .await;
        let state = manager
            .get_state(connection_id)
            .await
            .expect("connection state");
        let mut state = state.write().await;
        state.folder_id = Some(folder_id);
        state.conversation_id = Some(conversation_id);
        drop(state);
        rx
    }

    #[tokio::test]
    async fn closed_session_stays_queued_without_dispatcher() {
        let (db, _, conversation_id, manager, bus) = setup("/tmp/codeg-queue-no-cold-start").await;
        let (handle, task) = build_prompt_queue_runtime(
            db.conn.clone(),
            manager.clone_ref(),
            EventEmitter::Noop,
            bus,
        );
        let worker = tokio::spawn(task);
        prompt_queue_service::enqueue(&db.conn, input(conversation_id, "one", "wait"))
            .await
            .expect("enqueue");
        handle.wake(conversation_id);
        tokio::time::sleep(StdDuration::from_millis(80)).await;

        let snapshot = prompt_queue_service::snapshot(&db.conn, conversation_id)
            .await
            .expect("snapshot");
        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(snapshot.items[0].state, PromptQueueItemState::Queued);
        assert!(manager.list_connections().await.is_empty());
        worker.abort();
    }

    #[tokio::test]
    async fn closed_session_requests_runtime_start_so_mail_can_arrive() {
        let (db, _, conversation_id, manager, bus) =
            setup("/tmp/codeg-queue-dispatcher-start").await;
        let requested = Arc::new(std::sync::Mutex::new(Vec::new()));
        let hook_requested = requested.clone();
        let (handle, task) = build_prompt_queue_runtime_with_ensure_hook(
            db.conn.clone(),
            manager.clone_ref(),
            EventEmitter::Noop,
            bus,
            Arc::new(move |id| hook_requested.lock().unwrap().push(id)),
        );
        let worker = tokio::spawn(task);
        prompt_queue_service::enqueue(&db.conn, input(conversation_id, "one", "wait"))
            .await
            .expect("enqueue");
        handle.wake(conversation_id);
        tokio::time::sleep(StdDuration::from_millis(80)).await;

        assert_eq!(requested.lock().unwrap().as_slice(), [conversation_id]);
        let snapshot = prompt_queue_service::snapshot(&db.conn, conversation_id)
            .await
            .expect("snapshot");
        assert_eq!(snapshot.items[0].state, PromptQueueItemState::Queued);
        worker.abort();
    }

    #[tokio::test]
    async fn deliver_only_does_not_start_an_idle_turn() {
        let path = "/tmp/codeg-session-message-idle-inject";
        let (db, folder_id, target, manager, bus) = setup(path).await;
        let source = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let mut commands = bind_live_connection(&manager, "active", path, folder_id, target).await;
        collaboration_service::send(
            &db.conn,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                subject: "Test letter".into(),
                body: "PING idle".into(),
                client_dedupe_id: "idle-inject".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .expect("persist store_only");
        let (_handle, task) = build_prompt_queue_runtime(
            db.conn.clone(),
            manager.clone_ref(),
            EventEmitter::Noop,
            bus,
        );
        drop(task);
        assert!(commands.try_recv().is_err());
        let inbound = collaboration_service::feed(&db.conn, target, None)
            .await
            .unwrap()
            .inbound
            .remove(0);
        assert_eq!(inbound.state, CollaborationDeliveryState::Pending);
        assert!(inbound.agent_received_at.is_none());
    }

    #[tokio::test]
    async fn deliver_only_does_not_steer_a_busy_native_turn() {
        let path = "/tmp/codeg-session-message-busy-steer";
        let (db, folder_id, target, manager, bus) = setup(path).await;
        let source = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let mut commands = bind_live_connection(&manager, "active", path, folder_id, target).await;
        let state = manager.get_state("active").await.expect("state");
        {
            let mut state = state.write().await;
            state.turn_in_flight = true;
            state.native_steering_available = true;
        }
        collaboration_service::send(
            &db.conn,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                subject: "Test letter".into(),
                body: "PING steer".into(),
                client_dedupe_id: "busy-steer".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .expect("persist store_only");
        let (_handle, task) = build_prompt_queue_runtime(
            db.conn.clone(),
            manager.clone_ref(),
            EventEmitter::Noop,
            bus,
        );
        drop(task);
        assert!(commands.try_recv().is_err());
        let inbound = collaboration_service::feed(&db.conn, target, None)
            .await
            .unwrap()
            .inbound
            .remove(0);
        assert_eq!(inbound.state, CollaborationDeliveryState::Pending);
        assert!(inbound.agent_received_at.is_none());
    }

    #[tokio::test]
    async fn store_only_session_message_parks_when_busy_without_steer() {
        let path = "/tmp/codeg-session-message-busy-park";
        let (db, folder_id, target, manager, bus) = setup(path).await;
        let source = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let mut commands = bind_live_connection(&manager, "active", path, folder_id, target).await;
        let state = manager.get_state("active").await.expect("state");
        {
            let mut state = state.write().await;
            state.turn_in_flight = true;
            state.native_steering_available = false;
        }
        collaboration_service::send(
            &db.conn,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                subject: "Test letter".into(),
                body: "PING park".into(),
                client_dedupe_id: "busy-park".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .expect("persist store_only");
        let (_handle, task) = build_prompt_queue_runtime(
            db.conn.clone(),
            manager.clone_ref(),
            EventEmitter::Noop,
            bus,
        );
        drop(task);
        assert!(
            commands.try_recv().is_err(),
            "busy-without-steer must not start or inject a turn"
        );
        let inbound = collaboration_service::feed(&db.conn, target, None)
            .await
            .unwrap()
            .inbound
            .remove(0);
        assert_eq!(inbound.state, CollaborationDeliveryState::Pending);
        assert!(inbound.agent_received_at.is_none());
    }

    #[tokio::test]
    async fn deliver_only_does_not_start_a_turn_after_turn_complete() {
        let path = "/tmp/codeg-session-message-idle-wakeup";
        let (db, folder_id, target, manager, bus) = setup(path).await;
        let source = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let mut commands = bind_live_connection(&manager, "active", path, folder_id, target).await;
        let state = manager.get_state("active").await.expect("state");
        {
            let mut state = state.write().await;
            state.turn_in_flight = true;
            state.native_steering_available = false;
        }
        collaboration_service::send(
            &db.conn,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                subject: "Test letter".into(),
                body: "PING after long task".into(),
                client_dedupe_id: "idle-wakeup".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .expect("persist store_only");
        let (_handle, task) = build_prompt_queue_runtime(
            db.conn.clone(),
            manager.clone_ref(),
            EventEmitter::Noop,
            bus.clone(),
        );
        let worker = tokio::spawn(task);
        assert!(
            commands.try_recv().is_err(),
            "busy-without-steer must park until the long turn ends"
        );

        state.write().await.turn_in_flight = false;
        bus.send(Arc::new(EventEnvelope {
            seq: 1,
            connection_id: "active".into(),
            payload: AcpEvent::TurnComplete {
                session_id: "native-session".into(),
                stop_reason: "end_turn".into(),
                agent_type: "codex".into(),
            },
        }));
        tokio::time::sleep(StdDuration::from_millis(80)).await;
        assert!(commands.try_recv().is_err());
        let inbound = collaboration_service::feed(&db.conn, target, None)
            .await
            .unwrap()
            .inbound
            .remove(0);
        assert_eq!(inbound.state, CollaborationDeliveryState::Pending);
        assert!(inbound.agent_received_at.is_none());
        worker.abort();
    }

    #[tokio::test]
    async fn send_core_starts_an_idle_turn_through_the_human_queue() {
        let path = "/tmp/codeg-session-message-send-core-idle";
        let (db, folder_id, target, manager, bus) = setup(path).await;
        let source = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let mut commands = bind_live_connection(&manager, "active", path, folder_id, target).await;
        let (handle, task) = build_prompt_queue_runtime(
            db.conn.clone(),
            manager.clone_ref(),
            EventEmitter::Noop,
            bus,
        );
        let worker = tokio::spawn(task);
        crate::commands::collaboration::collaboration_send_core(
            &db.conn,
            &EventEmitter::Noop,
            &handle,
            Some(&manager),
            collaboration_input(source, target, "send-core-idle", "PING queue idle"),
        )
        .await
        .expect("send_core");

        let command = tokio::time::timeout(StdDuration::from_secs(2), commands.recv())
            .await
            .expect("idle queue timeout")
            .expect("prompt command");
        let ConnectionCommand::Prompt {
            blocks,
            dispatch_ack,
            ..
        } = command
        else {
            panic!("idle Session message must start a turn");
        };
        let PromptInputBlock::Text { text } = &blocks[0] else {
            panic!("expected text envelope");
        };
        assert!(text.contains("PING queue idle"));
        assert!(text.contains("Call read_message"));
        dispatch_ack
            .expect("collaboration dispatch acknowledgement")
            .send(())
            .unwrap();
        wait_until(|| async {
            collaboration_service::feed(&db.conn, target, None)
                .await
                .is_ok_and(|feed| feed.inbound[0].state == CollaborationDeliveryState::Embedded)
        })
        .await;
        worker.abort();
    }

    #[tokio::test]
    async fn send_core_steers_a_busy_native_turn() {
        let path = "/tmp/codeg-session-message-send-core-steer";
        let (db, folder_id, target, manager, bus) = setup(path).await;
        let source = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let mut commands = bind_live_connection(&manager, "active", path, folder_id, target).await;
        let state = manager.get_state("active").await.expect("state");
        {
            let mut state = state.write().await;
            state.turn_in_flight = true;
            state.native_steering_available = true;
        }
        let (handle, task) = build_prompt_queue_runtime(
            db.conn.clone(),
            manager.clone_ref(),
            EventEmitter::Noop,
            bus,
        );
        let worker = tokio::spawn(task);
        let mut input = collaboration_input(source, target, "send-core-steer", "PING queue steer");
        input.delivery_hint = CollaborationDeliveryHint::SteerIfSupported;
        crate::commands::collaboration::collaboration_send_core(
            &db.conn,
            &EventEmitter::Noop,
            &handle,
            Some(&manager),
            input,
        )
        .await
        .expect("send_core");

        let command = tokio::time::timeout(StdDuration::from_secs(2), commands.recv())
            .await
            .expect("steer timeout")
            .expect("steer command");
        let ConnectionCommand::Steer { text, reply } = command else {
            panic!("busy Session message must steer");
        };
        assert!(text.contains("PING queue steer"));
        assert!(text.contains("Call read_message"));
        reply.send(Ok(SteerOutcome::Injected)).expect("steer reply");
        wait_until(|| async {
            collaboration_service::feed(&db.conn, target, None)
                .await
                .is_ok_and(|feed| feed.inbound[0].state == CollaborationDeliveryState::Embedded)
        })
        .await;
        worker.abort();
    }

    #[tokio::test]
    async fn explicit_queue_wake_does_not_convert_deliver_only_mail() {
        let path = "/tmp/codeg-session-message-no-replay";
        let (db, folder_id, target, manager, bus) = setup(path).await;
        let source = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let mut commands = bind_live_connection(&manager, "active", path, folder_id, target).await;
        let state = manager.get_state("active").await.expect("state");
        {
            let mut state = state.write().await;
            state.turn_in_flight = true;
            state.native_steering_available = true;
        }
        let sent = collaboration_service::send(
            &db.conn,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                subject: "Test letter".into(),
                body: "already consumed".into(),
                client_dedupe_id: "no-replay".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .expect("persist");
        let (handle, task) = build_prompt_queue_runtime(
            db.conn.clone(),
            manager.clone_ref(),
            EventEmitter::Noop,
            bus,
        );
        let worker = tokio::spawn(task);
        handle.wake(target);
        tokio::time::sleep(StdDuration::from_millis(80)).await;
        assert!(commands.try_recv().is_err());
        let inbound = collaboration_service::feed(&db.conn, target, None)
            .await
            .unwrap()
            .inbound
            .remove(0);
        assert_eq!(inbound.event_id, sent.event_id);
        assert_eq!(inbound.state, CollaborationDeliveryState::Pending);
        worker.abort();
    }

    #[tokio::test]
    async fn active_idle_session_accepts_once_and_removes_the_claim() {
        let path = "/tmp/codeg-queue-active";
        let (db, folder_id, conversation_id, manager, bus) = setup(path).await;
        let mut commands =
            bind_live_connection(&manager, "active", path, folder_id, conversation_id).await;
        prompt_queue_service::enqueue(&db.conn, input(conversation_id, "stable-id", "hello"))
            .await
            .expect("enqueue");
        let (_handle, task) =
            build_prompt_queue_runtime(db.conn.clone(), manager, EventEmitter::Noop, bus);
        let worker = tokio::spawn(task);

        let command = tokio::time::timeout(StdDuration::from_secs(2), commands.recv())
            .await
            .expect("prompt timeout")
            .expect("prompt command");
        let ConnectionCommand::Prompt {
            blocks,
            user_message,
            dispatch_ack,
        } = command
        else {
            panic!("expected prompt command");
        };
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            user_message.as_ref().map(|(id, _)| id.as_str()),
            Some("stable-id")
        );
        assert!(dispatch_ack.is_none());
        wait_until(|| async {
            prompt_queue_service::snapshot(&db.conn, conversation_id)
                .await
                .is_ok_and(|snapshot| snapshot.items.is_empty())
        })
        .await;
        assert!(
            commands.try_recv().is_err(),
            "the accepted item must not replay"
        );
        worker.abort();
    }

    #[tokio::test]
    async fn invoke_when_idle_enters_the_native_transcript_once_and_reaches_embedded() {
        let path = "/tmp/codeg-collaboration-invoke-active";
        let (db, folder_id, target, manager, bus) = setup(path).await;
        let source = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let mut commands = bind_live_connection(&manager, "active", path, folder_id, target).await;
        let sent = collaboration_service::send(
            &db.conn,
            collaboration_input(source, target, "invoke-once", "review this claim"),
        )
        .await
        .expect("collaboration send");
        let (_handle, task) =
            build_prompt_queue_runtime(db.conn.clone(), manager, EventEmitter::Noop, bus);
        let worker = tokio::spawn(task);

        let command = tokio::time::timeout(StdDuration::from_secs(2), commands.recv())
            .await
            .expect("prompt timeout")
            .expect("prompt command");
        let ConnectionCommand::Prompt {
            blocks,
            user_message,
            dispatch_ack,
        } = command
        else {
            panic!("expected prompt command");
        };
        let PromptInputBlock::Text { text } = &blocks[0] else {
            panic!("expected stable text envelope");
        };
        assert!(text.contains("mailbox letter"));
        assert!(text.contains("Call read_message"));
        assert!(text.contains("review this claim"));
        assert_eq!(
            user_message.as_ref().map(|(id, _)| id.as_str()),
            Some(sent.deliveries[0].id.as_str())
        );
        dispatch_ack
            .expect("collaboration dispatch acknowledgement")
            .send(())
            .unwrap();

        wait_until(|| async {
            collaboration_service::feed(&db.conn, target, None)
                .await
                .is_ok_and(|feed| {
                    feed.inbound[0].state == CollaborationDeliveryState::Embedded
                        && feed.inbound[0].attempts == 1
                })
        })
        .await;
        assert!(prompt_queue_service::snapshot(&db.conn, target)
            .await
            .unwrap()
            .items
            .is_empty());
        assert!(commands.try_recv().is_err(), "delivery must not replay");
        worker.abort();
    }

    #[tokio::test]
    async fn normal_collaboration_turn_returns_one_store_only_final_reply() {
        let path = "/tmp/codeg-collaboration-auto-reply";
        let (db, folder_id, target, manager, bus) = setup(path).await;
        let source = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let mut commands = bind_live_connection(&manager, "active", path, folder_id, target).await;
        let mut request =
            collaboration_input(source, target, "auto-reply-runtime", "review the result");
        request.expects_reply = true;
        let sent = collaboration_service::send(&db.conn, request)
            .await
            .expect("collaboration send");
        let (_handle, task) = build_prompt_queue_runtime(
            db.conn.clone(),
            manager.clone_ref(),
            EventEmitter::Noop,
            bus.clone(),
        );
        let worker = tokio::spawn(task);

        let command = tokio::time::timeout(StdDuration::from_secs(2), commands.recv())
            .await
            .expect("prompt timeout")
            .expect("prompt command");
        let ConnectionCommand::Prompt {
            user_message,
            dispatch_ack,
            ..
        } = command
        else {
            panic!("expected prompt command");
        };
        dispatch_ack
            .expect("collaboration dispatch acknowledgement")
            .send(())
            .unwrap();
        let completed_message_id = user_message
            .map(|(id, _)| id)
            .expect("stable collaboration message id");
        assert_eq!(completed_message_id, sent.deliveries[0].id);
        wait_until(|| async {
            collaboration_service::feed(&db.conn, target, None)
                .await
                .is_ok_and(|feed| feed.inbound[0].state == CollaborationDeliveryState::Embedded)
        })
        .await;
        collaboration_service::mark_agent_read(&db.conn, target, &sent.event_id)
            .await
            .unwrap();

        let state = manager.get_state("active").await.expect("state");
        {
            let mut state = state.write().await;
            state.turn_in_flight = false;
            state.last_completed_user_message_id = Some(completed_message_id);
            state.last_assistant_text = Some("The result is internally consistent.".into());
            state.last_turn_ended_abnormally = false;
        }
        bus.send(Arc::new(EventEnvelope {
            seq: 1,
            connection_id: "active".into(),
            payload: AcpEvent::TurnComplete {
                session_id: "native-session".into(),
                stop_reason: "end_turn".into(),
                agent_type: "codex".into(),
            },
        }));

        wait_until(|| async {
            collaboration_service::feed(&db.conn, source, None)
                .await
                .is_ok_and(|feed| {
                    feed.inbound.iter().any(|delivery| {
                        delivery.reply_to_event_id.as_deref() == Some(sent.event_id.as_str())
                            && delivery.body == "The result is internally consistent."
                            && delivery.invocation_policy
                                == CollaborationInvocationPolicy::StoreOnly
                            && !delivery.expects_reply
                    })
                })
        })
        .await;
        assert!(prompt_queue_service::snapshot(&db.conn, source)
            .await
            .unwrap()
            .items
            .is_empty());
        worker.abort();
    }

    #[tokio::test]
    async fn abnormal_collaboration_turn_does_not_auto_reply() {
        let path = "/tmp/codeg-collaboration-no-auto-reply";
        let (db, folder_id, target, manager, bus) = setup(path).await;
        let source = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let mut commands = bind_live_connection(&manager, "active", path, folder_id, target).await;
        let mut request =
            collaboration_input(source, target, "abnormal-auto-reply", "review the result");
        request.expects_reply = true;
        let sent = collaboration_service::send(&db.conn, request)
            .await
            .expect("collaboration send");
        let (_handle, task) = build_prompt_queue_runtime(
            db.conn.clone(),
            manager.clone_ref(),
            EventEmitter::Noop,
            bus.clone(),
        );
        let worker = tokio::spawn(task);
        let command = tokio::time::timeout(StdDuration::from_secs(2), commands.recv())
            .await
            .expect("prompt timeout")
            .expect("prompt command");
        let ConnectionCommand::Prompt {
            user_message,
            dispatch_ack,
            ..
        } = command
        else {
            panic!("expected prompt command");
        };
        dispatch_ack
            .expect("collaboration dispatch acknowledgement")
            .send(())
            .unwrap();
        wait_until(|| async {
            collaboration_service::feed(&db.conn, target, None)
                .await
                .is_ok_and(|feed| feed.inbound[0].state == CollaborationDeliveryState::Embedded)
        })
        .await;
        let state = manager.get_state("active").await.expect("state");
        {
            let mut state = state.write().await;
            state.turn_in_flight = false;
            state.last_completed_user_message_id = user_message.map(|(id, _)| id);
            state.last_assistant_text = Some("partial answer".into());
            state.last_turn_ended_abnormally = true;
        }
        bus.send(Arc::new(EventEnvelope {
            seq: 1,
            connection_id: "active".into(),
            payload: AcpEvent::TurnComplete {
                session_id: "native-session".into(),
                stop_reason: "cancelled".into(),
                agent_type: "codex".into(),
            },
        }));
        tokio::time::sleep(StdDuration::from_millis(100)).await;
        assert!(!collaboration_service::feed(&db.conn, source, None)
            .await
            .unwrap()
            .inbound
            .iter()
            .any(|delivery| delivery.reply_to_event_id.as_deref() == Some(sent.event_id.as_str())));
        worker.abort();
    }

    #[tokio::test]
    async fn invoke_when_idle_waits_while_busy_then_runs_on_turn_complete() {
        let path = "/tmp/codeg-collaboration-invoke-busy";
        let (db, folder_id, target, manager, bus) = setup(path).await;
        let source = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let mut commands = bind_live_connection(&manager, "active", path, folder_id, target).await;
        let state = manager.get_state("active").await.expect("state");
        state.write().await.turn_in_flight = true;
        collaboration_service::send(
            &db.conn,
            collaboration_input(source, target, "invoke-busy", "wait for idle"),
        )
        .await
        .expect("collaboration send");
        let (_handle, task) = build_prompt_queue_runtime(
            db.conn.clone(),
            manager.clone_ref(),
            EventEmitter::Noop,
            bus.clone(),
        );
        let worker = tokio::spawn(task);

        tokio::time::sleep(StdDuration::from_millis(80)).await;
        assert!(
            commands.try_recv().is_err(),
            "busy target must not be interrupted"
        );
        assert_eq!(
            collaboration_service::feed(&db.conn, target, None)
                .await
                .unwrap()
                .inbound[0]
                .state,
            CollaborationDeliveryState::Queued
        );

        state.write().await.turn_in_flight = false;
        bus.send(Arc::new(EventEnvelope {
            seq: 1,
            connection_id: "active".into(),
            payload: AcpEvent::TurnComplete {
                session_id: "native-session".into(),
                stop_reason: "end_turn".into(),
                agent_type: "codex".into(),
            },
        }));
        let command = tokio::time::timeout(StdDuration::from_secs(2), commands.recv())
            .await
            .expect("queued prompt timeout")
            .expect("queued prompt");
        let ConnectionCommand::Prompt { dispatch_ack, .. } = command else {
            panic!("expected queued prompt");
        };
        dispatch_ack
            .expect("collaboration dispatch acknowledgement")
            .send(())
            .unwrap();
        wait_until(|| async {
            collaboration_service::feed(&db.conn, target, None)
                .await
                .is_ok_and(|feed| feed.inbound[0].state == CollaborationDeliveryState::Embedded)
        })
        .await;
        worker.abort();
    }

    #[tokio::test]
    async fn explicit_interrupt_waits_for_terminal_before_dispatching_saved_message() {
        let path = "/tmp/codeg-collaboration-interrupt";
        let (db, folder_id, target, manager, bus) = setup(path).await;
        let source = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let mut commands = bind_live_connection(&manager, "active", path, folder_id, target).await;
        let state = manager.get_state("active").await.expect("state");
        state.write().await.turn_in_flight = true;
        let sent = collaboration_service::send(
            &db.conn,
            collaboration_input(source, target, "interrupt-message", "change direction now"),
        )
        .await
        .expect("collaboration send");
        let prepared = collaboration_interrupt_service::prepare(
            &db.conn,
            InterruptCollaborationInput {
                event_id: sent.event_id,
                target_conversation_id: target,
                client_dedupe_id: "interrupt-operation".into(),
                reason: "user requested stop and send".into(),
            },
            "active",
            true,
        )
        .await
        .expect("prepare interrupt");
        assert!(prepared.should_cancel);

        manager
            .cancel(&db.conn, "active")
            .await
            .expect("enqueue cancel");
        assert!(matches!(
            commands.recv().await,
            Some(ConnectionCommand::Cancel)
        ));
        let waiting =
            collaboration_interrupt_service::mark_cancel_enqueued(&db.conn, &prepared.operation.id)
                .await
                .expect("persist cancel acknowledgement");
        assert_eq!(
            waiting.state,
            CollaborationInterruptState::WaitingForTerminal
        );

        let (_handle, task) = build_prompt_queue_runtime(
            db.conn.clone(),
            manager.clone_ref(),
            EventEmitter::Noop,
            bus.clone(),
        );
        let worker = tokio::spawn(task);
        tokio::time::sleep(StdDuration::from_millis(80)).await;
        assert!(
            commands.try_recv().is_err(),
            "saved message must not dispatch before TurnComplete"
        );

        state.write().await.turn_in_flight = false;
        bus.send(Arc::new(EventEnvelope {
            seq: 1,
            connection_id: "active".into(),
            payload: AcpEvent::TurnComplete {
                session_id: "native-session".into(),
                stop_reason: "cancelled".into(),
                agent_type: "codex".into(),
            },
        }));
        let command = tokio::time::timeout(StdDuration::from_secs(2), commands.recv())
            .await
            .expect("next prompt timeout")
            .expect("next prompt");
        let ConnectionCommand::Prompt { dispatch_ack, .. } = command else {
            panic!("expected next prompt");
        };
        dispatch_ack
            .expect("collaboration dispatch acknowledgement")
            .send(())
            .unwrap();
        wait_until(|| async {
            collaboration_service::feed(&db.conn, target, None)
                .await
                .is_ok_and(|feed| {
                    feed.inbound[0].state == CollaborationDeliveryState::Embedded
                        && feed.inbound[0].interrupt_state
                            == Some(CollaborationInterruptState::Completed)
                })
        })
        .await;
        worker.abort();
    }

    #[tokio::test]
    async fn steer_hint_uses_proven_native_channel_and_consumes_queue_once() {
        let path = "/tmp/codeg-collaboration-native-steer";
        let (db, folder_id, target, manager, bus) = setup(path).await;
        let source = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let mut commands = bind_live_connection(&manager, "active", path, folder_id, target).await;
        let state = manager.get_state("active").await.expect("state");
        {
            let mut state = state.write().await;
            state.turn_in_flight = true;
            state.native_steering_available = true;
        }
        let mut input = collaboration_input(source, target, "native-steer", "correct the premise");
        input.delivery_hint = CollaborationDeliveryHint::SteerIfSupported;
        collaboration_service::send(&db.conn, input)
            .await
            .expect("collaboration send");
        let (_handle, task) =
            build_prompt_queue_runtime(db.conn.clone(), manager, EventEmitter::Noop, bus);
        let worker = tokio::spawn(task);

        let command = tokio::time::timeout(StdDuration::from_secs(2), commands.recv())
            .await
            .expect("steer timeout")
            .expect("steer command");
        let ConnectionCommand::Steer { text, reply } = command else {
            panic!("expected native steer command");
        };
        assert!(text.contains("mailbox letter"));
        assert!(text.contains("Call read_message"));
        assert!(text.contains("correct the premise"));
        reply.send(Ok(SteerOutcome::Injected)).expect("steer reply");

        wait_until(|| async {
            collaboration_service::feed(&db.conn, target, None)
                .await
                .is_ok_and(|feed| feed.inbound[0].state == CollaborationDeliveryState::Embedded)
        })
        .await;
        assert!(prompt_queue_service::snapshot(&db.conn, target)
            .await
            .unwrap()
            .items
            .is_empty());
        assert!(commands.try_recv().is_err(), "native steer must not replay");
        worker.abort();
    }

    #[tokio::test]
    async fn native_prompt_required_keeps_the_collaboration_message_queued() {
        let path = "/tmp/codeg-collaboration-steer-prompt-required";
        let (db, folder_id, target, manager, bus) = setup(path).await;
        let source = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let mut commands = bind_live_connection(&manager, "active", path, folder_id, target).await;
        let state = manager.get_state("active").await.expect("state");
        {
            let mut state = state.write().await;
            state.turn_in_flight = true;
            state.native_steering_available = true;
        }
        let mut input = collaboration_input(source, target, "steer-race", "keep ownership");
        input.delivery_hint = CollaborationDeliveryHint::SteerIfSupported;
        collaboration_service::send(&db.conn, input)
            .await
            .expect("collaboration send");
        let (_handle, task) =
            build_prompt_queue_runtime(db.conn.clone(), manager, EventEmitter::Noop, bus);
        let worker = tokio::spawn(task);

        let command = tokio::time::timeout(StdDuration::from_secs(2), commands.recv())
            .await
            .expect("steer timeout")
            .expect("steer command");
        let ConnectionCommand::Steer { reply, .. } = command else {
            panic!("expected native steer command");
        };
        reply
            .send(Ok(SteerOutcome::PromptRequired))
            .expect("steer reply");

        wait_until(|| async {
            prompt_queue_service::snapshot(&db.conn, target)
                .await
                .is_ok_and(|snapshot| {
                    snapshot.items.len() == 1
                        && snapshot.items[0].state == PromptQueueItemState::Queued
                })
        })
        .await;
        assert_eq!(
            collaboration_service::feed(&db.conn, target, None)
                .await
                .unwrap()
                .inbound[0]
                .state,
            CollaborationDeliveryState::Queued
        );
        worker.abort();
    }

    #[tokio::test]
    async fn uncertain_native_steer_pauses_instead_of_replaying() {
        let path = "/tmp/codeg-collaboration-steer-unknown";
        let (db, folder_id, target, manager, bus) = setup(path).await;
        let source = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let mut commands = bind_live_connection(&manager, "active", path, folder_id, target).await;
        let state = manager.get_state("active").await.expect("state");
        {
            let mut state = state.write().await;
            state.turn_in_flight = true;
            state.native_steering_available = true;
        }
        let mut input =
            collaboration_input(source, target, "steer-unknown", "never replay blindly");
        input.delivery_hint = CollaborationDeliveryHint::SteerIfSupported;
        collaboration_service::send(&db.conn, input)
            .await
            .expect("collaboration send");
        let (_handle, task) =
            build_prompt_queue_runtime(db.conn.clone(), manager, EventEmitter::Noop, bus);
        let worker = tokio::spawn(task);

        let command = tokio::time::timeout(StdDuration::from_secs(2), commands.recv())
            .await
            .expect("steer timeout")
            .expect("steer command");
        let ConnectionCommand::Steer { reply, .. } = command else {
            panic!("expected native steer command");
        };
        reply
            .send(Err(AcpError::protocol("steer response was lost")))
            .expect("steer reply");

        wait_until(|| async {
            prompt_queue_service::snapshot(&db.conn, target)
                .await
                .is_ok_and(|snapshot| {
                    snapshot.items.len() == 1
                        && snapshot.items[0].state == PromptQueueItemState::Paused
                        && snapshot.items[0].paused_reason.as_deref()
                            == Some("dispatch_outcome_unknown")
                })
        })
        .await;
        assert_eq!(
            collaboration_service::feed(&db.conn, target, None)
                .await
                .unwrap()
                .inbound[0]
                .state,
            CollaborationDeliveryState::Failed
        );
        assert!(
            commands.try_recv().is_err(),
            "uncertain steer must not replay"
        );
        worker.abort();
    }

    #[tokio::test]
    async fn unsupported_native_steer_stays_durably_queued_without_mcp_pull() {
        let path = "/tmp/codeg-collaboration-steer-fallback";
        let (db, folder_id, target, manager, bus) = setup(path).await;
        let source = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let mut commands = bind_live_connection(&manager, "active", path, folder_id, target).await;
        let state = manager.get_state("active").await.expect("state");
        {
            let mut state = state.write().await;
            state.turn_in_flight = true;
            state.native_steering_available = false;
            state.feedback_tool_available = true;
        }
        let mut input =
            collaboration_input(source, target, "steer-fallback", "do not fake a steer");
        input.delivery_hint = CollaborationDeliveryHint::SteerIfSupported;
        collaboration_service::send(&db.conn, input)
            .await
            .expect("collaboration send");
        let (_handle, task) =
            build_prompt_queue_runtime(db.conn.clone(), manager, EventEmitter::Noop, bus);
        let worker = tokio::spawn(task);

        tokio::time::sleep(StdDuration::from_millis(100)).await;
        assert!(
            commands.try_recv().is_err(),
            "MCP pull feedback must never impersonate native steering"
        );
        let queue = prompt_queue_service::snapshot(&db.conn, target)
            .await
            .expect("queue");
        assert_eq!(queue.items.len(), 1);
        assert_eq!(queue.items[0].state, PromptQueueItemState::Queued);
        assert_eq!(
            collaboration_service::feed(&db.conn, target, None)
                .await
                .unwrap()
                .inbound[0]
                .state,
            CollaborationDeliveryState::Queued
        );
        worker.abort();
    }

    #[tokio::test]
    async fn turn_complete_drains_the_next_item_but_busy_state_does_not() {
        let path = "/tmp/codeg-queue-turn-complete";
        let (db, folder_id, conversation_id, manager, bus) = setup(path).await;
        let mut commands =
            bind_live_connection(&manager, "active", path, folder_id, conversation_id).await;
        prompt_queue_service::enqueue(&db.conn, input(conversation_id, "one", "first"))
            .await
            .expect("one");
        prompt_queue_service::enqueue(&db.conn, input(conversation_id, "two", "second"))
            .await
            .expect("two");
        let (_handle, task) = build_prompt_queue_runtime(
            db.conn.clone(),
            manager.clone_ref(),
            EventEmitter::Noop,
            bus.clone(),
        );
        let worker = tokio::spawn(task);

        tokio::time::timeout(StdDuration::from_secs(2), commands.recv())
            .await
            .expect("first timeout")
            .expect("first prompt");
        tokio::time::sleep(StdDuration::from_millis(80)).await;
        assert!(
            commands.try_recv().is_err(),
            "busy Session must not receive item two"
        );

        let state = manager.get_state("active").await.expect("state");
        {
            let mut state = state.write().await;
            state.turn_in_flight = false;
            state.status = ConnectionStatus::Connected;
        }
        bus.send(Arc::new(EventEnvelope {
            seq: 1,
            connection_id: "active".into(),
            payload: AcpEvent::TurnComplete {
                session_id: "native-session".into(),
                stop_reason: "end_turn".into(),
                agent_type: "codex".into(),
            },
        }));
        let second = tokio::time::timeout(StdDuration::from_secs(2), commands.recv())
            .await
            .expect("second timeout")
            .expect("second prompt");
        let ConnectionCommand::Prompt { user_message, .. } = second else {
            panic!("expected second prompt");
        };
        assert_eq!(
            user_message.as_ref().map(|(id, _)| id.as_str()),
            Some("two")
        );
        worker.abort();
    }

    #[tokio::test]
    async fn cwd_mismatch_pauses_instead_of_sending_to_the_wrong_session() {
        let (db, folder_id, conversation_id, manager, bus) =
            setup("/tmp/codeg-queue-expected-cwd").await;
        let mut commands = bind_live_connection(
            &manager,
            "wrong-cwd",
            "/tmp/codeg-queue-other-cwd",
            folder_id,
            conversation_id,
        )
        .await;
        prompt_queue_service::enqueue(&db.conn, input(conversation_id, "one", "do not send"))
            .await
            .expect("enqueue");
        let (_handle, task) =
            build_prompt_queue_runtime(db.conn.clone(), manager, EventEmitter::Noop, bus);
        let worker = tokio::spawn(task);

        wait_until(|| async {
            prompt_queue_service::snapshot(&db.conn, conversation_id)
                .await
                .is_ok_and(|snapshot| snapshot.paused_reason.is_some())
        })
        .await;
        let snapshot = prompt_queue_service::snapshot(&db.conn, conversation_id)
            .await
            .expect("snapshot");
        assert_eq!(snapshot.items[0].state, PromptQueueItemState::Paused);
        assert!(
            snapshot
                .paused_reason
                .unwrap()
                .contains("working directory"),
            "pause should explain the safety boundary"
        );
        assert!(commands.try_recv().is_err());
        worker.abort();
    }

    #[tokio::test]
    async fn cancelling_a_turn_pauses_followups_before_they_can_continue() {
        let path = "/tmp/codeg-queue-cancel";
        let (db, folder_id, conversation_id, manager, _bus) = setup(path).await;
        let mut commands =
            bind_live_connection(&manager, "active", path, folder_id, conversation_id).await;
        prompt_queue_service::enqueue(&db.conn, input(conversation_id, "later", "wait"))
            .await
            .expect("enqueue");

        crate::commands::acp::acp_cancel_core(&db, &manager, &EventEmitter::Noop, "active")
            .await
            .expect("cancel");
        let command = commands.recv().await.expect("cancel command");
        assert!(matches!(command, ConnectionCommand::Cancel));
        let snapshot = prompt_queue_service::snapshot(&db.conn, conversation_id)
            .await
            .expect("snapshot");
        assert_eq!(
            snapshot.paused_reason.as_deref(),
            Some("cancelled_current_turn")
        );
        assert_eq!(snapshot.items[0].state, PromptQueueItemState::Queued);
    }
}
