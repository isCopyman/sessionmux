use std::path::PathBuf;
use std::sync::Arc;

use crate::acp::delegation::listener::TokenRegistry;
use crate::acp::manager::ConnectionManager;
use crate::acp::InternalEventBus;
use crate::chat_channel::manager::ChatChannelManager;
use crate::db::AppDatabase;
use crate::pet_state_mapper::PetStateHandle;
use crate::terminal::manager::TerminalManager;
use crate::web::event_bridge::{EventEmitter, WebEventBroadcaster};
use crate::web::WebServerState;
use crate::workspace_transfer::WorkspaceTransferManager;

pub struct AppState {
    pub db: AppDatabase,
    pub connection_manager: ConnectionManager,
    pub terminal_manager: TerminalManager,
    pub event_broadcaster: Arc<WebEventBroadcaster>,
    /// Process-wide bus for typed `Arc<EventEnvelope>` delivery to
    /// in-process consumers (lifecycle, pet state mapper, chat-channel
    /// subscribers). Distinct from `event_broadcaster`, which carries
    /// JSON-shaped `WebEvent`s for transport-bound delivery.
    pub acp_event_bus: Arc<InternalEventBus>,
    pub emitter: EventEmitter,
    /// Wake handle for the process-wide backend-authoritative Session
    /// follow-up queue. The worker itself is spawned once per backend process.
    pub prompt_queue: crate::prompt_queue::PromptQueueHandle,
    /// Wake handle for the Session Timer engine (RFC P2). CRUD commands wake
    /// it so a just-created due timer doesn't wait for the next sweep.
    pub session_timer: crate::session_timer::SessionTimerHandle,
    pub data_dir: PathBuf,
    pub web_server_state: WebServerState,
    pub chat_channel_manager: ChatChannelManager,
    pub workspace_transfer: Arc<WorkspaceTransferManager>,
    /// Latest ambient `PetState` written by `pet_state_subscriber_task`.
    /// Read by `pet_get_current_state` so a freshly-opened pet window can
    /// pick up the current state without waiting for the next transition.
    pub pet_state: PetStateHandle,
    /// Per-launch ephemeral tokens identifying parent ACP connections.
    /// Registered when `load_mcp_servers_for_agent` injects the
    /// `codeg-mcp` MCP entry, revoked on parent teardown.
    pub codeg_mcp_tokens: Arc<TokenRegistry>,
    /// Absolute path of the UDS / named pipe the companion connects to.
    /// PID-scoped so multiple codeg processes on the same host don't fight.
    pub codeg_mcp_socket_path: PathBuf,
    /// Hot-swappable progressive Host Control feature/write policy. Shared by
    /// MCP injection and the typed Host Core for execution-time rechecks.
    pub host_control_config: crate::acp::host_control::HostControlRuntimeConfig,
    /// Hot-swappable live-feedback (`check_user_feedback`) enable flag. Shared
    /// with the codeg-mcp injection so MCP injection reads it, and updated by
    /// the feedback settings command on save. Populated at startup by
    /// `apply_persisted_feedback_config`.
    pub feedback_config: crate::acp::feedback::FeedbackRuntimeConfig,
    /// Hot-swappable ask-user-question (`ask_user_question`) enable flag. Shared
    /// with the codeg-mcp injection so MCP injection reads it, and updated by
    /// the question settings command on save. Populated at startup by
    /// `apply_persisted_question_config`.
    pub question_config: crate::acp::question::QuestionRuntimeConfig,
    /// Hot-swappable get-session-info (`get_session_info`) enable flag. Shared
    /// with the codeg-mcp injection so MCP injection reads it, and updated by
    /// the session-info settings command on save. Populated at startup by
    /// `apply_persisted_session_info_config`.
    pub session_info_config: crate::acp::session_info::SessionInfoRuntimeConfig,
    /// Hot-swappable managed Session communication capability. Read when a new
    /// companion is injected and again by the Host Core on every send.
    pub session_collaboration_config:
        crate::acp::session_collaboration::SessionCollaborationRuntimeConfig,
    /// Hot-swappable chat-authoring flags (`create_automation` /
    /// `create_work_task`). Shared with the codeg-mcp injection so MCP
    /// injection reads it, re-read by the authoring write path at call time, and
    /// updated by the chat-authoring settings command on save. Populated at
    /// startup by `apply_persisted_chat_authoring_config`.
    pub chat_authoring_config: crate::acp::chat_authoring::ChatAuthoringRuntimeConfig,
    /// Serializes mutually-exclusive system operations — in-place
    /// self-update, restart, rollback — so a second click can't race a
    /// download/swap already in flight. Handlers `try_lock` and reject when
    /// held (an upgrade is already running).
    pub system_op_lock: Arc<tokio::sync::Mutex<()>>,
    /// Source of truth for an in-flight / completed app self-update, shared by
    /// the desktop (tauri-plugin-updater) and server (in-place swap) paths.
    /// The upgrade UI subscribes to it and re-syncs from a snapshot on mount,
    /// so download progress survives settings-page navigation and reloads.
    pub update_state: crate::update::AppUpdateStateHandle,
}

pub fn default_system_op_lock() -> Arc<tokio::sync::Mutex<()>> {
    Arc::new(tokio::sync::Mutex::new(()))
}

pub fn default_update_state() -> crate::update::AppUpdateStateHandle {
    crate::update::new_update_state_handle()
}

pub fn default_connection_manager() -> ConnectionManager {
    ConnectionManager::new()
}

pub fn default_terminal_manager() -> TerminalManager {
    TerminalManager::new()
}

pub fn default_chat_channel_manager() -> ChatChannelManager {
    ChatChannelManager::new()
}

/// Tauri managed-state wrapper for the codeg-mcp UDS / named-pipe address.
#[derive(Clone)]
pub struct CodegMcpSocketPath(pub PathBuf);

/// Build the shared codeg-mcp token registry, per-process socket path, and
/// runtime feature handles. Shared by server and desktop bootstrap.
///
/// The listener task is _not_ spawned here — callers spawn it after they
/// own an `Arc<AppState>` (or the relevant pieces) so the listener can
/// borrow the long-lived state without circular Arc shenanigans.
pub fn build_codeg_mcp_stack(
    connection_manager: &ConnectionManager,
) -> (
    Arc<TokenRegistry>,
    PathBuf,
    crate::acp::host_control::HostControlRuntimeConfig,
    crate::acp::feedback::FeedbackRuntimeConfig,
    crate::acp::question::QuestionRuntimeConfig,
    crate::acp::session_info::SessionInfoRuntimeConfig,
    crate::acp::session_collaboration::SessionCollaborationRuntimeConfig,
    crate::acp::chat_authoring::ChatAuthoringRuntimeConfig,
) {
    use crate::acp::connection::CodegMcpInjection;
    use crate::acp::delegation::listener::default_socket_path;
    let tokens = Arc::new(TokenRegistry::default());
    let socket_path = default_socket_path(&std::env::temp_dir());
    let host_control = crate::acp::host_control::HostControlRuntimeConfig::new();
    let feedback = crate::acp::feedback::FeedbackRuntimeConfig::new();
    let ask = crate::acp::question::QuestionRuntimeConfig::new();
    let sessions = crate::acp::session_info::SessionInfoRuntimeConfig::new();
    let collaboration = crate::acp::session_collaboration::SessionCollaborationRuntimeConfig::new();
    let authoring = crate::acp::chat_authoring::ChatAuthoringRuntimeConfig::new();

    // Install the injection on the manager so spawn_agent picks it up
    // without an extra parameter at every call site.
    connection_manager.install_codeg_mcp(CodegMcpInjection {
        tokens: tokens.clone(),
        socket_path: socket_path.clone(),
        host_control: host_control.clone(),
        feedback: feedback.clone(),
        ask: ask.clone(),
        sessions: sessions.clone(),
        collaboration: collaboration.clone(),
        authoring: authoring.clone(),
        // Same backing manager as the listener's question lookup; used only by
        // the run_connection teardown guard to reclaim a parked ask.
        questions: Arc::new(crate::acp::manager::ConnectionManagerQuestionLookup {
            manager: Arc::new(connection_manager.clone_ref()),
        }) as Arc<dyn crate::acp::question::SessionQuestionAccess>,
        // Grok `exit_plan_mode` bridge — always wired (native plan mode, no
        // feature flag), same backing manager as the question lookup.
        plan_approvals: Arc::new(crate::acp::manager::ConnectionManagerPlanApprovalLookup {
            manager: Arc::new(connection_manager.clone_ref()),
        }) as Arc<dyn crate::acp::plan_approval::SessionPlanApprovalAccess>,
    });

    (
        tokens,
        socket_path,
        host_control,
        feedback,
        ask,
        sessions,
        collaboration,
        authoring,
    )
}

impl AppState {
    /// Test-only constructor: build an `AppState` wired to an in-memory
    /// database and a `WebOnly` event emitter. Suitable for axum-test driven
    /// HTTP integration tests where no Tauri runtime is available.
    ///
    /// `data_dir` is a temp directory; handlers that touch it must use
    /// `tempfile::tempdir()` and pass the resulting path in.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn new_for_test(db: crate::db::AppDatabase, data_dir: PathBuf) -> Self {
        use crate::acp::{EventBusMetrics, InternalEventBus};
        use crate::web::event_bridge::WebEventBroadcaster;

        let broadcaster = Arc::new(WebEventBroadcaster::new());
        let metrics = Arc::new(EventBusMetrics::default());
        let acp_event_bus = Arc::new(InternalEventBus::new(metrics));
        let emitter = EventEmitter::web_only(broadcaster.clone(), acp_event_bus.clone());

        let connection_manager = default_connection_manager();
        let (
            codeg_mcp_tokens,
            codeg_mcp_socket_path,
            host_control_config,
            feedback_config,
            question_config,
            session_info_config,
            session_collaboration_config,
            chat_authoring_config,
        ) = build_codeg_mcp_stack(&connection_manager);

        Self {
            db,
            connection_manager,
            terminal_manager: default_terminal_manager(),
            event_broadcaster: broadcaster,
            acp_event_bus,
            emitter,
            prompt_queue: crate::prompt_queue::PromptQueueHandle::disconnected_for_test(),
            session_timer: crate::session_timer::SessionTimerHandle::disconnected_for_test(),
            data_dir,
            web_server_state: crate::web::WebServerState::new(),
            chat_channel_manager: default_chat_channel_manager(),
            workspace_transfer: Arc::new(
                crate::workspace_transfer::WorkspaceTransferManager::new_for_tests(
                    std::time::Duration::from_secs(60),
                ),
            ),
            pet_state: crate::pet_state_mapper::new_pet_state_handle(),
            codeg_mcp_tokens,
            codeg_mcp_socket_path,
            host_control_config,
            feedback_config,
            question_config,
            session_info_config,
            session_collaboration_config,
            chat_authoring_config,
            system_op_lock: default_system_op_lock(),
            update_state: default_update_state(),
        }
    }
}
