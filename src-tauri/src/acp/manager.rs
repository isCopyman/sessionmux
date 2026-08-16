use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ActiveValue::Set, DatabaseConnection, EntityTrait,
    TransactionTrait,
};

use crate::acp::connection::{
    spawn_agent_connection, AgentConnection, ConnectionCommand, ForkActivation, GoalControlAction,
    SteerOutcome,
};
use crate::acp::error::AcpError;
use crate::acp::feedback::{
    bounded_feedback_batch, FeedbackItem, FeedbackStatus, PendingFeedback, SessionFeedbackAccess,
    MAX_FEEDBACK_CHARS, MAX_FEEDBACK_RESPONSE_BYTES,
};
use crate::acp::plan_approval::{
    PlanApprovalAnswer, RegisteredPlanApproval, SessionPlanApprovalAccess,
};
use crate::acp::question::{
    build_outcome, QuestionAnswer, QuestionOutcome, QuestionSpec, RegisteredQuestion,
    SessionQuestionAccess,
};
use crate::acp::terminal_runtime::TerminalShellRuntimeConfig;
use crate::acp::types::{
    AcpEvent, AgentOptionsSnapshot, ConfigStaleKind, ConnectionInfo, ConnectionStatus,
    ForkResultInfo, ForkViewHandoffInfo, PromptCapabilitiesInfo, PromptInputBlock,
};
use crate::db::entities::collection_conversation;
use crate::db::entities::conversation::{self, ConversationKind, ConversationStatus};
use crate::db::service::{collaboration_service, conversation_service};
use crate::db::AppDatabase;
use crate::models::agent::AgentType;
use crate::models::CollaborationChanged;
use crate::web::event_bridge::{
    emit_event, emit_with_state, emit_with_state_gated, EventEmitter, COLLABORATION_CHANGED_EVENT,
};

/// Cap on the number of prompt-text chars kept in the `user_prompt_sent`
/// preview. Past this, `truncate_str` keeps this many chars and appends a short
/// `...` marker (so the rendered string can be a few chars longer). Bounds the
/// event payload so a large paste can't bloat the ring buffer, the per-channel
/// IM message, or the webhook body.
const USER_PROMPT_PREVIEW_MAX_CHARS: usize = 500;

/// Grace window `disconnect_all` waits after firing every `Disconnect` before
/// hard-killing surviving agent process trees. Long enough for a driver thread
/// to unwind and run its own post-loop cleanup (delegation/question/plan-approval
/// reclaim), short enough not to stall a quit noticeably. It is NOT there to
/// make the agent's death gentler — the graceful path ends in the same
/// `kill_tree`.
const DISCONNECT_ALL_GRACE: Duration = Duration::from_millis(500);

/// True for ids in the parsers' turn-id namespace (`turn-<digits>`), which every
/// parser assigns via `format!("turn-{}", n)`. A broadcast `message_id` must
/// never land here: it would collide with a persisted transcript turn id and let
/// id-keyed cross-client dedup suppress or hide a prompt. Used to reject an
/// untrusted client-supplied `message_id` of that shape.
fn is_reserved_turn_id(id: &str) -> bool {
    matches!(id.strip_prefix("turn-"), Some(rest)
        if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

/// Build the bounded preview string for a `user_prompt_sent` notification from
/// the `Text` blocks of a user prompt. Joins the (trimmed, non-empty) text
/// blocks with a space and caps the kept text at `USER_PROMPT_PREVIEW_MAX_CHARS`
/// chars (a `...` marker is appended past the cap). Returns `None` when the
/// prompt carries no text (e.g. image-only) — the notification fires for text
/// messages only.
fn user_prompt_text_preview(blocks: &[PromptInputBlock]) -> Option<String> {
    let joined = blocks
        .iter()
        .filter_map(|b| match b {
            PromptInputBlock::Text { text } => {
                let t = text.trim();
                (!t.is_empty()).then_some(t)
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    let trimmed = joined.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(crate::parsers::truncate_str(
            trimmed,
            USER_PROMPT_PREVIEW_MAX_CHARS,
        ))
    }
}

fn publish_collaboration_change(emitter: &EventEmitter, conversation_ids: Vec<i32>) {
    if conversation_ids.is_empty() {
        return;
    }
    emit_event(
        emitter,
        COLLABORATION_CHANGED_EVENT,
        CollaborationChanged { conversation_ids },
    );
}

/// Composite key identifying a logical agent session for spawn-time dedup.
/// Two `acp_connect` calls with the same triple race for the same `Mutex`,
/// so the second one observes the first's freshly-spawned connection in
/// `find_connection_for_reuse` instead of starting a duplicate process.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct SpawnDedupKey {
    agent_type: AgentType,
    working_dir: Option<PathBuf>,
    session_id: String,
}

/// Default upper bound on how long `spawn_agent` will hold the per-session
/// dedup lock waiting for `SessionStarted`. Picked to comfortably cover
/// cold-start agents (claude-code/codex warm: <2s; npx-fetched cold: 10–30s)
/// without deadlocking the next concurrent acp_connect when an agent is
/// genuinely broken.
pub(crate) const SPAWN_HANDSHAKE_TIMEOUT_SECS: u64 = 60;

/// Read the spawn-handshake timeout from `CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS`,
/// falling back to `SPAWN_HANDSHAKE_TIMEOUT_SECS`. Returns the configured
/// `Duration`. Tests can construct the manager with a custom value via
/// `with_spawn_handshake_timeout` instead of mutating env.
fn spawn_handshake_timeout_from_env() -> Duration {
    let secs = std::env::var("CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(SPAWN_HANDSHAKE_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

/// Outcome of the `spawn_agent` dedup wait. Logged so production can audit
/// how often the timeout fires vs. the agent handshake completes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandshakeWaitOutcome {
    /// `SessionStarted` applied; `external_id` is now set on the state.
    Ready,
    /// Sender was dropped before SessionStarted fired (typically the
    /// connection died during init — `run_connection` returned Err).
    Aborted,
    /// Timeout elapsed before either of the above. Releases the dedup lock
    /// so the next caller can proceed; the slow agent is no worse off.
    TimedOut,
}

impl HandshakeWaitOutcome {
    fn as_str(self) -> &'static str {
        match self {
            HandshakeWaitOutcome::Ready => "ready",
            HandshakeWaitOutcome::Aborted => "aborted",
            HandshakeWaitOutcome::TimedOut => "timeout",
        }
    }
}

/// Wait for the spawn-time `SessionStarted` signal, bounded by `timeout`.
/// Extracted so the outcome enum can be unit-tested without spawning a
/// real agent process.
async fn wait_for_session_started(
    rx: tokio::sync::oneshot::Receiver<()>,
    timeout: Duration,
) -> (HandshakeWaitOutcome, Duration) {
    let start = std::time::Instant::now();
    let outcome = match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(())) => HandshakeWaitOutcome::Ready,
        Ok(Err(_)) => HandshakeWaitOutcome::Aborted,
        Err(_) => HandshakeWaitOutcome::TimedOut,
    };
    (outcome, start.elapsed())
}

pub struct ConnectionManager {
    pub(crate) connections: Arc<Mutex<HashMap<String, AgentConnection>>>,
    /// Per-(agent, working_dir, session_id) async mutex. Held across the
    /// dedup-lookup + spawn + SessionStarted-wait critical section so two
    /// concurrent `spawn_agent` calls for the same logical session can't
    /// both miss dedup during the handshake window. Entries persist for
    /// process lifetime — bounded by the number of distinct sessions ever
    /// connected.
    spawn_locks: Arc<Mutex<HashMap<SpawnDedupKey, Arc<Mutex<()>>>>>,
    /// Bound on how long `spawn_agent` waits for the agent's handshake
    /// before releasing the dedup lock. Configurable per-instance for
    /// tests; in production initialized from env via
    /// `spawn_handshake_timeout_from_env`.
    spawn_handshake_timeout: Duration,
    /// Shared General Settings shell used by ACP terminal fallbacks. Cloned
    /// into each connection runtime so a setting update applies to existing
    /// model sessions as well as newly spawned ones.
    terminal_shell_config: TerminalShellRuntimeConfig,
    /// Delegation broker + token registry + UDS path installed during app
    /// bootstrap (`install_codeg_mcp`). When present, `spawn_agent` propagates
    /// the injection to `spawn_agent_connection`, which makes
    /// `codeg-mcp` appear in the agent's MCP server list during ACP
    /// init. `Arc<OnceLock>` so the inner `Self` cloned from `clone_ref` sees
    /// the install too — the lock is set once at startup and never mutated.
    codeg_mcp_injection: Arc<std::sync::OnceLock<crate::acp::connection::CodegMcpInjection>>,
    /// Per-agent-type serialization for `probe_agent_options`. Without
    /// this, rapid agent-tab clicks in the settings UI would fan out one
    /// real CLI process per click — each one running up to 60s. The
    /// mutex bounds concurrent probes for the same agent_type to one;
    /// different agent_types remain parallel.
    probe_locks: Arc<Mutex<HashMap<AgentType, Arc<tokio::sync::Mutex<()>>>>>,
    /// In-flight `ask_user_question` calls awaiting the user's answer, keyed by
    /// the globally-unique `question_id`. The listener parks on the receiver;
    /// the answer / cancel path resolves (and removes) the matching sender.
    /// Shared across `clone_ref` clones so the listener-facing
    /// `register_question` and the command-facing `answer_question` touch the
    /// same map. Size tracks live concurrency (the agent is blocked per ask) —
    /// no cap, no cumulative growth; entries are removed on answer / cancel /
    /// connection teardown.
    pending_questions: Arc<Mutex<HashMap<String, PendingQuestionEntry>>>,
    /// In-flight Grok `exit_plan_mode` approvals awaiting the user's decision,
    /// keyed by the globally-unique `approval_id`. The connection's ext handler
    /// parks on the receiver; the answer / cancel path resolves (and removes) the
    /// matching sender. Shared across `clone_ref` clones so the connection-facing
    /// `register_plan_approval` and the command-facing `answer_plan_approval`
    /// touch the same map. At most one per connection (the agent is blocked in
    /// its `exit_plan_mode` call) — no cap, no cumulative growth.
    pending_plan_approvals: Arc<Mutex<HashMap<String, PendingPlanApprovalEntry>>>,
}

/// A parked `ask_user_question` awaiting its answer. The `sender` resolves the
/// blocked listener round-trip; `questions` is retained so `answer_question` can
/// build the self-describing outcome without a `SessionState` read (race-free).
struct PendingQuestionEntry {
    parent_connection_id: String,
    questions: Vec<QuestionSpec>,
    sender: tokio::sync::oneshot::Sender<QuestionOutcome>,
}

/// A parked Grok `exit_plan_mode` approval awaiting the user's decision. The
/// `sender` resolves the blocked ext-request round-trip in the connection's
/// handler. `parent_connection_id` routes the resolved event + answer.
struct PendingPlanApprovalEntry {
    parent_connection_id: String,
    sender: tokio::sync::oneshot::Sender<PlanApprovalAnswer>,
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            spawn_locks: Arc::new(Mutex::new(HashMap::new())),
            spawn_handshake_timeout: spawn_handshake_timeout_from_env(),
            terminal_shell_config: TerminalShellRuntimeConfig::new(),
            codeg_mcp_injection: Arc::new(std::sync::OnceLock::new()),
            probe_locks: Arc::new(Mutex::new(HashMap::new())),
            pending_questions: Arc::new(Mutex::new(HashMap::new())),
            pending_plan_approvals: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Returns a shallow clone sharing the same underlying connection map.
    pub fn clone_ref(&self) -> Self {
        Self {
            connections: self.connections.clone(),
            spawn_locks: self.spawn_locks.clone(),
            spawn_handshake_timeout: self.spawn_handshake_timeout,
            terminal_shell_config: self.terminal_shell_config.clone(),
            codeg_mcp_injection: self.codeg_mcp_injection.clone(),
            probe_locks: self.probe_locks.clone(),
            pending_questions: self.pending_questions.clone(),
            pending_plan_approvals: self.pending_plan_approvals.clone(),
        }
    }

    /// Set the delegation injection context exactly once during bootstrap.
    /// Calling twice is a no-op — protects against accidental re-init in
    /// the unlikely event a second `build_delegation_stack` runs.
    pub fn install_codeg_mcp(&self, injection: crate::acp::connection::CodegMcpInjection) {
        let _ = self.codeg_mcp_injection.set(injection);
    }

    fn codeg_mcp_snapshot(&self) -> Option<crate::acp::connection::CodegMcpInjection> {
        self.codeg_mcp_injection.get().cloned()
    }

    /// Returns the shared terminal-shell setting consumed by ACP terminal
    /// runtimes. Keeping the handle shared makes saves apply immediately to
    /// connections that are already running.
    pub fn terminal_shell_config(&self) -> TerminalShellRuntimeConfig {
        self.terminal_shell_config.clone()
    }

    /// Test-only constructor that overrides the spawn-handshake timeout.
    /// Production code should use `new()`.
    #[cfg(test)]
    fn with_spawn_handshake_timeout(timeout: Duration) -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            spawn_locks: Arc::new(Mutex::new(HashMap::new())),
            spawn_handshake_timeout: timeout,
            terminal_shell_config: TerminalShellRuntimeConfig::new(),
            codeg_mcp_injection: Arc::new(std::sync::OnceLock::new()),
            probe_locks: Arc::new(Mutex::new(HashMap::new())),
            pending_questions: Arc::new(Mutex::new(HashMap::new())),
            pending_plan_approvals: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Insert a synthetic `AgentConnection` for tests that need to exercise
    /// downstream code (attach, event broadcast, conversation linking)
    /// without spawning a real agent process. The returned connection is
    /// marked `Connected` and has a dropped `cmd_tx` receiver, so any
    /// attempt to send a prompt resolves to `ProcessExited` — fine for
    /// tests asserting on event-bus or session-state behavior.
    ///
    /// Gated behind `cfg(test)` (in-crate unit tests) and the `test-utils`
    /// feature (integration tests in `tests/*.rs`); the item is physically
    /// uncompiled in release builds so no production caller can reach it.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn insert_test_connection(
        &self,
        id: &str,
        agent_type: AgentType,
        working_dir: Option<PathBuf>,
        emitter: EventEmitter,
    ) {
        use crate::acp::session_state::SessionState;
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let mut state = SessionState::new(
            id.to_string(),
            agent_type,
            working_dir,
            "test-window".to_string(),
            None,
        );
        state.status = ConnectionStatus::Connected;
        let conn = AgentConnection {
            id: id.to_string(),
            agent_type,
            status: ConnectionStatus::Connected,
            owner_window_label: "test-window".to_string(),
            cmd_tx: tx,
            state: Arc::new(tokio::sync::RwLock::new(state)),
            emitter,
            prompt_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_fingerprint: String::new(),
            last_observed_fingerprint: String::new(),
            child_pid: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        };
        let mut map = self.connections.lock().await;
        map.insert(id.to_string(), conn);
    }

    /// As [`insert_test_connection`], but keeps the command receiver ALIVE and
    /// returns it, so `send_prompt` can reach the concurrency gate (a dropped
    /// receiver fails `reserve()` with `ProcessExited` BEFORE the gate check,
    /// making the `TurnInProgress` branch untestable). Hold the returned
    /// receiver for the test's duration; drop it to simulate the process dying.
    ///
    /// Gated identically to [`insert_test_connection`] so it never compiles into
    /// a release build.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn insert_test_connection_live(
        &self,
        id: &str,
        agent_type: AgentType,
        working_dir: Option<PathBuf>,
        emitter: EventEmitter,
    ) -> tokio::sync::mpsc::Receiver<crate::acp::connection::ConnectionCommand> {
        use crate::acp::session_state::SessionState;
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let mut state = SessionState::new(
            id.to_string(),
            agent_type,
            working_dir,
            "test-window".to_string(),
            None,
        );
        state.status = ConnectionStatus::Connected;
        let conn = AgentConnection {
            id: id.to_string(),
            agent_type,
            status: ConnectionStatus::Connected,
            owner_window_label: "test-window".to_string(),
            cmd_tx: tx,
            state: Arc::new(tokio::sync::RwLock::new(state)),
            emitter,
            prompt_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_fingerprint: String::new(),
            last_observed_fingerprint: String::new(),
            child_pid: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        };
        self.connections.lock().await.insert(id.to_string(), conn);
        rx
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn spawn_agent(
        &self,
        agent_type: AgentType,
        working_dir: Option<String>,
        session_id: Option<String>,
        runtime_env: BTreeMap<String, String>,
        owner_window_label: String,
        emitter: EventEmitter,
        preferred_mode_id: Option<String>,
        preferred_config_values: BTreeMap<String, String>,
    ) -> Result<String, AcpError> {
        // Connection dedup: when resuming an agent session (session_id is
        // Some), look for a live AgentConnection that already represents
        // the same external session in the same working_dir for the same
        // agent_type and is not torn down. If found, reuse it instead of
        // spawning a fresh process — this is what makes a browser refresh
        // mid-turn re-attach to the existing live state rather than orphan it.
        let working_dir_path = working_dir.as_ref().map(PathBuf::from);

        // Acquire a per-(agent, working_dir, session_id) async mutex so two
        // concurrent connects for the same logical session can't both miss
        // dedup during the handshake window. The lookup → spawn → wait-for-
        // SessionStarted critical section runs under this lock; the second
        // waiter, on entry, observes the first call's connection with
        // `state.external_id` already populated and returns its id via
        // `find_connection_for_reuse`. Skipped entirely when `session_id`
        // is None (fresh sessions can't dedup — by design — since the
        // agent assigns the id).
        let session_id_for_log = session_id.clone();
        let dedup_lock = if let Some(sid) = session_id.as_deref() {
            let key = SpawnDedupKey {
                agent_type,
                working_dir: working_dir_path.clone(),
                session_id: sid.to_string(),
            };
            let mu = {
                let mut locks = self.spawn_locks.lock().await;
                locks
                    .entry(key)
                    .or_insert_with(|| Arc::new(Mutex::new(())))
                    .clone()
            };
            Some(mu.lock_owned().await)
        } else {
            None
        };

        if let Some(existing) = self
            .find_connection_for_reuse(agent_type, working_dir_path.as_ref(), session_id.as_deref())
            .await
        {
            tracing::info!(
                "[ACP] reusing connection id={} for session_id={}",
                existing,
                session_id.as_deref().unwrap_or("")
            );
            return Ok(existing);
        }

        let connection_id = uuid::Uuid::new_v4().to_string();
        tracing::info!(
            "[ACP] spawning connection id={} owner_window={} agent={:?}",
            connection_id,
            owner_window_label,
            agent_type
        );

        // `spawn_agent_connection` inserts the entry into `self.connections`,
        // installs the SessionStarted dedup signal on the state, registers
        // a cleanup hook, and returns the rx half of the signal. Any spawn
        // failure short-circuits before we touch the rx wait.
        let session_started_rx = spawn_agent_connection(
            connection_id.clone(),
            agent_type,
            working_dir,
            session_id,
            runtime_env,
            owner_window_label,
            emitter,
            self.connections.clone(),
            preferred_mode_id,
            preferred_config_values,
            self.codeg_mcp_snapshot(),
            self.terminal_shell_config.clone(),
        )
        .await?;

        // When dedup is active, hold the lock until the agent's
        // SessionStarted has applied (so external_id is populated for the
        // next waiter), aborted (connection died), or the timeout fires.
        // Logged on every wait so production can audit real-world handshake
        // latencies and tune `CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS`.
        if dedup_lock.is_some() {
            let timeout = self.spawn_handshake_timeout;
            let (outcome, elapsed) = wait_for_session_started(session_started_rx, timeout).await;
            tracing::info!(
                "[ACP] dedup_wait connection_id={} session_id={} outcome={} \
                 elapsed_ms={} timeout_ms={}",
                connection_id,
                session_id_for_log.as_deref().unwrap_or(""),
                outcome.as_str(),
                elapsed.as_millis(),
                timeout.as_millis(),
            );
        }
        // session_started_rx (in the no-dedup branch) is dropped here. tx
        // staying inside SessionState gets dropped naturally when the
        // connection terminates, no leak.

        drop(dedup_lock);

        Ok(connection_id)
    }

    /// Bump `last_activity_at` for a live connection so the idle sweep
    /// won't reap it. Used by the frontend keepalive loop to protect
    /// connections backing currently-open conversation tabs (the
    /// frontend is the only side that knows which tabs the user has
    /// open). Silently no-ops if the connection is missing or already
    /// in a terminal state — touch must never resurrect a dead
    /// connection or contend with the spawn/disconnect paths.
    pub async fn touch(&self, conn_id: &str) -> bool {
        let state_arc = {
            let connections = self.connections.lock().await;
            match connections.get(conn_id) {
                Some(conn) => conn.state.clone(),
                None => return false,
            }
        };
        let mut state = state_arc.write().await;
        if matches!(
            state.status,
            ConnectionStatus::Disconnected | ConnectionStatus::Error
        ) {
            return false;
        }
        state.last_activity_at = chrono::Utc::now();
        true
    }

    /// Disconnect connections that have been idle longer than `idle_timeout`.
    /// "Idle" means: status is `Connected`, no `pending_permission`, no
    /// launched-but-unresolved background work (async sub-agent / background
    /// shell — disconnecting kills the agent CLI and the background work with
    /// it), and no activity (no events, no commands) for at least
    /// `idle_timeout`. `Prompting` connections are always preserved (a turn is
    /// in flight). Returns the number of connections that were disconnected.
    pub async fn sweep_idle(&self, idle_timeout: Duration) -> usize {
        let now = chrono::Utc::now();
        let timeout = match chrono::Duration::from_std(idle_timeout) {
            Ok(d) => d,
            Err(_) => return 0,
        };
        let to_disconnect: Vec<String> = {
            let connections = self.connections.lock().await;
            let mut victims = Vec::new();
            for (id, conn) in connections.iter() {
                let Ok(state) = conn.state.try_read() else {
                    // Per-state writer holds the lock; a future tick will
                    // re-evaluate this entry. Don't block the connections
                    // mutex on it.
                    continue;
                };
                if state.status != ConnectionStatus::Connected {
                    continue;
                }
                if state.pending_permission.is_some() {
                    continue;
                }
                if state.has_active_background_work(now) {
                    continue;
                }
                let elapsed = now.signed_duration_since(state.last_activity_at);
                if elapsed >= timeout {
                    victims.push(id.clone());
                }
            }
            victims
        };
        let mut disconnected = 0;
        for id in to_disconnect {
            tracing::info!("[ACP] idle sweep disconnecting connection={}", id);
            if self.disconnect(&id).await.is_ok() {
                disconnected += 1;
            }
        }
        disconnected
    }

    /// Compare each running connection's spawn-time config fingerprint against a
    /// freshly recomputed one (keyed by agent type in `fresh`) and notify those
    /// that drifted. Drives the conversation-side "restart to apply" banner after
    /// a settings save.
    ///
    /// Emit policy, per connection:
    /// - emit `SessionConfigStale { stale }` only when the current fingerprint
    ///   differs from the one we last observed for it — a no-op save (identical
    ///   values) stays silent, while a second real change re-emits so a dismissed
    ///   banner reappears.
    /// - `stale = (current != spawn)`, so reverting a setting back to its
    ///   launch-time value emits `stale = false` and clears the banner.
    ///
    /// Returns the count of running connections currently on stale config across
    /// the affected agents (for the settings-side "N sessions need restart"
    /// toast). Connections whose agent type isn't in `fresh` are left untouched.
    ///
    /// `emit_with_state` is deferred until AFTER the connections-map lock is
    /// released (we collect targets first) so the SessionState write lock is
    /// never taken while holding the map lock.
    pub async fn refresh_connection_staleness(
        &self,
        fresh: &HashMap<AgentType, String>,
        kind: ConfigStaleKind,
    ) -> usize {
        let mut targets = Vec::new();
        let mut stale_count = 0usize;
        {
            let mut connections = self.connections.lock().await;
            for conn in connections.values_mut() {
                let Some(current) = fresh.get(&conn.agent_type) else {
                    continue;
                };
                let stale = *current != conn.config_fingerprint;
                if stale {
                    stale_count += 1;
                }
                if *current != conn.last_observed_fingerprint {
                    conn.last_observed_fingerprint = current.clone();
                    targets.push((Arc::clone(&conn.state), conn.emitter.clone(), stale));
                }
            }
        }
        for (state, emitter, stale) in targets {
            emit_with_state(
                &state,
                &emitter,
                AcpEvent::SessionConfigStale { stale, kind },
            )
            .await;
        }
        stale_count
    }

    /// Look up an existing live connection that we can reuse instead of
    /// spawning a new process. Reuse criteria, ALL must hold:
    /// - `session_id` is Some (we never dedup speculative / fresh connects)
    /// - the connection's `state.external_id` equals `session_id`
    /// - the connection's `agent_type` equals the requested one
    /// - the connection's `working_dir` equals the requested one (compared as
    ///   `Option<PathBuf>` so canonicalization is the caller's concern)
    /// - the connection's `state.status` is neither `Disconnected` nor `Error`
    ///
    /// Per-session state is acquired via `read().await` rather than `try_read`:
    /// the only writer is `emit_with_state`, whose critical section is
    /// microseconds (apply_event + seq++ + broadcast::send), so contention
    /// resolves quickly and the previous "skip on writer" behavior was just
    /// trading correctness (false-negative dedup → duplicate process spawn)
    /// for an imperceptible latency win. The connections-map mutex is held
    /// across the awaits — fine because no path takes `state.write()` while
    /// holding the connections mutex (no lock-cycle).
    pub(crate) async fn find_connection_for_reuse(
        &self,
        agent_type: AgentType,
        working_dir: Option<&PathBuf>,
        session_id: Option<&str>,
    ) -> Option<String> {
        // No session_id → caller is opening a fresh session; never dedup.
        let session_id = session_id?;
        let connections = self.connections.lock().await;
        for (id, conn) in connections.iter() {
            if conn.agent_type != agent_type {
                continue;
            }
            let state = conn.state.read().await;
            if state.external_id.as_deref() != Some(session_id) {
                continue;
            }
            if state.working_dir.as_ref() != working_dir {
                continue;
            }
            if matches!(
                state.status,
                ConnectionStatus::Disconnected | ConnectionStatus::Error
            ) {
                continue;
            }
            return Some(id.clone());
        }
        None
    }

    /// Forwards a prompt to the connection's command channel without
    /// touching `prompt_lock`. Internal helper — both `send_prompt` and
    /// `send_prompt_linked` acquire the lock externally and then call
    /// this. Re-entering through `send_prompt` from `send_prompt_linked`
    /// while holding the lock would deadlock, hence the split.
    async fn send_prompt_inner(
        &self,
        conn_id: &str,
        blocks: Vec<PromptInputBlock>,
        user_message: Option<(String, Vec<crate::acp::UserMessageBlock>)>,
    ) -> Result<(), AcpError> {
        // Reject an empty prompt BEFORE touching the concurrency gate. An empty
        // prompt produces no turn — and thus no `TurnComplete` to clear the gate
        // — so enqueuing one with the gate set would wedge the connection into
        // rejecting every future send. `map_prompt_blocks` is 1:1, so empty
        // input blocks is the only way the loop could see an empty prompt; we
        // stop it here at the single shared enqueue path.
        if blocks.is_empty() {
            return Err(AcpError::protocol(
                "prompt must contain at least one content block".to_string(),
            ));
        }
        let (cmd_tx, state_arc) = {
            let connections = self.connections.lock().await;
            let conn = connections
                .get(conn_id)
                .ok_or_else(|| AcpError::ConnectionNotFound(conn_id.into()))?;
            (conn.cmd_tx.clone(), conn.state.clone())
        };
        // Concurrency gate: reject a second prompt while a turn is already in
        // flight on this connection. Reserve channel capacity FIRST — that
        // `reserve().await` is the only point that can block or be cancelled.
        // Then check+set the gate and hand the command to the permit
        // synchronously: because there is NO await between setting
        // `turn_in_flight` and the infallible `permit.send`, a dropped/cancelled
        // future can never strand the flag true (it is either never set — when
        // cancelled during reserve or the lock acquisition — or always followed
        // by the enqueue). The flag is set BEFORE the enqueue so it is in place
        // before the loop can dequeue and clear it on `TurnComplete`. Without
        // the gate the second `Prompt` would queue behind the active turn and be
        // silently dropped by the loop's in-turn handler (`_ => {}`) while the
        // caller saw success. `send_prompt` callers (e.g. the chat channel)
        // rely on this gate alone (no `prompt_lock`).
        let permit = cmd_tx
            .reserve()
            .await
            .map_err(|_| AcpError::ProcessExited)?;
        {
            let mut s = state_arc.write().await;
            if s.turn_in_flight {
                // Names the gate, not just the outcome: the linked path checks
                // the same flag once before its side effects and again here, and
                // the two lines share a module target with no file/line in the
                // default format — identical text would be unattributable.
                tracing::debug!(
                    connection_id = %conn_id,
                    "[ACP] prompt rejected at the send gate: a turn is already in flight"
                );
                return Err(AcpError::TurnInProgress);
            }
            s.turn_in_flight = true;
        }
        permit.send(ConnectionCommand::Prompt {
            blocks,
            user_message,
        });
        Ok(())
    }

    /// Clone the connection's `prompt_lock` under a short connections-map lock.
    /// Returned Arc allows the caller to hold the prompt lock without
    /// keeping the connections map locked.
    async fn clone_prompt_lock(
        &self,
        conn_id: &str,
    ) -> Result<Arc<tokio::sync::Mutex<()>>, AcpError> {
        let connections = self.connections.lock().await;
        let conn = connections
            .get(conn_id)
            .ok_or_else(|| AcpError::ConnectionNotFound(conn_id.into()))?;
        Ok(conn.prompt_lock.clone())
    }

    pub async fn send_prompt(
        &self,
        conn_id: &str,
        blocks: Vec<PromptInputBlock>,
    ) -> Result<(), AcpError> {
        let prompt_lock = self.clone_prompt_lock(conn_id).await?;
        let _guard = prompt_lock.lock_owned().await;
        self.send_prompt_inner(conn_id, blocks, None).await
    }

    /// Queue-aware variant for host integrations that already have the
    /// Session database. Chat channels use this path: they share the same live
    /// connection as the GUI, so a channel message must not jump a durable GUI
    /// follow-up merely because it arrived through a different ingress.
    pub async fn send_prompt_queue_aware(
        &self,
        db: &sea_orm::DatabaseConnection,
        conn_id: &str,
        mut blocks: Vec<PromptInputBlock>,
    ) -> Result<(), AcpError> {
        let prompt_lock = self.clone_prompt_lock(conn_id).await?;
        let _guard = prompt_lock.lock_owned().await;
        let (conversation_id, emitter) = {
            let connections = self.connections.lock().await;
            let conn = connections
                .get(conn_id)
                .ok_or_else(|| AcpError::ConnectionNotFound(conn_id.into()))?;
            let conversation_id = conn.state.read().await.conversation_id;
            let emitter = conn.emitter.clone();
            (conversation_id, emitter)
        };
        if let Some(conversation_id) = conversation_id {
            let admitted = crate::db::service::prompt_queue_service::send_is_admitted(
                db,
                conversation_id,
                None,
            )
            .await
            .map_err(|err| AcpError::protocol(err.to_string()))?;
            if !admitted {
                return Err(AcpError::TurnInProgress);
            }
        }

        let turn_ref = format!("host-{}", uuid::Uuid::new_v4());
        let claimed = if let Some(conversation_id) = conversation_id {
            match collaboration_service::claim_pending_store_only_for_turn(
                db,
                conversation_id,
                &turn_ref,
            )
            .await
            {
                Ok(Some(batch)) => {
                    publish_collaboration_change(&emitter, batch.affected_conversation_ids.clone());
                    let mut harness_blocks = batch.blocks.clone();
                    harness_blocks.extend(blocks);
                    blocks = harness_blocks;
                    Some(batch)
                }
                Ok(None) => None,
                Err(err) => {
                    tracing::warn!(
                        connection_id = %conn_id,
                        conversation_id,
                        "[ACP] could not attach pending Session messages: {err}"
                    );
                    None
                }
            }
        } else {
            None
        };

        let outcome = self.send_prompt_inner(conn_id, blocks, None).await;
        if let (Some(conversation_id), Some(batch)) = (conversation_id, claimed.as_ref()) {
            let transition = if outcome.is_ok() {
                collaboration_service::mark_store_only_batch_embedded(db, conversation_id, batch)
                    .await
            } else {
                collaboration_service::release_store_only_batch(db, conversation_id, batch).await
            };
            match transition {
                Ok(affected) => publish_collaboration_change(&emitter, affected),
                Err(err) => tracing::error!(
                    connection_id = %conn_id,
                    conversation_id,
                    "[ACP][ERROR] could not finalize attached Session messages: {err}"
                ),
            }
        }
        outcome
    }

    /// Send a prompt while ensuring a `Conversation` DB row is bound to this
    /// connection. On the first call (when `state.conversation_id` is None),
    /// either:
    /// - **Caller-supplied path** — if `conversation_id` is `Some(id)`, the
    ///   caller (the frontend) has already created the row and we adopt it via
    ///   `ConversationLinked`. Requires `folder_id` to be `Some` so the event
    ///   carries both ids without forcing subscribers to re-query the DB.
    /// - **Backend-creates path** — if `conversation_id` is `None`, we create
    ///   the row from `folder_id` (required) and emit `ConversationLinked`.
    ///   Returns an error if `folder_id` is also `None`.
    ///
    /// Subsequent calls (when state is already linked) ignore both
    /// `folder_id` and `conversation_id` and just forward the prompt.
    ///
    /// Back-compat wrapper for callers that don't supply a client message id
    /// (the delegation broker, internal/test paths). The UI send path uses
    /// [`send_prompt_linked_with_message_id`] so the sender's optimistic turn
    /// dedups against the broadcast `UserMessage` echo by exact id.
    pub async fn send_prompt_linked(
        &self,
        db: &AppDatabase,
        conn_id: &str,
        blocks: Vec<PromptInputBlock>,
        folder_id: Option<i32>,
        conversation_id: Option<i32>,
    ) -> Result<Option<i32>, AcpError> {
        self.send_prompt_linked_with_message_id(
            db,
            conn_id,
            blocks,
            folder_id,
            conversation_id,
            None,
        )
        .await
    }

    /// As [`send_prompt_linked`], plus an optional `client_message_id`: the
    /// id the sending UI assigned to its own optimistic user turn. When the
    /// user prompt is broadcast as [`AcpEvent::UserMessage`] (for cross-client
    /// viewers), this id becomes the event's `message_id`, so the sender's
    /// runtime dedups the echo against its optimistic turn by EXACT id rather
    /// than a heuristic — and an unrelated optimistic turn on another client
    /// never suppresses a different sender's prompt. `None` falls back to a
    /// connection-scoped id for non-UI senders.
    #[allow(clippy::too_many_arguments)]
    pub async fn send_prompt_linked_with_message_id(
        &self,
        db: &AppDatabase,
        conn_id: &str,
        mut blocks: Vec<PromptInputBlock>,
        folder_id: Option<i32>,
        conversation_id: Option<i32>,
        client_message_id: Option<String>,
    ) -> Result<Option<i32>, AcpError> {
        // Reject an empty prompt up front, BEFORE any side effects: linking /
        // creating the conversation row, flipping it to InProgress, or emitting
        // events. An empty prompt is never accepted, so it must not mutate
        // persisted state (create a row, or flip an existing one — which would
        // then be rolled back to Cancelled). `send_prompt_inner` keeps a
        // defensive copy of this guard for the non-linked `send_prompt` path.
        if blocks.is_empty() {
            return Err(AcpError::protocol(
                "prompt must contain at least one content block".to_string(),
            ));
        }
        // Caller-supplied conversation_id requires folder_id (we include it in
        // the emitted ConversationLinked event so subscribers don't have to
        // re-query the DB). Validate before touching any state.
        if conversation_id.is_some() && folder_id.is_none() {
            return Err(AcpError::protocol(
                "conversation_id provided without folder_id".to_string(),
            ));
        }
        // Acquire the per-connection prompt lock for the entire link-check
        // + DB write + emit + cmd_tx.send sequence. Two concurrent prompts
        // (multiple browser tabs of the same conversation; chat-channel
        // racing the UI) are now strictly serialized — the second waiter
        // observes `already_linked == true` after the first commits, so
        // it can't double-create a conversation row.
        let prompt_lock = self.clone_prompt_lock(conn_id).await?;
        let _prompt_guard = prompt_lock.lock_owned().await;

        // Snapshot what we need from the connection map under one short lock.
        // The conversation-linked check happens INSIDE the prompt lock so
        // any racing send sees a consistent post-link state.
        let (
            state_arc,
            emitter,
            agent_type,
            already_linked,
            linked_conversation_id,
            turn_in_flight,
        ) = {
            let connections = self.connections.lock().await;
            let conn = connections
                .get(conn_id)
                .ok_or_else(|| AcpError::ConnectionNotFound(conn_id.into()))?;
            let (already, linked_id, in_flight) = {
                let s = conn.state.read().await;
                (
                    s.conversation_id.is_some(),
                    s.conversation_id,
                    s.turn_in_flight,
                )
            };
            (
                conn.state.clone(),
                conn.emitter.clone(),
                conn.agent_type,
                already,
                linked_id,
                in_flight,
            )
        };

        if let (Some(requested), Some(linked)) = (conversation_id, linked_conversation_id) {
            if requested != linked {
                return Err(AcpError::protocol(format!(
                    "connection {conn_id} is linked to conversation {linked}, not {requested}"
                )));
            }
        }

        // Reject a concurrent prompt while a turn is already in flight, BEFORE
        // any side effects (row creation, InProgress emit, user-message
        // broadcast). `send_prompt_inner` re-checks and sets the flag
        // authoritatively below; doing it here too — while still holding
        // `prompt_lock`, so the value can't change underneath us (the loop only
        // ever clears it) — keeps a rejected prompt from flipping the row to
        // InProgress or broadcasting a phantom user message. The frontend turns
        // this rejection into a queued message above the input box.
        if turn_in_flight {
            // Distinct from `send_prompt_inner`'s send-gate line so a log reader
            // can tell which of the two checks bounced the prompt.
            tracing::debug!(
                connection_id = %conn_id,
                "[ACP] prompt rejected before admission: a turn is already in flight"
            );
            return Err(AcpError::TurnInProgress);
        }

        // Re-hydrate uploaded image attachments (web / remote-workspace mode
        // sends empty-payload marker blocks with a `file://` uri into the
        // uploads root; see `prompt_hydration`). Deliberately placed AFTER
        // admission — the connection-exists check (`clone_prompt_lock` above)
        // and the busy reject — so garbage or concurrent prompts never
        // trigger file reads, and the prompt lock we hold serializes
        // hydration per connection (a natural concurrency bound). Still
        // BEFORE any side effect (linking / row creation / status flip) and
        // before the `user_blocks_from_prompt` projection below, so a failure
        // aborts cleanly and the viewer broadcast, the sender echo, and the
        // agent all see the full bytes.
        crate::acp::prompt_hydration::hydrate_prompt_blocks(
            &mut blocks,
            &crate::paths::codeg_uploads_root(),
        )
        .await?;
        // Collaboration envelopes are private Harness context. Keep the real
        // user blocks separate so previews and cross-view UserMessage events
        // never present another Session's mail as text authored by the human.
        let visible_user_blocks = blocks.clone();

        if !already_linked {
            match (conversation_id, folder_id) {
                // Branch A: caller already owns a row — adopt it. No DB write.
                (Some(caller_conv_id), Some(caller_folder_id)) => {
                    emit_with_state(
                        &state_arc,
                        &emitter,
                        AcpEvent::ConversationLinked {
                            conversation_id: caller_conv_id,
                            folder_id: caller_folder_id,
                            parent_conversation_id: None,
                            parent_tool_use_id: None,
                        },
                    )
                    .await;
                }
                // Function-entry guard rejects this combination.
                (Some(_), None) => unreachable!(
                    "conversation_id without folder_id should have been rejected at function entry"
                ),
                // Branch B: backend creates the row from caller-supplied
                // folder_id. Phase 3c-1 made folder_id required here — every
                // production caller that reaches this branch passes one, and
                // silent fallback to working_dir-based find-or-create masked
                // contract violations.
                (None, Some(folder_id)) => {
                    let row =
                        conversation_service::create(&db.conn, folder_id, agent_type, None, None)
                            .await
                            .map_err(|e| AcpError::protocol(e.to_string()))?;
                    emit_with_state(
                        &state_arc,
                        &emitter,
                        AcpEvent::ConversationLinked {
                            conversation_id: row.id,
                            folder_id,
                            parent_conversation_id: None,
                            parent_tool_use_id: None,
                        },
                    )
                    .await;
                    // Sidebar sync: a conversation born here (agent path — a
                    // prompt sent without a pre-created row, not the create
                    // button) must reach every client immediately via the global
                    // `conversation://changed` channel. Roots land in the sidebar
                    // list; delegation children (parent set) are routed into their
                    // parent's expanded sub-session subtree and bump its chevron.
                    // Both carry `external_id: null` here (no session yet) — the
                    // external_id write below re-broadcasts the full summary.
                    crate::commands::conversations::emit_conversation_upsert(
                        &emitter, &db.conn, row.id,
                    )
                    .await;
                }
                (None, None) => {
                    return Err(AcpError::protocol(
                        "folder_id required for new conversation row".to_string(),
                    ));
                }
            }

            // UI new-conversation path: SessionStarted applied state.external_id
            // back during acp_connect, but conversation_id was None then so the
            // lifecycle subscriber's SessionStarted handler skipped the DB write.
            // Now that we just linked the row in the same prompt_lock critical
            // section, snapshot external_id and persist it synchronously — no
            // dependence on broadcaster eventual consistency. The chat_channel
            // reverse-order path (link before SessionStarted) is unaffected and
            // continues to be handled by the lifecycle subscriber.
            let (cid_opt, eid_opt) = {
                let s = state_arc.read().await;
                (s.conversation_id, s.external_id.clone())
            };
            if let (Some(cid), Some(eid)) = (cid_opt, eid_opt) {
                conversation_service::update_external_id(&db.conn, cid, eid)
                    .await
                    .map_err(|e| AcpError::protocol(e.to_string()))?;
                // SessionStarted arrived BEFORE this link, so the lifecycle
                // subscriber skipped its broadcast (no conversation_id then).
                // Now that external_id is persisted, converge every client's
                // sidebar with the complete summary — this also corrects a
                // Branch B upsert above that necessarily carried
                // `external_id: null`. Root-only via the helper.
                crate::commands::conversations::emit_conversation_upsert(&emitter, &db.conn, cid)
                    .await;
            } else if cid_opt.is_some() {
                tracing::info!(
                    "[manager] send_prompt_linked: conversation linked but \
                     external_id not yet on state (conn={conn_id}); lifecycle \
                     subscriber will catch up when SessionStarted arrives"
                );
            }
        }

        // Centralized status transition: every prompt send flips the
        // conversation row to InProgress. This MUST happen on every call
        // (including the already-linked path) so that a follow-up turn whose
        // row is currently `pending_review` correctly transitions back. The
        // DB write precedes the event emit so any subscriber observing
        // `ConversationStatusChanged` can assume the row is consistent.
        // `update_status` is a single UPDATE — idempotent with respect to
        // the same status value, so re-writing `InProgress` is a benign no-op
        // on the row (touches `updated_at` only).
        let conversation_id_for_status = state_arc.read().await.conversation_id;
        let viewer_message_id = if conversation_id_for_status.is_some() {
            Some(match client_message_id.as_deref() {
                Some(id) if !is_reserved_turn_id(id) => id.to_string(),
                _ => format!("user-{}-{}", conn_id, state_arc.read().await.event_seq),
            })
        } else {
            None
        };
        if let Some(cid) = conversation_id_for_status {
            // A durable follow-up queue owns FIFO for this Session. An ordinary
            // send from another window may have rendered an older empty-queue
            // snapshot; reject it under the same per-connection prompt lock so
            // it is re-enqueued at the tail instead of jumping the durable
            // head. The queue worker is admitted by its exact claimed id.
            let admitted = crate::db::service::prompt_queue_service::send_is_admitted(
                &db.conn,
                cid,
                client_message_id.as_deref(),
            )
            .await
            .map_err(|err| AcpError::protocol(err.to_string()))?;
            if !admitted {
                tracing::debug!(
                    connection_id = %conn_id,
                    conversation_id = cid,
                    "[ACP] prompt rejected before admission: durable queue has priority"
                );
                return Err(AcpError::TurnInProgress);
            }
            conversation_service::update_status(&db.conn, cid, ConversationStatus::InProgress)
                .await
                .map_err(|e| AcpError::protocol(e.to_string()))?;
            emit_with_state(
                &state_arc,
                &emitter,
                AcpEvent::ConversationStatusChanged {
                    conversation_id: cid,
                    status: ConversationStatus::InProgress,
                },
            )
            .await;
        }

        // A `store_only` message is delivered visibly without waking the
        // target. Attach the oldest bounded batch only when a real user/queue
        // message starts the next ordinary turn. An invoke_when_idle queue item
        // is excluded transactionally by the collaboration service so its
        // reply obligation cannot become ambiguous.
        let claimed_store_only = if client_message_id.is_some() {
            match (conversation_id_for_status, viewer_message_id.as_deref()) {
                (Some(cid), Some(turn_ref)) => {
                    match collaboration_service::claim_pending_store_only_for_turn(
                        &db.conn, cid, turn_ref,
                    )
                    .await
                    {
                        Ok(Some(batch)) => {
                            publish_collaboration_change(
                                &emitter,
                                batch.affected_conversation_ids.clone(),
                            );
                            let mut harness_blocks = batch.blocks.clone();
                            harness_blocks.extend(blocks);
                            blocks = harness_blocks;
                            Some(batch)
                        }
                        Ok(None) => None,
                        Err(err) => {
                            tracing::warn!(
                                connection_id = %conn_id,
                                conversation_id = cid,
                                "[ACP] could not attach pending Session messages: {err}"
                            );
                            None
                        }
                    }
                }
                _ => None,
            }
        } else {
            None
        };

        // Capture a bounded preview of the user's message BEFORE `blocks` is
        // moved into `send_prompt_inner`. Only on the genuine UI path
        // (`delegation.is_none()`): delegation / sub-agent prompts are not user
        // messages. Emitted after the send succeeds (below) so a prompt that
        // never reached the agent produces no "user message" notification.
        let user_prompt_preview = user_prompt_text_preview(&visible_user_blocks);

        // Project the user's prompt blocks for the cross-client viewer
        // broadcast BEFORE `send_prompt_inner` consumes `blocks`, and hand the
        // payload to the connection loop (via `ConnectionCommand::Prompt`) so it
        // emits the `UserMessage` event in-order, right before the agent
        // request — guaranteeing its seq precedes the turn's agent events and
        // that it only fires for a prompt actually processed (a failed enqueue
        // delivers no command, so nothing strands a `pending_user_message`).
        // Gated on `delegation.is_none()` (children surface kickoff text
        // separately) and a bound conversation row (a sidebar-visible turn). The
        // `message_id` prefers the sender's client-supplied id (exact echo
        // dedup), falling back to a connection-scoped id for non-UI senders.
        let user_message: Option<(String, Vec<crate::acp::UserMessageBlock>)> =
            if conversation_id_for_status.is_some() {
                let user_blocks = crate::acp::user_blocks_from_prompt(&visible_user_blocks);
                if user_blocks.is_empty() {
                    None
                } else {
                    let message_id = viewer_message_id
                        .clone()
                        .expect("bound non-delegation prompts have a viewer message id");
                    Some((message_id, user_blocks))
                }
            } else {
                None
            };

        // We hold `_prompt_guard` here, so call the lock-free inner helper —
        // re-entering `send_prompt` would try to acquire the same mutex and
        // deadlock. The helper reserves channel capacity FIRST and only then
        // sets the turn-in-flight gate, with no await before the infallible
        // `permit.send`; so a failure (channel closed / process exited) happens
        // at the reserve step, BEFORE the gate is set — there is nothing to roll
        // back. On that failure we still flip the row to `Cancelled` so the UI
        // doesn't strand on `in_progress`: no `TurnComplete` will ever arrive
        // for a prompt that never reached the agent, so without this the
        // lifecycle subscriber's PendingReview write also never fires and the
        // row would be stuck until a follow-up `send_prompt_linked` re-flipped it.
        match self.send_prompt_inner(conn_id, blocks, user_message).await {
            Ok(()) => {
                if let (Some(cid), Some(batch)) =
                    (conversation_id_for_status, claimed_store_only.as_ref())
                {
                    match collaboration_service::mark_store_only_batch_embedded(
                        &db.conn, cid, batch,
                    )
                    .await
                    {
                        Ok(affected) => publish_collaboration_change(&emitter, affected),
                        Err(err) => tracing::error!(
                            connection_id = %conn_id,
                            conversation_id = cid,
                            "[ACP][ERROR] prompt was accepted but attached Session messages could not be finalized: {err}"
                        ),
                    }
                }
                // The prompt reached the agent: surface it to the chat-channel
                // "user message" event feed. Notification-only — never gates the
                // send result.
                if let Some(text_preview) = user_prompt_preview {
                    emit_with_state(
                        &state_arc,
                        &emitter,
                        AcpEvent::UserPromptSent { text_preview },
                    )
                    .await;
                }
                Ok(conversation_id_for_status)
            }
            Err(send_err) => {
                if let Some(cid) = conversation_id_for_status {
                    if let Some(batch) = claimed_store_only.as_ref() {
                        match collaboration_service::release_store_only_batch(&db.conn, cid, batch)
                            .await
                        {
                            Ok(affected) => publish_collaboration_change(&emitter, affected),
                            Err(err) => tracing::error!(
                                connection_id = %conn_id,
                                conversation_id = cid,
                                "[ACP][ERROR] could not restore attached Session messages after send failure: {err}"
                            ),
                        }
                    }
                    match conversation_service::update_status(
                        &db.conn,
                        cid,
                        ConversationStatus::Cancelled,
                    )
                    .await
                    {
                        Ok(_) => {
                            emit_with_state(
                                &state_arc,
                                &emitter,
                                AcpEvent::ConversationStatusChanged {
                                    conversation_id: cid,
                                    status: ConversationStatus::Cancelled,
                                },
                            )
                            .await;
                        }
                        Err(rollback_err) => {
                            // Best-effort: original send error is the load-bearing
                            // signal; rollback failure is logged but not surfaced.
                            tracing::error!(
                                "[ACP][ERROR] failed to mark conversation {cid} cancelled \
                                 after send failure (original={send_err}): {rollback_err}"
                            );
                        }
                    }
                }
                Err(send_err)
            }
        }
    }

    pub async fn set_mode(&self, conn_id: &str, mode_id: String) -> Result<(), AcpError> {
        let cmd_tx = {
            let connections = self.connections.lock().await;
            let conn = connections
                .get(conn_id)
                .ok_or_else(|| AcpError::ConnectionNotFound(conn_id.into()))?;
            conn.cmd_tx.clone()
        };
        cmd_tx
            .send(ConnectionCommand::SetMode { mode_id })
            .await
            .map_err(|_| AcpError::ProcessExited)
    }

    pub async fn set_config_option(
        &self,
        conn_id: &str,
        config_id: String,
        value_id: String,
    ) -> Result<(), AcpError> {
        let cmd_tx = {
            let connections = self.connections.lock().await;
            let conn = connections
                .get(conn_id)
                .ok_or_else(|| AcpError::ConnectionNotFound(conn_id.into()))?;
            conn.cmd_tx.clone()
        };
        cmd_tx
            .send(ConnectionCommand::SetConfigOption {
                config_id,
                value_id,
            })
            .await
            .map_err(|_| AcpError::ProcessExited)
    }

    /// Pause or clear the session's active Codex goal via the connection loop
    /// (codex-acp #293). Looked up by connectionId; the loop sources the
    /// sessionId from the live session, so callers only supply the action.
    pub async fn goal_control(
        &self,
        conn_id: &str,
        action: GoalControlAction,
    ) -> Result<(), AcpError> {
        let cmd_tx = {
            let connections = self.connections.lock().await;
            let conn = connections
                .get(conn_id)
                .ok_or_else(|| AcpError::ConnectionNotFound(conn_id.into()))?;
            conn.cmd_tx.clone()
        };
        cmd_tx
            .send(ConnectionCommand::GoalControl { action })
            .await
            .map_err(|_| AcpError::ProcessExited)
    }

    pub async fn cancel(&self, db: &DatabaseConnection, conn_id: &str) -> Result<(), AcpError> {
        // Serialize Cancel with prompt admission. If a prompt has already
        // crossed its final queue check, let it enter the command stream first
        // so this Cancel stops that prompt rather than racing ahead of it.
        let prompt_lock = self.clone_prompt_lock(conn_id).await?;
        let _guard = prompt_lock.lock_owned().await;
        let (cmd_tx, state_arc, emitter) = {
            let connections = self.connections.lock().await;
            let conn = connections
                .get(conn_id)
                .ok_or_else(|| AcpError::ConnectionNotFound(conn_id.into()))?;
            (
                conn.cmd_tx.clone(),
                conn.state.clone(),
                conn.emitter.clone(),
            )
        };
        cmd_tx
            .send(ConnectionCommand::Cancel)
            .await
            .map_err(|_| AcpError::ProcessExited)?;

        // Eagerly flip the row to `Cancelled` so the sidebar/tabs leave the
        // "running" state immediately. The agent typically replies with
        // `TurnComplete{cancelled}` which the lifecycle subscriber ignores,
        // and stays connected (so `handle_terminal_event` doesn't fire either)
        // — without this write the row would strand on `InProgress`.
        // CAS-guarded so we don't overwrite a `PendingReview`/`Completed`
        // status if the turn happened to end just before the user clicked.
        let conversation_id = state_arc.read().await.conversation_id;
        if let Some(cid) = conversation_id {
            match conversation_service::update_status_if(
                db,
                cid,
                ConversationStatus::InProgress,
                ConversationStatus::Cancelled,
            )
            .await
            {
                Ok(true) => {
                    emit_with_state(
                        &state_arc,
                        &emitter,
                        AcpEvent::ConversationStatusChanged {
                            conversation_id: cid,
                            status: ConversationStatus::Cancelled,
                        },
                    )
                    .await;
                }
                Ok(false) => {}
                Err(e) => {
                    tracing::error!(
                        "[ACP][ERROR] failed to mark conversation {cid} cancelled \
                         on user cancel (conn={conn_id}): {e}"
                    );
                }
            }
        }

        Ok(())
    }

    pub async fn respond_permission(
        &self,
        conn_id: &str,
        request_id: &str,
        option_id: &str,
    ) -> Result<(), AcpError> {
        let cmd_tx = {
            let connections = self.connections.lock().await;
            let conn = connections
                .get(conn_id)
                .ok_or_else(|| AcpError::ConnectionNotFound(conn_id.into()))?;
            conn.cmd_tx.clone()
        };
        cmd_tx
            .send(ConnectionCommand::RespondPermission {
                request_id: request_id.into(),
                option_id: option_id.into(),
            })
            .await
            .map_err(|_| AcpError::ProcessExited)
    }

    /// Fork without mutating the original identity: C1 remains bound to S1,
    /// while a freshly-created C2 is bound to S2. The live connection moves to
    /// C2 only after its row commits; the initiating View follows the explicit
    /// handoff while other Views remain attached to C1.
    pub async fn fork_session(
        &self,
        db: &AppDatabase,
        conn_id: &str,
        // Caller-supplied linkage for a connection that resumed a historical
        // conversation but hasn't sent a prompt through it yet. Such a
        // connection is bound to its session via `session_id` (resume) but its
        // conversation ROW isn't linked until the first prompt fires
        // `ConversationLinked` (see `send_prompt_linked`). A fork-send forks
        // BEFORE that first prompt, so without adopting the row here the fork
        // would reject as unlinked. Ignored when the connection is already
        // linked (the common new-conversation-then-fork path), and both must be
        // `Some` to link (a `conversation_id` needs its `folder_id`, mirroring
        // `send_prompt_linked`'s Branch A contract).
        link_conversation_id: Option<i32>,
        link_folder_id: Option<i32>,
    ) -> Result<ForkResultInfo, AcpError> {
        let (state_arc, cmd_tx, emitter) = {
            let connections = self.connections.lock().await;
            let conn = connections
                .get(conn_id)
                .ok_or_else(|| AcpError::ConnectionNotFound(conn_id.into()))?;
            (
                conn.state.clone(),
                conn.cmd_tx.clone(),
                conn.emitter.clone(),
            )
        };

        // Serialize the fork against concurrent prompts on this connection via
        // the same per-connection `prompt_lock` that `send_prompt`/
        // `send_prompt_linked` hold. A fork re-points the live session, so a
        // prompt must never start a turn underneath it. The lock is held for the
        // WHOLE operation (gate check → enqueue → protocol round-trip →
        // persistence); because the LOCK (not a flag) provides the exclusion,
        // the fork never SETS `turn_in_flight`, so there is no flag a dropped
        // future could strand and no window where a prompt's side effects (row
        // create / InProgress) commit only to lose the gate to a fork and roll
        // back to `Cancelled`.
        let prompt_lock = self.clone_prompt_lock(conn_id).await?;
        let prompt_guard = prompt_lock.lock_owned().await;

        // Link the conversation row on demand, under the prompt lock so it
        // can't race a concurrent first prompt. A conversation opened from
        // history resumes via `session_id`, but its row is bound to the
        // connection only when the first prompt fires `ConversationLinked`;
        // fork-send forks first, so adopt the caller-supplied row here (the
        // same existing-row path as `send_prompt_linked` Branch A). No-op when
        // already linked, or when the caller didn't supply both ids (the check
        // below then rejects, unchanged).
        if state_arc.read().await.conversation_id.is_none() {
            if let (Some(cid), Some(fid)) = (link_conversation_id, link_folder_id) {
                emit_with_state(
                    &state_arc,
                    &emitter,
                    AcpEvent::ConversationLinked {
                        conversation_id: cid,
                        folder_id: fid,
                        parent_conversation_id: None,
                        parent_tool_use_id: None,
                    },
                )
                .await;
            }
        }

        // Fork requires a linked conversation row — the sibling we're about
        // to create exists to preserve THIS row's pre-fork history. Without
        // a current row, fork would either orphan S1 or violate the
        // no-pre-prompt-row invariant.
        let conversation_id = state_arc.read().await.conversation_id.ok_or_else(|| {
            AcpError::protocol("fork_session requires a linked conversation row".to_string())
        })?;

        // Reject if a turn is already in flight. `prompt_lock` is FREE between a
        // prompt's enqueue and its `TurnComplete` (it is released the moment the
        // command is queued), so the lock alone can't catch a turn the loop is
        // mid-processing — only the gate can. We CHECK the gate (bouncing with
        // `TurnInProgress` so the caller re-queues) under the prompt lock, where
        // the loop is the only writer and the value can't flip to true
        // underneath us, but we never SET it: not setting the gate is precisely
        // why a dropped fork can't wedge the connection.
        if state_arc.read().await.turn_in_flight {
            tracing::debug!(
                connection_id = %conn_id,
                "[ACP] fork rejected: a turn is already in flight"
            );
            return Err(AcpError::TurnInProgress);
        }

        // CANCELLATION SHIELD. Up to here the fork is side-effect-free: if THIS
        // future is dropped now (e.g. an HTTP client disconnecting mid-fork), the
        // `prompt_guard` drops and nothing happened. But the instant we enqueue
        // `ConnectionCommand::Fork`, the connection loop executes the agent
        // `session/fork`. The connection loop returns S2 but waits at an
        // activation barrier. The detached task owns protocol → C2 commit →
        // sidebar upsert → activation, so caller cancellation cannot expose a
        // half-persisted identity or the invalid C1/S2 pair.
        // We await the task's handle purely to hand the result back to a live
        // caller; the result is harmlessly discarded if the caller is gone.
        let db_conn = db.conn.clone();
        let conn_id_for_task = conn_id.to_string();
        let handle = tokio::spawn(async move {
            // Holding the owned guard for the whole task is what shields the
            // persistence from caller cancellation.
            let _prompt_guard = prompt_guard;
            let outcome: Result<ForkResultInfo, AcpError> = async {
                // Protocol-only round trip — no DB writes inside the loop.
                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                let (activation_tx, activation_rx) = tokio::sync::oneshot::channel();
                cmd_tx
                    .send(ConnectionCommand::Fork {
                        reply: reply_tx,
                        activation: activation_rx,
                    })
                    .await
                    .map_err(|_| AcpError::ProcessExited)?;
                let protocol_result = reply_rx
                    .await
                    .map_err(|_| AcpError::protocol("Fork reply channel closed".to_string()))??;

                let forked_session_id = protocol_result.forked_session_id;
                let original_session_id = protocol_result.original_session_id;

                let forked_conversation = match Self::persist_fork_outcome(
                    &db_conn,
                    conversation_id,
                    forked_session_id.clone(),
                    original_session_id.clone(),
                )
                .await
                {
                    Ok(summary) => summary,
                    Err(error) => {
                        let _ = activation_tx.send(ForkActivation::Abort {
                            reason: error.to_string(),
                        });
                        return Err(error);
                    }
                };
                let forked_conversation_id = forked_conversation.id;

                // Publish C2 before activating the live connection so clients
                // can resolve ConversationForked without inventing metadata.
                crate::commands::conversations::emit_conversation_upsert(
                    &emitter,
                    &db_conn,
                    forked_conversation_id,
                )
                .await;

                let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
                let activation_sent = activation_tx.send(ForkActivation::Commit {
                    original_conversation_id: conversation_id,
                    forked_conversation_id,
                    folder_id: forked_conversation.folder_id,
                    ack: ack_tx,
                });
                let (connection_rebound, activation_error) = if activation_sent.is_err() {
                    (
                        false,
                        Some("fork connection exited before activation".to_string()),
                    )
                } else {
                    match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                        Ok(Ok(Ok(()))) => (true, None),
                        Ok(Ok(Err(error))) => (false, Some(error)),
                        Ok(Err(_)) => (
                            false,
                            Some("fork activation acknowledgement channel closed".to_string()),
                        ),
                        Err(_) => (
                            false,
                            Some("fork activation acknowledgement timed out".to_string()),
                        ),
                    }
                };

                Ok(ForkResultInfo {
                    forked_session_id,
                    original_session_id,
                    original_conversation_id: conversation_id,
                    forked_conversation_id,
                    forked_conversation,
                    active_view_handoff: ForkViewHandoffInfo {
                        from_conversation_id: conversation_id,
                        to_conversation_id: forked_conversation_id,
                        connection_rebound,
                        activation_error,
                    },
                })
            }
            .await;
            // Surface failures even when the caller is gone (the detached task's
            // Result would otherwise be dropped silently).
            if let Err(ref e) = outcome {
                tracing::error!(
                    "[ACP][ERROR] fork persistence failed (conn={conn_id_for_task}): {e}"
                );
            }
            outcome
        });

        match handle.await {
            Ok(result) => result,
            Err(join_err) => {
                tracing::error!(
                    "[ACP][ERROR] fork persistence task did not complete (conn={conn_id}): \
                     {join_err}"
                );
                Err(AcpError::protocol(format!(
                    "fork persistence task did not complete: {join_err}"
                )))
            }
        }
    }

    /// Persist immutable fork identities: leave C1/S1 untouched and INSERT C2
    /// bound to S2. C2 inherits stable execution/display metadata and the
    /// original Collection membership, but no live Turn/queue/mailbox state.
    ///
    /// Factored out of [`fork_session`] so the cancellation-shielded task body
    /// stays readable. Everything runs in one transaction; the connection loop
    /// cannot attach S2 until this returns successfully.
    ///
    /// The transaction is deliberately WRITE-FIRST. SeaORM's SQLite backend
    /// always opens a transaction with a plain (deferred) `BEGIN` — access mode
    /// isn't configurable per-transaction for SQLite — so a transaction that
    /// LED with the `SELECT` of the current row would take a read snapshot
    /// first; if any other pooled connection commits a write before this
    /// transaction's later UPDATE (routine under this app's concurrent
    /// multi-conversation load), SQLite can't promote that now-stale snapshot
    /// to a writer and fails the whole transaction with `SQLITE_BUSY_SNAPSHOT`
    /// (code 517) — surfaced to the user as "database is locked" even though
    /// nothing was actually deadlocked, and NOT retried by `busy_timeout` (that
    /// only covers ordinary lock contention). So the FIRST statement is a write
    /// (a value-preserving self-assignment that claims the writer lock), and
    /// only THEN do we read the row. Reading under the held write
    /// lock has a second payoff: because no other writer can interpose between
    /// the read and the UPDATE/INSERT, the title/metadata we derive can't be a
    /// stale snapshot that a concurrent rename/soft-delete already superseded —
    /// the fork observes the latest committed row and never clobbers a newer
    /// title or forks from stale routing. The opening write does not alter C1's
    /// activity timestamp merely because it was forked.
    ///
    /// The claim write is filtered on `deleted_at IS NULL` (the codebase-wide
    /// "live row" predicate). Forking a soft-deleted conversation would
    /// otherwise resurrect it as a fresh, visible sibling (`deleted_at = None`),
    /// so a claim that matches no LIVE row is treated as not-found and the whole
    /// fork aborts without writing anything. The external-id check also rejects
    /// a stale connection binding instead of cloning the wrong native history.
    async fn persist_fork_outcome(
        db_conn: &DatabaseConnection,
        conversation_id: i32,
        forked_session_id: String,
        original_session_id: String,
    ) -> Result<crate::models::DbConversationSummary, AcpError> {
        use sea_orm::sea_query::Expr;
        use sea_orm::{ColumnTrait, QueryFilter};

        let forked_conversation_id = db_conn
            .transaction::<_, i32, sea_orm::DbErr>(|txn| {
                Box::pin(async move {
                    let now = chrono::Utc::now();

                    // WRITE FIRST — see the fn doc. This value-preserving write
                    // is the transaction's opening statement so SQLite acquires the
                    // writer lock immediately instead of taking a deferred read
                    // snapshot it would later have to (and might fail to)
                    // promote. Filtered on `deleted_at IS NULL` so a soft-deleted
                    // conversation can't be forked back into a live sibling;
                    // `rows_affected == 0` means the row is gone OR deleted.
                    let claimed = conversation::Entity::update_many()
                        .col_expr(
                            conversation::Column::UpdatedAt,
                            Expr::col(conversation::Column::UpdatedAt).into(),
                        )
                        .filter(conversation::Column::Id.eq(conversation_id))
                        .filter(conversation::Column::DeletedAt.is_null())
                        .exec(txn)
                        .await?;
                    if claimed.rows_affected == 0 {
                        return Err(sea_orm::DbErr::Custom(format!(
                            "conversation {conversation_id} not found or already deleted"
                        )));
                    }

                    // Read UNDER the write lock: this SELECT sees the latest
                    // committed state and no other writer can interpose before
                    // this transaction finishes, so the derived title/metadata
                    // below can't be superseded by a concurrent rename/delete.
                    // The successful live-row claim above guarantees this returns
                    // Some; the `ok_or_else` is defensive.
                    let current = conversation::Entity::find_by_id(conversation_id)
                        .one(txn)
                        .await?
                        .ok_or_else(|| {
                            sea_orm::DbErr::Custom(format!(
                                "conversation {conversation_id} not found"
                            ))
                        })?;

                    if current.external_id.as_deref() != Some(original_session_id.as_str()) {
                        return Err(sea_orm::DbErr::Custom(format!(
                            "conversation {conversation_id} binding changed during fork: expected {original_session_id}, found {:?}",
                            current.external_id
                        )));
                    }

                    // Strip any `[Fork]` prefix tolerantly (matches the prior
                    // frontend regex `/^\[Fork]\s*/g` behaviour for both spaced
                    // and no-space variants). None title stays None.
                    let clean_title: Option<String> = current.title.as_ref().map(|t| {
                        t.strip_prefix("[Fork]")
                            .map(str::trim_start)
                            .unwrap_or(t.as_str())
                            .to_string()
                    });

                    let folder_id = current.folder_id;
                    let agent_type_str = current.agent_type.clone();
                    let git_branch = current.git_branch.clone();
                    // C2 keeps C1's sidebar routing (a forked
                    // chat conversation must stay in the Chat group). `Delegate`
                    // is unreachable here — children are never forked from the
                    // UI — but the invariant `delegate ⟺ parent_id set` wins
                    // over inheritance, so it degrades to `Regular`.
                    let forked_kind = match current.kind {
                        ConversationKind::Delegate => ConversationKind::Regular,
                        ref kind => kind.clone(),
                    };

                    // INSERT the fork as a new immutable identity. The Codeg-
                    // authored marker is locked so native title backfill cannot
                    // erase the distinction; an untitled fork remains unlocked.
                    let forked = conversation::ActiveModel {
                        id: NotSet,
                        folder_id: Set(folder_id),
                        title: Set(clean_title.map(|title| format!("[Fork] {title}"))),
                        title_locked: Set(current.title.is_some()),
                        agent_type: Set(agent_type_str),
                        status: Set(ConversationStatus::PendingReview),
                        kind: Set(forked_kind),
                        model: Set(current.model.clone()),
                        git_branch: Set(git_branch),
                        external_id: Set(Some(forked_session_id)),
                        parent_id: Set(None),
                        parent_tool_use_id: Set(None),
                        delegation_call_id: Set(None),
                        message_count: Set(0),
                        created_at: Set(now),
                        updated_at: Set(now),
                        deleted_at: Set(None),
                        archived_at: Set(None),
                        pinned_at: Set(None),
                        origin_cwd: Set(current.origin_cwd.clone()),
                    };
                    let inserted = forked.insert(txn).await?;

                    if let Some(membership) = collection_conversation::Entity::find_by_id(
                        conversation_id,
                    )
                    .one(txn)
                    .await?
                    {
                        collection_conversation::ActiveModel {
                            conversation_id: Set(inserted.id),
                            collection_id: Set(membership.collection_id),
                            created_at: Set(now),
                            updated_at: Set(now),
                        }
                        .insert(txn)
                        .await?;
                    }
                    Ok(inserted.id)
                })
            })
            .await
            .map_err(|e| AcpError::protocol(e.to_string()))?;

        conversation_service::get_by_id(db_conn, forked_conversation_id)
            .await
            .map_err(|e| AcpError::protocol(e.to_string()))
    }

    pub async fn disconnect(&self, conn_id: &str) -> Result<(), AcpError> {
        let cmd_tx = {
            let mut connections = self.connections.lock().await;
            connections.remove(conn_id).map(|conn| conn.cmd_tx)
        };
        if let Some(cmd_tx) = cmd_tx {
            tracing::info!("[ACP] disconnect connection={}", conn_id);
            let _ = cmd_tx.send(ConnectionCommand::Disconnect).await;
            Ok(())
        } else {
            Err(AcpError::ConnectionNotFound(conn_id.into()))
        }
    }

    /// Probe an agent for the modes / config_options it advertises on a fresh
    /// session, then immediately disconnect. The probe runs with
    /// `EventEmitter::Noop` so no event reaches the desktop webview, the
    /// global `WebEventBroadcaster`, or the `InternalEventBus` — the events
    /// land only in this probe connection's own (unsubscribed) per-connection
    /// stream and in its `SessionState` (which is the read source here).
    ///
    /// Used by the delegation-settings UI to enumerate the options the user
    /// can override, with the guarantee that what the UI shows is exactly
    /// what `codeg-mcp` will pass through to `session/set_config_option`
    /// when a delegation actually fires.
    ///
    /// Returns `Ok(snapshot)` even when the agent advertises no options
    /// (empty `config_options`, `None` modes) — that's a valid outcome the
    /// UI can render as "this agent has nothing to configure."
    pub async fn probe_agent_options(
        &self,
        agent_type: AgentType,
        working_dir: Option<String>,
        runtime_env: BTreeMap<String, String>,
    ) -> Result<AgentOptionsSnapshot, AcpError> {
        // Owner window label is informational only (used for
        // disconnect_by_owner_window), but worth being explicit so a probe
        // connection that somehow leaks past the disconnect below is easy to
        // identify in logs / debug snapshots.
        let owner_window = "delegation-probe".to_string();
        // Serialize concurrent probes for the same agent_type. Rapid tab
        // switching in the settings UI would otherwise fan out one real
        // CLI process per click — each one running up to 60s. The mutex
        // bounds this to one in-flight probe per agent type; different
        // agent_types still probe in parallel.
        //
        // The outer `probe_locks` guard MUST be dropped BEFORE the
        // `.lock_owned().await` on the per-agent mutex. If we held it
        // across the await, a probe queued behind another for the SAME
        // agent_type would keep the outer map locked, blocking probes
        // for every OTHER agent_type too — silently turning the
        // per-agent serialization into a global one.
        let per_agent_lock: Arc<tokio::sync::Mutex<()>> = {
            let mut locks = self.probe_locks.lock().await;
            locks
                .entry(agent_type)
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        let _probe_guard = per_agent_lock.lock_owned().await;
        let conn_id = self
            .spawn_agent(
                agent_type,
                working_dir,
                None, // brand-new session — no resume
                runtime_env,
                owner_window,
                EventEmitter::Noop,
                None,
                BTreeMap::new(),
            )
            .await?;

        // Hold an `Arc<RwLock<SessionState>>` alongside the manager's own
        // entry so the state survives even if the connection task cleans
        // up its map slot mid-handshake. Without this, an agent that
        // errors during Initialize would trigger cleanup before the
        // probe's poll loop sees the `AcpEvent::Error` payload, and
        // `wait_for_session_options` would surface the unhelpful
        // `ConnectionNotFound` instead of the agent's own error text.
        let state_arc = self.get_state(&conn_id).await;

        // Generous timeout because some agents (Gemini in particular) take
        // 8-10s just to answer Initialize before session/new can even start;
        // a tight cap here would consistently return an empty snapshot and
        // make the settings UI claim those agents have nothing to configure.
        // Matches the per-step Initialize timeout in `connection.rs`.
        let probe_timeout = Duration::from_secs(60);
        let raw_snapshot = self.wait_for_session_options(&conn_id, probe_timeout).await;

        // If the wait errored, prefer the agent's own captured error
        // message over the generic ProbeTimedOut / ConnectionNotFound —
        // an agent that died on Initialize already explained why.
        let snapshot = match raw_snapshot {
            Ok(s) => Ok(s),
            Err(wait_err) => {
                let captured = if let Some(state) = state_arc.as_ref() {
                    state.read().await.last_error.clone()
                } else {
                    None
                };
                Err(match captured {
                    Some(err) => AcpError::protocol(err.message),
                    None => wait_err,
                })
            }
        };

        // Always disconnect — including on Err — so a failed probe doesn't
        // leak an agent process. Ignore disconnect errors (best-effort
        // cleanup; the agent will exit when its stdio is dropped anyway).
        let _ = self.disconnect(&conn_id).await;
        snapshot
    }

    /// Poll a connection's `SessionState` until the agent signals it has
    /// finished publishing its initial selectors (`SelectorsReady`), then
    /// give a small grace window for any tightly-following follow-up updates
    /// before snapshotting. Waiting on `selectors_ready` — not just
    /// `config_options.is_some()` — matters because some agents emit an
    /// empty `SessionConfigOptions` first and then push the real options
    /// in a subsequent update; returning on the first `Some(vec![])` would
    /// race ahead of those updates and report the agent as having nothing
    /// to configure.
    ///
    /// The `SessionConfigOptions` / `SelectorsReady` ACP events populate
    /// `SessionState` via `apply_event` regardless of which `EventEmitter`
    /// variant the connection uses — that's why the probe can rely on
    /// `Noop` and still observe the values here.
    ///
    /// Returns `AcpError::ProbeTimedOut` when the timeout elapses without
    /// `selectors_ready` ever flipping to `true`. Distinguishing that case
    /// from a clean "ready with no options" snapshot lets the UI tell the
    /// user "the agent never published its options — retry" instead of
    /// silently claiming the agent has nothing to configure.
    async fn wait_for_session_options(
        &self,
        conn_id: &str,
        timeout: Duration,
    ) -> Result<AgentOptionsSnapshot, AcpError> {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(50);
        // Grace window between `selectors_ready` flipping true and the
        // snapshot we return. Lets a stragging `ConfigOptionUpdate` that
        // an agent emits in the same tick land before we read.
        let grace_period = Duration::from_millis(500);
        let mut selectors_ready_at: Option<std::time::Instant> = None;
        loop {
            let (config_options, modes, available_commands, prompt_capabilities, selectors_ready) = {
                let conns = self.connections.lock().await;
                let conn = conns
                    .get(conn_id)
                    .ok_or_else(|| AcpError::ConnectionNotFound(conn_id.into()))?;
                let s = conn.state.read().await;
                (
                    s.config_options.clone(),
                    s.modes.clone(),
                    s.available_commands.clone(),
                    s.prompt_capabilities.clone(),
                    s.selectors_ready,
                )
            };
            if selectors_ready {
                let ready_at = *selectors_ready_at.get_or_insert_with(std::time::Instant::now);
                if ready_at.elapsed() >= grace_period {
                    // Commands ride along from the same probe session (the grace
                    // window lets a late `available_commands` land before we read).
                    return Ok(AgentOptionsSnapshot {
                        modes,
                        config_options: config_options.unwrap_or_default(),
                        available_commands,
                        prompt_capabilities,
                    });
                }
            }
            if start.elapsed() >= timeout {
                return Err(AcpError::ProbeTimedOut);
            }
            tokio::time::sleep(poll_interval).await;
        }
    }

    pub async fn disconnect_by_owner_window(&self, owner_window_label: &str) -> usize {
        let cmd_txs = {
            let mut connections = self.connections.lock().await;
            let ids: Vec<String> = connections
                .iter()
                .filter_map(|(id, conn)| {
                    if conn.owner_window_label == owner_window_label {
                        Some(id.clone())
                    } else {
                        None
                    }
                })
                .collect();

            let mut txs = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(conn) = connections.remove(&id) {
                    txs.push(conn.cmd_tx);
                }
            }
            txs
        };

        let disconnected = cmd_txs.len();
        for cmd_tx in cmd_txs {
            let _ = cmd_tx.send(ConnectionCommand::Disconnect).await;
        }
        tracing::info!(
            "[ACP] disconnect by owner window owner_window={} count={}",
            owner_window_label,
            disconnected
        );
        disconnected
    }

    /// Disconnect every connection, then hard-kill any surviving agent process
    /// trees as a shutdown backstop.
    ///
    /// The graceful path (send `Disconnect` → the connection driver thread
    /// breaks its command loop → `run_connection` unwinds → the vendored
    /// `sacp-tokio` `ChildGuard::drop` runs `kill_tree`) is enough on its own
    /// *when it gets to run*. It doesn't at process exit: `run_connection` is
    /// driven on a dedicated `std::thread` (see `spawn_agent`), and when Tauri's
    /// `ExitRequested` handler returns the process terminates those threads
    /// mid-flight — often before the driver reaches `ChildGuard::drop` — so the
    /// agent CLI (and its own children, e.g. MCP servers / a forked `node`) is
    /// reparented and lingers until it independently notices its stdin EOF
    /// (~30s for Claude Code / node).
    ///
    /// So: fire every `Disconnect`, give the graceful path a short grace window,
    /// then synchronously `kill_tree` whatever is still running.
    ///
    /// What the window actually buys is NOT a gentler death for the agent — the
    /// graceful path ends in the same `kill_tree` — it is time for the driver
    /// thread's own post-loop cleanup (delegation-token revoke, `cancel_by_parent`
    /// for delegations / questions / plan approvals), which at quit previously
    /// got no time at all.
    ///
    /// Each pid is read from its live cell as late as possible — after the
    /// window, inside the kill loop — never from an up-front snapshot. That is
    /// load-bearing in both directions:
    /// - A connection whose process was reaped during the window has already
    ///   had its cell zeroed by the `on_exit` callback, so the backstop skips it
    ///   instead of `kill_tree`-ing a pid the OS may have recycled onto an
    ///   unrelated process tree. A connection that merely *ended* keeps its pid
    ///   and still gets swept — `ChildGuard::drop` signals without waiting, so
    ///   "the driver returned" is not "the agent is gone".
    /// - A connection whose child spawned *after* quit began (still `Connecting`
    ///   when the map was drained) publishes its pid into the same cell, so the
    ///   backstop still reaches it — an up-front snapshot would read `0` and
    ///   leak exactly the orphan this exists to kill.
    ///
    /// Only pids this manager spawned (and their descendants) are touched;
    /// `child_pid == 0` (never spawned, already finished, or a test/viewer entry
    /// that owns no process) is skipped.
    pub async fn disconnect_all(&self) -> usize {
        let handles: Vec<(
            tokio::sync::mpsc::Sender<ConnectionCommand>,
            Arc<std::sync::atomic::AtomicU32>,
        )> = {
            let mut connections = self.connections.lock().await;
            connections
                .drain()
                .map(|(_, conn)| (conn.cmd_tx, conn.child_pid))
                .collect()
        };
        let disconnected = handles.len();
        // `try_send`, not `send().await`: this is the shutdown path, and the
        // backstop below is only reachable if every send returns. A connection
        // whose command queue is full (32 deep) would otherwise park the whole
        // quit here — precisely the wedged connection whose tree most needs
        // killing. A dropped `Disconnect` just means that connection skips the
        // graceful path and gets hard-killed instead.
        for (cmd_tx, _) in &handles {
            let _ = cmd_tx.try_send(ConnectionCommand::Disconnect);
        }
        tracing::info!("[ACP] disconnect_all count={}", disconnected);

        if disconnected == 0 {
            return 0;
        }

        // Grace window: let the drivers unwind and run their own cleanup.
        tokio::time::sleep(DISCONNECT_ALL_GRACE).await;

        // Backstop: hard-kill whatever is still running. Runs on a blocking
        // thread so the synchronous `kill_tree` doesn't stall the async runtime
        // while a `block_on(disconnect_all())` shutdown caller waits on it.
        let pid_cells: Vec<Arc<std::sync::atomic::AtomicU32>> =
            handles.into_iter().map(|(_, pid)| pid).collect();
        let _ = tokio::task::spawn_blocking(move || {
            for cell in pid_cells {
                // Load here, not before the window — see the doc comment.
                let pid = cell.load(std::sync::atomic::Ordering::SeqCst);
                if pid == 0 {
                    continue;
                }
                match kill_tree::blocking::kill_tree(pid) {
                    Ok(_) => {
                        tracing::info!(
                            "[ACP] disconnect_all backstop killed process tree pid={pid}"
                        );
                    }
                    Err(e) => {
                        // The process can still exit between the load and the
                        // kill; that error is expected, not a failure.
                        tracing::debug!("[ACP] disconnect_all backstop kill_tree pid={pid}: {e}");
                    }
                }
            }
        })
        .await;

        disconnected
    }

    pub async fn list_connections(&self) -> Vec<ConnectionInfo> {
        let connections = self.connections.lock().await;
        connections.values().map(|c| c.info()).collect()
    }

    /// Raw per-connection rows for the pet panel's active-session list.
    /// "Active" = the connection is currently `Prompting`, awaiting a
    /// permission, or in an `Error` state — the sessions a user would want to
    /// see or act on from the floating pet. Idle `Connected` sessions are
    /// excluded to keep the list focused (mirrors the Codex pet "signal"
    /// model).
    ///
    /// `title` is left empty here: this layer has no DB handle. The command
    /// layer (`pet_list_active_sessions_core`) fills it from the conversation
    /// row. Connections without both a bound `conversation_id` and `folder_id`
    /// are skipped — the panel needs both to render a row and to navigate to
    /// it. Lock discipline mirrors `find_connection_by_conversation_id`: hold
    /// the connections mutex while taking each per-session read lock (the
    /// reads are microseconds and released each iteration).
    pub async fn list_active_sessions(&self) -> Vec<crate::models::pet::PetSessionEntry> {
        let connections = self.connections.lock().await;
        let mut out = Vec::new();
        for (id, conn) in connections.iter() {
            let state = conn.state.read().await;
            let (Some(conversation_id), Some(folder_id)) = (state.conversation_id, state.folder_id)
            else {
                continue;
            };
            let pending = state
                .pending_permission
                .as_ref()
                .map(crate::models::pet::PetPermissionSummary::from);
            let is_active = pending.is_some()
                || matches!(
                    state.status,
                    ConnectionStatus::Prompting | ConnectionStatus::Error
                );
            if !is_active {
                continue;
            }
            out.push(crate::models::pet::PetSessionEntry {
                connection_id: id.clone(),
                conversation_id,
                folder_id,
                agent_type: state.agent_type,
                title: String::new(),
                status: state.status.clone(),
                pending,
                // Same reason as `title`: resolving a delegation child's parent
                // needs the DB. Filled by `pet_list_active_sessions_core`.
                parent: None,
            });
        }
        out
    }

    /// Clone the `Arc<RwLock<SessionState>>` for a given connection id so the
    /// caller can read/write state without holding the connections mutex.
    /// Returns `None` if no such connection is registered.
    pub async fn get_state(
        &self,
        conn_id: &str,
    ) -> Option<std::sync::Arc<tokio::sync::RwLock<crate::acp::SessionState>>> {
        let connections = self.connections.lock().await;
        connections.get(conn_id).map(|conn| conn.state.clone())
    }

    /// Like `get_state`, but also clones the connection's `EventEmitter`.
    /// Used by the lifecycle subscriber when it needs to both update the
    /// per-session state and re-broadcast a derived event (e.g. emitting
    /// `ConversationStatusChanged` after writing the row's status).
    /// One short lock on the connections map; both pieces are cheap to clone.
    pub async fn get_state_and_emitter(
        &self,
        conn_id: &str,
    ) -> Option<(
        std::sync::Arc<tokio::sync::RwLock<crate::acp::SessionState>>,
        EventEmitter,
    )> {
        let connections = self.connections.lock().await;
        connections
            .get(conn_id)
            .map(|conn| (conn.state.clone(), conn.emitter.clone()))
    }

    /// Wait (bounded) for the connected agent to say what a prompt may carry.
    ///
    /// `spawn_agent` returns as soon as the process is up and registered: it
    /// only holds for `SessionStarted` when deduplicating a RESUME, so a fresh
    /// session's `initialize` — which is what publishes these capabilities
    /// (`emit_prompt_capabilities`) — is still in flight when it returns. A
    /// caller that must know the encoding before its first prompt therefore has
    /// to ask, rather than read the state and find `None`.
    ///
    /// `None` means the wait ran out or the connection went away; callers treat
    /// that as "no information" and send what they already had, never as an
    /// error — the prompt itself is about to surface any real problem.
    pub async fn wait_for_prompt_capabilities(
        &self,
        conn_id: &str,
        timeout: Duration,
    ) -> Option<PromptCapabilitiesInfo> {
        let state = {
            let connections = self.connections.lock().await;
            connections.get(conn_id)?.state.clone()
        };
        let start = std::time::Instant::now();
        loop {
            {
                let s = state.read().await;
                if let Some(caps) = s.prompt_capabilities.clone() {
                    return Some(caps);
                }
                // A connection that already died will never advertise anything.
                if matches!(
                    s.status,
                    ConnectionStatus::Disconnected | ConnectionStatus::Error
                ) {
                    return None;
                }
            }
            if start.elapsed() >= timeout {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Append a live-feedback note to a connection's session and broadcast it.
    ///
    /// Validation: the text is trimmed and rejected when empty
    /// ([`AcpError::InvalidFeedback`]) or longer than [`MAX_FEEDBACK_CHARS`] —
    /// the full text rides in the broadcast event, the snapshot, and the MCP
    /// response, so a sanity bound keeps one pathological note from bloating
    /// them. (There is deliberately no per-turn COUNT cap: the set is cleared
    /// every turn, so its size scales with human typing, not unboundedly.)
    ///
    /// Rejected with [`AcpError::NoActiveTurn`] unless a turn is in flight —
    /// feedback is mid-turn steering, pulled by the agent via the
    /// `check_user_feedback` MCP tool; with no active turn there is nothing to
    /// steer and the note would strand (the frontend falls back to an ordinary
    /// prompt). The append rides `emit_with_state` so `SessionState.feedback`,
    /// the ring buffer, and every attached client stay in lockstep.
    pub async fn submit_feedback(
        &self,
        conn_id: &str,
        text: String,
    ) -> Result<FeedbackItem, AcpError> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(AcpError::InvalidFeedback("empty note".into()));
        }
        if trimmed.chars().count() > MAX_FEEDBACK_CHARS {
            return Err(AcpError::InvalidFeedback(format!(
                "note exceeds {MAX_FEEDBACK_CHARS} characters"
            )));
        }
        let text = trimmed.to_string();
        let (state, cmd_tx, emitter) = {
            let connections = self.connections.lock().await;
            let conn = connections
                .get(conn_id)
                .ok_or_else(|| AcpError::ConnectionNotFound(conn_id.to_string()))?;
            (
                conn.state.clone(),
                conn.cmd_tx.clone(),
                conn.emitter.clone(),
            )
        };
        // Channel-aware eligibility: a note is deliverable natively (the
        // `_session/steering` push, synthesized at initialize) or through the
        // `check_user_feedback` pull tool. Reject only when NEITHER channel
        // exists — e.g. a session launched before the feature was enabled (no
        // tool) on an adapter without proven native steering.
        // `native_steering_available` is re-read on every call so a
        // `startedNewTurn` downgrade mid-session silently reroutes later
        // notes to the pull path.
        let (native, tool_available) = {
            let s = state.read().await;
            (s.native_steering_available, s.feedback_tool_available)
        };
        if !native && !tool_available {
            return Err(AcpError::FeedbackDisabled);
        }

        if native {
            return Self::submit_feedback_native(conn_id, state, cmd_tx, emitter, text).await;
        }

        let item =
            FeedbackItem::new_pending(uuid::Uuid::new_v4().to_string(), text, chrono::Utc::now());
        // Gate on `turn_in_flight` and append in ONE critical section (via the
        // gated emit): a `TurnComplete` (flips the flag) or `UserMessage`
        // (clears `feedback`) can't slip between the gate and the append+seq, so
        // a note is never stranded on a finished turn nor re-added to a new one.
        let applied = emit_with_state_gated(
            &state,
            &emitter,
            AcpEvent::FeedbackSubmitted { item: item.clone() },
            |s| s.turn_in_flight,
        )
        .await;
        if !applied {
            return Err(AcpError::NoActiveTurn);
        }
        Ok(item)
    }

    /// Try the proven native `_session/steering` channel without falling back
    /// to the MCP pull-based feedback path.
    ///
    /// Cross-Session delivery uses this narrower contract: `Some` means the
    /// adapter consumed the immutable collaboration envelope; `None` means it
    /// definitely did not and the durable prompt queue still owns it. A real
    /// transport/protocol error remains an error because consumption may be
    /// uncertain and the caller must pause rather than replay blindly.
    pub(crate) async fn try_submit_native_feedback(
        &self,
        conn_id: &str,
        text: String,
    ) -> Result<Option<FeedbackItem>, AcpError> {
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.chars().count() > MAX_FEEDBACK_CHARS {
            return Ok(None);
        }
        let text = trimmed.to_string();
        let (state, cmd_tx, emitter) = {
            let connections = self.connections.lock().await;
            let Some(conn) = connections.get(conn_id) else {
                return Ok(None);
            };
            (
                conn.state.clone(),
                conn.cmd_tx.clone(),
                conn.emitter.clone(),
            )
        };
        {
            let state = state.read().await;
            if !state.native_steering_available || !state.turn_in_flight {
                return Ok(None);
            }
        }
        match Self::submit_feedback_native(conn_id, state, cmd_tx, emitter, text).await {
            Ok(item) => Ok(Some(item)),
            Err(AcpError::NoActiveTurn) => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// Native `_session/steering` delivery — the push half of
    /// [`Self::submit_feedback`], cancellation-shielded.
    ///
    /// SHIELD (fork_session's pattern): once `ConnectionCommand::Steer` is
    /// enqueued, the connection loop performs the injection REGARDLESS of
    /// whether this caller survives. An HTTP caller disconnecting mid-await
    /// must not leave "injected but unrecorded" state — no `FeedbackItem`, no
    /// broadcast, absent from reconnect snapshots — so enqueue → outcome →
    /// record → emit runs in a DETACHED task and the caller merely awaits the
    /// JoinHandle to relay the result to a still-live client.
    ///
    /// Invariants (asserted by tests):
    /// * NO DOUBLE DELIVERY. A natively-injected note is `Delivered` from
    ///   birth, and `read_pending_feedback` only returns `Pending` — so even
    ///   with the `check_user_feedback` tool still injected (it is, by
    ///   design), a pull can never hand the agent the same text again.
    /// * UNGATED EMIT. The pull path's `emit_with_state_gated(turn_in_flight)`
    ///   is exactly wrong here: the adapter's `injected` reply is the
    ///   authoritative fact the content reached the turn. Gating on a
    ///   just-settled turn would misreport `NoActiveTurn`, and the caller
    ///   would resubmit the same text as a prompt → duplicate. A delivered
    ///   note recorded right after `TurnComplete` is harmless — the notes
    ///   list renders only while prompting, and the next turn's `UserMessage`
    ///   clears `feedback`.
    async fn submit_feedback_native(
        conn_id: &str,
        state: Arc<tokio::sync::RwLock<crate::acp::session_state::SessionState>>,
        cmd_tx: tokio::sync::mpsc::Sender<ConnectionCommand>,
        emitter: EventEmitter,
        text: String,
    ) -> Result<FeedbackItem, AcpError> {
        // Cheap pre-flight, NOT the authoritative check (that's the loop's
        // idle arm replying `NoActiveTurn`): skip the round-trip when no turn
        // is in flight at all.
        if !state.read().await.turn_in_flight {
            return Err(AcpError::NoActiveTurn);
        }
        let conn_id_for_task = conn_id.to_string();
        let handle = tokio::spawn(async move {
            let outcome: Result<FeedbackItem, AcpError> = async {
                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                cmd_tx
                    .send(ConnectionCommand::Steer {
                        text: text.clone(),
                        reply: reply_tx,
                    })
                    .await
                    .map_err(|_| AcpError::ProcessExited)?;
                let steer = reply_rx
                    .await
                    .map_err(|_| AcpError::protocol("Steer reply channel closed".to_string()))??;
                match steer {
                    // Honored opt-in: the content was NOT consumed and is
                    // still host-owned. Surface the frontend's existing
                    // turn-end fallback (resubmit as a normal prompt).
                    SteerOutcome::PromptRequired => return Err(AcpError::NoActiveTurn),
                    SteerOutcome::Injected => {}
                    SteerOutcome::StartedNewTurn => {
                        // The adapter ignored the opt-in and detached a turn
                        // (stale binary lying about its version?). The content
                        // IS consumed — record it delivered, NEVER resend —
                        // but this adapter can't be trusted with the idle race
                        // again: downgrade to the MCP pull channel for the
                        // rest of the session.
                        tracing::warn!(
                            "[ACP][feedback] _session/steering returned startedNewTurn \
                             (conn={conn_id_for_task}); downgrading native steering for \
                             this session"
                        );
                        state.write().await.native_steering_available = false;
                    }
                }
                let item = FeedbackItem::new_delivered(
                    uuid::Uuid::new_v4().to_string(),
                    text,
                    chrono::Utc::now(),
                );
                // Ungated on purpose — see the invariant on this fn's doc.
                emit_with_state(
                    &state,
                    &emitter,
                    AcpEvent::FeedbackSubmitted { item: item.clone() },
                )
                .await;
                Ok(item)
            }
            .await;
            // Surface real failures even when the caller is gone (the
            // detached task's Result would otherwise drop silently).
            // `NoActiveTurn` is an expected reroute, not a failure.
            if let Err(ref e) = outcome {
                if !matches!(e, AcpError::NoActiveTurn) {
                    tracing::error!(
                        "[ACP][ERROR] native feedback submit failed (conn={conn_id_for_task}): {e}"
                    );
                }
            }
            outcome
        });
        match handle.await {
            Ok(result) => result,
            Err(join_err) => Err(AcpError::protocol(format!(
                "steer task did not complete: {join_err}"
            ))),
        }
    }

    /// Read the pending feedback for a connection WITHOUT marking it delivered.
    /// Returns an immediate snapshot. Read-only — backs the READ half of the
    /// `check_user_feedback` round-trip so the listener can commit delivery only
    /// after the response is actually written (a dropped / failed write leaves
    /// the notes pending for the agent's next check).
    pub async fn read_pending_feedback(&self, conn_id: &str) -> Vec<PendingFeedback> {
        let Some(state) = self.get_state(conn_id).await else {
            return Vec::new();
        };
        let pending: Vec<PendingFeedback> = {
            let s = state.read().await;
            s.feedback
                .iter()
                .filter(|f| f.status == FeedbackStatus::Pending)
                .map(|f| PendingFeedback {
                    id: f.id.clone(),
                    text: f.text.clone(),
                    created_at: f.created_at,
                })
                .collect()
        };
        bounded_feedback_batch(pending, MAX_FEEDBACK_RESPONSE_BYTES)
    }

    /// Mark the named notes `Delivered` and broadcast the consumption. Called by
    /// the listener ONLY after the `check_user_feedback` response was written to
    /// the companion, so a dropped / failed write leaves the notes pending and
    /// the agent's next check re-delivers them (at-least-once).
    ///
    /// Delivery boundary: "delivered" means the response reached the agent's MCP
    /// companion over the UDS. The one remaining hop (companion → agent stdout)
    /// can only fail when the agent process is gone/closing — i.e. the turn is
    /// being torn down, at which point the note is moot (the agent won't act on
    /// it). A mid-wait cancel is already handled upstream by the listener's
    /// peer-close race (no commit), and a cancel after the round-trip completes
    /// cannot suppress the response (the companion's inflight entry is already
    /// consumed). So this is the right boundary for a best-effort steering
    /// side-channel; an end-to-end ack would only cover the moot teardown tail.
    ///
    /// The mark happens under a single write lock; only notes still `Pending`
    /// flip (idempotent — a repeated commit, or a note already consumed by a
    /// racing call, is skipped) and only the ids actually flipped are emitted,
    /// so a double-commit can't double-broadcast.
    pub async fn commit_feedback_delivered(&self, conn_id: &str, ids: Vec<String>) {
        if ids.is_empty() {
            return;
        }
        let Some((state, emitter)) = self.get_state_and_emitter(conn_id).await else {
            return;
        };
        let id_set: std::collections::HashSet<&String> = ids.iter().collect();
        let delivered_at = chrono::Utc::now();
        let marked: Vec<String> = {
            let mut s = state.write().await;
            let mut marked = Vec::new();
            for f in s.feedback.iter_mut() {
                if f.status == FeedbackStatus::Pending && id_set.contains(&f.id) {
                    f.status = FeedbackStatus::Delivered;
                    f.delivered_at = Some(delivered_at);
                    marked.push(f.id.clone());
                }
            }
            marked
        };
        if !marked.is_empty() {
            emit_with_state(
                &state,
                &emitter,
                AcpEvent::FeedbackConsumed {
                    ids: marked,
                    delivered_at,
                },
            )
            .await;
        }
    }

    /// Register a blocking `ask_user_question` on a connection: park a one-shot
    /// in `pending_questions` keyed by a fresh `question_id`, broadcast the
    /// `QuestionRequest` (so every attached client renders the interactive card
    /// and a mid-turn attach recovers it from the snapshot), and hand the
    /// receiver back to the listener to await. `None` when the connection is
    /// gone (nothing to ask) OR when this connection already has a pending ask
    /// — see below.
    ///
    /// One pending ask per connection: `SessionState.pending_question` and the
    /// frontend card are single slots, so a second concurrent ask would
    /// overwrite the first's card/snapshot and orphan the first (still-parked)
    /// tool call with no way to answer it. A single agent is blocked in its
    /// `ask_user_question` call and cannot issue a second, so this only guards a
    /// parallel / misbehaving MCP client; the refused second call resolves as
    /// `declined` (the listener's None path) so its agent proceeds with its own
    /// judgment instead of hanging. The check + insert are atomic under the
    /// registry lock.
    pub async fn register_question(
        &self,
        conn_id: &str,
        questions: Vec<QuestionSpec>,
    ) -> Option<RegisteredQuestion> {
        // Defense-in-depth: the companion validates, but the broker socket is
        // only token-gated, so refuse to broadcast malformed/oversized specs
        // (None → the listener declines the ask, as for any other None path).
        if crate::acp::question::validate_specs(&questions).is_err() {
            return None;
        }
        let (state, emitter) = self.get_state_and_emitter(conn_id).await?;
        let question_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut reg = self.pending_questions.lock().await;
            if reg.values().any(|e| e.parent_connection_id == conn_id) {
                return None;
            }
            reg.insert(
                question_id.clone(),
                PendingQuestionEntry {
                    parent_connection_id: conn_id.to_string(),
                    questions: questions.clone(),
                    sender: tx,
                },
            );
        }
        // Ungated emit: the agent is blocked in the tool call, so the card must
        // show regardless of any turn-flag timing.
        emit_with_state(
            &state,
            &emitter,
            AcpEvent::QuestionRequest {
                question_id: question_id.clone(),
                questions,
            },
        )
        .await;
        // Teardown event-ordering race: `cancel_questions_by_parent` may have
        // drained this entry between the insert above and the emit just now. The
        // QuestionRequest we broadcast would then have no waiter, and the sweep's
        // QuestionResolved may have raced ahead of it — leaving a card up with no
        // live backend waiter. Emit a compensating QuestionResolved (ordered after
        // our QuestionRequest) and decline. (The listener's post-register token
        // re-check covers the complementary case: a register that lands entirely
        // after the sweep, which this presence check would not catch.)
        if self
            .compensate_if_question_drained(&question_id, &state, &emitter)
            .await
        {
            return None;
        }
        Some(RegisteredQuestion {
            question_id,
            answer_rx: rx,
        })
    }

    /// Returns `true` — after emitting a clearing `QuestionResolved` — when
    /// `question_id` is no longer pending, i.e. a teardown sweep drained it in the
    /// window after its `QuestionRequest` was broadcast. The compensating event is
    /// ordered after the request so no client keeps a card with no live backend
    /// waiter. Returns `false` (no emit) while the entry is still parked.
    async fn compensate_if_question_drained(
        &self,
        question_id: &str,
        state: &std::sync::Arc<tokio::sync::RwLock<crate::acp::SessionState>>,
        emitter: &EventEmitter,
    ) -> bool {
        if self
            .pending_questions
            .lock()
            .await
            .contains_key(question_id)
        {
            return false;
        }
        emit_with_state(
            state,
            emitter,
            AcpEvent::QuestionResolved {
                question_id: question_id.to_string(),
            },
        )
        .await;
        true
    }

    /// Resolve a pending `ask_user_question` with the user's submission (from any
    /// client). Removes the one-shot atomically (first answer wins; a duplicate /
    /// already-resolved id is an idempotent no-op), sends the self-describing
    /// outcome to the blocked listener, and broadcasts `QuestionResolved` so the
    /// card clears on every client. Routing uses the entry's stored parent
    /// connection (the `question_id` is the authoritative key), so a stale
    /// `conn_id` from the caller can't misroute.
    pub async fn answer_question(
        &self,
        conn_id: &str,
        question_id: &str,
        answer: QuestionAnswer,
    ) -> Result<(), AcpError> {
        let _ = conn_id;
        let entry = self.pending_questions.lock().await.remove(question_id);
        let Some(entry) = entry else {
            // Already answered / canceled / gone elsewhere — idempotent success.
            return Ok(());
        };
        let outcome = build_outcome(&entry.questions, &answer);
        // Ignore a dropped receiver: the listener may have abandoned the wait
        // (peer-close) at the same instant; the resolved-event below still clears
        // the card.
        let _ = entry.sender.send(outcome);
        if let Some((state, emitter)) = self
            .get_state_and_emitter(&entry.parent_connection_id)
            .await
        {
            emit_with_state(
                &state,
                &emitter,
                AcpEvent::QuestionResolved {
                    question_id: question_id.to_string(),
                },
            )
            .await;
        }
        Ok(())
    }

    /// Cancel a pending `ask_user_question` — the companion's tool call was
    /// canceled (peer-close) or the connection is tearing down. Removes the
    /// one-shot (dropping the sender unblocks the listener with a declined
    /// outcome) and broadcasts `QuestionResolved` so the card clears. No-op if
    /// the question was already answered / gone.
    pub async fn cancel_question(&self, conn_id: &str, question_id: &str) {
        let _ = conn_id;
        let removed = self.pending_questions.lock().await.remove(question_id);
        let Some(entry) = removed else {
            return;
        };
        if let Some((state, emitter)) = self
            .get_state_and_emitter(&entry.parent_connection_id)
            .await
        {
            emit_with_state(
                &state,
                &emitter,
                AcpEvent::QuestionResolved {
                    question_id: question_id.to_string(),
                },
            )
            .await;
        }
    }

    /// Cancel every pending `ask_user_question` parked on a connection that is
    /// tearing down. The `run_connection` cleanup guard calls this (alongside
    /// the delegation `DelegationBroker::cancel_by_parent` cascade) so question
    /// entries — and the listener tasks parked on them — are reclaimed
    /// synchronously on disconnect, instead of lingering until the companion's
    /// ask socket happens to close. Dropping each entry's sender unblocks its
    /// listener with a declined outcome; the `QuestionResolved` broadcast clears
    /// the card on every client. No-op when nothing is pending for this parent.
    pub async fn cancel_questions_by_parent(&self, conn_id: &str) {
        // Remove every entry for this parent under the lock (dropping their
        // senders unblocks the parked listeners), then emit outside the lock —
        // the registry mutex is never held across an await.
        let drained: Vec<String> = {
            let mut reg = self.pending_questions.lock().await;
            let ids: Vec<String> = reg
                .iter()
                .filter(|(_, e)| e.parent_connection_id == conn_id)
                .map(|(id, _)| id.clone())
                .collect();
            for id in &ids {
                reg.remove(id);
            }
            ids
        };
        if drained.is_empty() {
            return;
        }
        // Best-effort card clear: depending on the teardown path the connection
        // may already be out of the map (`disconnect` removes it before the
        // run_connection cleanup guard fires this sweep), so tolerate `None` — the
        // core removal above already ran and the frontend clears on disconnect.
        if let Some((state, emitter)) = self.get_state_and_emitter(conn_id).await {
            for question_id in drained {
                emit_with_state(&state, &emitter, AcpEvent::QuestionResolved { question_id }).await;
            }
        }
    }

    /// Register a pending Grok `exit_plan_mode` approval on `conn_id`, broadcast
    /// `PlanApprovalRequest` (so every attached client renders the card and a
    /// mid-turn attach recovers it from the snapshot), and return the receiver
    /// the connection's ext handler awaits. `None` when the connection is gone
    /// (nothing to approve) OR when one is already pending on it (the agent is
    /// blocked in a single `exit_plan_mode` call; a second would orphan the
    /// first). Mirrors [`Self::register_question`].
    pub async fn register_plan_approval(
        &self,
        conn_id: &str,
        tool_call_id: String,
        plan_markdown: String,
    ) -> Option<RegisteredPlanApproval> {
        let (state, emitter) = self.get_state_and_emitter(conn_id).await?;
        let approval_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut reg = self.pending_plan_approvals.lock().await;
            if reg.values().any(|e| e.parent_connection_id == conn_id) {
                return None;
            }
            reg.insert(
                approval_id.clone(),
                PendingPlanApprovalEntry {
                    parent_connection_id: conn_id.to_string(),
                    sender: tx,
                },
            );
        }
        // Ungated emit: the agent is blocked in the tool call, so the card must
        // show regardless of any turn-flag timing.
        emit_with_state(
            &state,
            &emitter,
            AcpEvent::PlanApprovalRequest {
                approval_id: approval_id.clone(),
                tool_call_id,
                plan_markdown,
            },
        )
        .await;
        // Teardown event-ordering race (mirrors `register_question`): the
        // cleanup guard's `cancel_plan_approvals_by_parent` may have drained this
        // entry between the insert above and the emit just now — its
        // `PlanApprovalResolved` could then have raced ahead of our
        // `PlanApprovalRequest`, leaving a card up with no live backend waiter.
        // Emit a compensating `PlanApprovalResolved` (ordered after our request)
        // and decline.
        if self
            .compensate_if_plan_approval_drained(&approval_id, &state, &emitter)
            .await
        {
            return None;
        }
        Some(RegisteredPlanApproval {
            approval_id,
            answer_rx: rx,
        })
    }

    /// Returns `true` — after emitting a clearing `PlanApprovalResolved` — when
    /// `approval_id` is no longer pending, i.e. a teardown sweep drained it in the
    /// window after its `PlanApprovalRequest` was broadcast. Mirrors
    /// [`Self::compensate_if_question_drained`].
    async fn compensate_if_plan_approval_drained(
        &self,
        approval_id: &str,
        state: &std::sync::Arc<tokio::sync::RwLock<crate::acp::SessionState>>,
        emitter: &EventEmitter,
    ) -> bool {
        if self
            .pending_plan_approvals
            .lock()
            .await
            .contains_key(approval_id)
        {
            return false;
        }
        emit_with_state(
            state,
            emitter,
            AcpEvent::PlanApprovalResolved {
                approval_id: approval_id.to_string(),
            },
        )
        .await;
        true
    }

    /// Resolve a pending plan approval with the user's decision (from any
    /// client). Removes the one-shot atomically (first answer wins; a duplicate /
    /// already-resolved id is an idempotent no-op), sends the decision to the
    /// blocked ext handler, and broadcasts `PlanApprovalResolved` so the card
    /// clears on every client. Routing uses the entry's stored parent connection
    /// (the `approval_id` is the authoritative key), so a stale `conn_id` from the
    /// caller can't misroute. Mirrors [`Self::answer_question`].
    pub async fn answer_plan_approval(
        &self,
        conn_id: &str,
        approval_id: &str,
        answer: PlanApprovalAnswer,
    ) -> Result<(), AcpError> {
        let _ = conn_id;
        let entry = self.pending_plan_approvals.lock().await.remove(approval_id);
        let Some(entry) = entry else {
            // Already answered / canceled / gone elsewhere — idempotent success.
            return Ok(());
        };
        // Ignore a dropped receiver: the handler may have abandoned the wait
        // (teardown) at the same instant; the resolved event below still clears
        // the card.
        let _ = entry.sender.send(answer);
        if let Some((state, emitter)) = self
            .get_state_and_emitter(&entry.parent_connection_id)
            .await
        {
            emit_with_state(
                &state,
                &emitter,
                AcpEvent::PlanApprovalResolved {
                    approval_id: approval_id.to_string(),
                },
            )
            .await;
        }
        Ok(())
    }

    /// Cancel every pending plan approval parked on a connection that is tearing
    /// down (the `run_connection` cleanup guard calls this). Dropping each
    /// entry's sender resolves the handler's await as a disconnect (Grok keeps
    /// plan mode active, re-surfaces on reconnect); the `PlanApprovalResolved`
    /// broadcast clears the card. Mirrors [`Self::cancel_questions_by_parent`].
    pub async fn cancel_plan_approvals_by_parent(&self, conn_id: &str) {
        let drained: Vec<String> = {
            let mut reg = self.pending_plan_approvals.lock().await;
            let ids: Vec<String> = reg
                .iter()
                .filter(|(_, e)| e.parent_connection_id == conn_id)
                .map(|(id, _)| id.clone())
                .collect();
            for id in &ids {
                reg.remove(id);
            }
            ids
        };
        if drained.is_empty() {
            return;
        }
        // Best-effort card clear: the connection may already be out of the map
        // (disconnect removes it before this sweep), so tolerate `None`.
        if let Some((state, emitter)) = self.get_state_and_emitter(conn_id).await {
            for approval_id in drained {
                emit_with_state(
                    &state,
                    &emitter,
                    AcpEvent::PlanApprovalResolved { approval_id },
                )
                .await;
            }
        }
    }

    /// Resolve a conversation_id to its currently-active connection id, if any.
    /// Used by the by-conversation snapshot endpoint and the LifecycleSubscriber.
    /// Per-session state is acquired via `read().await` to avoid the
    /// `try_read`-skip false negative that would intermittently return None
    /// while `emit_with_state` is mid-update — the wait is microseconds.
    pub async fn find_connection_by_conversation_id(&self, conversation_id: i32) -> Option<String> {
        let connections = self.connections.lock().await;
        for (id, conn) in connections.iter() {
            let state = conn.state.read().await;
            if state.conversation_id == Some(conversation_id) {
                return Some(id.clone());
            }
        }
        None
    }

    /// The in-flight user prompt for `conversation_id` and the instant its turn
    /// started, if a turn is currently running on its live connection. `Some`
    /// exactly between `UserMessage` and `TurnComplete` (see
    /// `SessionState.pending_user_message` / `pending_user_message_started_at`);
    /// `None` when no connection is bound to the conversation or no turn is in
    /// flight.
    ///
    /// Used by the detail endpoint to stamp the persisted in-flight user turn
    /// with the broadcast `message_id`, so a cross-client viewer's synthesized
    /// turn (keyed by that same id) dedups against it instead of rendering a
    /// second copy. The start instant lets the matcher tell the in-flight prompt
    /// apart from a prior identical one. One lock pass over the connections map,
    /// mirroring `find_connection_by_conversation_id`.
    pub async fn pending_user_message_for_conversation(
        &self,
        conversation_id: i32,
    ) -> Option<(
        crate::acp::session_state::PendingUserMessage,
        Option<chrono::DateTime<chrono::Utc>>,
    )> {
        let connections = self.connections.lock().await;
        for conn in connections.values() {
            let state = conn.state.read().await;
            if state.conversation_id == Some(conversation_id) {
                return state
                    .pending_user_message
                    .clone()
                    .map(|pending| (pending, state.pending_user_message_started_at));
            }
        }
        None
    }

    /// Resolve an `(external_id, agent_type)` (agent session) to its
    /// currently-active connection id, if any. Sibling to
    /// `find_connection_by_conversation_id`, used as the discovery fallback for
    /// the cross-client viewer attach: a connection binds its `conversation_id`
    /// only on the first prompt, but its `external_id` is set as soon as the
    /// session starts — so for a historical conversation opened by a second
    /// client *before* anyone has sent a prompt, the by-conversation lookup
    /// misses while this one still finds the live owner, letting the second
    /// client attach as a viewer instead of reusing the connection as a
    /// (mis-tagged) owner and later tearing it down.
    ///
    /// `agent_type` is part of the match because `external_id` is unique only
    /// per agent (`UNIQUE(external_id, agent_type)`), not globally — without it,
    /// a session id shared across two agents could attach a viewer to the wrong
    /// agent's connection.
    pub async fn find_connection_by_external_id(
        &self,
        external_id: &str,
        agent_type: AgentType,
    ) -> Option<String> {
        let connections = self.connections.lock().await;
        for (id, conn) in connections.iter() {
            if conn.agent_type != agent_type {
                continue;
            }
            let state = conn.state.read().await;
            if state.external_id.as_deref() == Some(external_id) {
                return Some(id.clone());
            }
        }
        None
    }
}

/// Production impl of `ParentSessionLookup` for the delegation listener.
/// Resolves the parent's current `conversation_id` by reading its
/// `SessionState`. Bundled with `ConnectionManagerSpawner` here so the
/// concrete wiring lives next to the manager it depends on.
#[derive(Clone)]
pub struct ConnectionManagerParentLookup {
    pub manager: Arc<ConnectionManager>,
}

#[async_trait::async_trait]
impl crate::acp::delegation::listener::ParentSessionLookup for ConnectionManagerParentLookup {
    async fn current_conversation_id(&self, parent_connection_id: &str) -> Option<i32> {
        let state = self.manager.get_state(parent_connection_id).await?;
        let snapshot = state.read().await;
        snapshot.conversation_id
    }
}

/// Production impl of `SessionFeedbackAccess` for the delegation listener's
/// `check_user_feedback` arm. Resolves the parent connection's pending feedback
/// by delegating to `ConnectionManager::read_pending_feedback` /
/// `commit_feedback_delivered`. Mirrors
/// `ConnectionManagerParentLookup` so the listener stays unit-testable with an
/// in-memory stub.
#[derive(Clone)]
pub struct ConnectionManagerFeedbackLookup {
    pub manager: Arc<ConnectionManager>,
}

#[async_trait::async_trait]
impl SessionFeedbackAccess for ConnectionManagerFeedbackLookup {
    async fn read_pending_feedback(&self, parent_connection_id: &str) -> Vec<PendingFeedback> {
        self.manager
            .read_pending_feedback(parent_connection_id)
            .await
    }

    async fn commit_feedback_delivered(&self, parent_connection_id: &str, ids: Vec<String>) {
        self.manager
            .commit_feedback_delivered(parent_connection_id, ids)
            .await
    }
}

/// Production impl of `SessionQuestionAccess` for the delegation listener's
/// `ask_user_question` arm. Registers / cancels the parent connection's pending
/// question by delegating to `ConnectionManager`. Mirrors
/// `ConnectionManagerFeedbackLookup` so the listener stays unit-testable with an
/// in-memory stub.
#[derive(Clone)]
pub struct ConnectionManagerQuestionLookup {
    pub manager: Arc<ConnectionManager>,
}

#[async_trait::async_trait]
impl SessionQuestionAccess for ConnectionManagerQuestionLookup {
    async fn register_question(
        &self,
        parent_connection_id: &str,
        questions: Vec<QuestionSpec>,
    ) -> Option<RegisteredQuestion> {
        self.manager
            .register_question(parent_connection_id, questions)
            .await
    }

    async fn cancel_question(&self, parent_connection_id: &str, question_id: &str) {
        self.manager
            .cancel_question(parent_connection_id, question_id)
            .await
    }

    async fn cancel_questions_by_parent(&self, parent_connection_id: &str) {
        self.manager
            .cancel_questions_by_parent(parent_connection_id)
            .await
    }
}

/// Production impl of [`SessionPlanApprovalAccess`] for the Grok `exit_plan_mode`
/// ext bridge. Registers / cancels the parent connection's pending plan approval
/// by delegating to `ConnectionManager`. Mirrors `ConnectionManagerQuestionLookup`
/// so the connection handler stays unit-testable with an in-memory stub.
#[derive(Clone)]
pub struct ConnectionManagerPlanApprovalLookup {
    pub manager: Arc<ConnectionManager>,
}

#[async_trait::async_trait]
impl SessionPlanApprovalAccess for ConnectionManagerPlanApprovalLookup {
    async fn register_plan_approval(
        &self,
        parent_connection_id: &str,
        tool_call_id: String,
        plan_markdown: String,
    ) -> Option<RegisteredPlanApproval> {
        self.manager
            .register_plan_approval(parent_connection_id, tool_call_id, plan_markdown)
            .await
    }

    async fn cancel_plan_approvals_by_parent(&self, parent_connection_id: &str) {
        self.manager
            .cancel_plan_approvals_by_parent(parent_connection_id)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::connection::AgentConnection;
    use crate::acp::session_state::SessionState;
    use crate::acp::types::ConnectionStatus;
    use crate::web::event_bridge::{EventEmitter, WebEvent, WebEventBroadcaster};
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::{broadcast, mpsc, RwLock};

    #[test]
    fn is_reserved_turn_id_matches_only_the_parser_namespace() {
        // Rejected: the parsers' `turn-<digits>` ids (an untrusted client id of
        // this shape would collide with a persisted transcript turn).
        assert!(is_reserved_turn_id("turn-0"));
        assert!(is_reserved_turn_id("turn-42"));
        // Accepted: anything else, including the real UI sender id shape and the
        // connection-scoped fallback shape.
        assert!(!is_reserved_turn_id("optimistic-9f3c1a2b"));
        assert!(!is_reserved_turn_id("user-conn-7"));
        assert!(!is_reserved_turn_id("turn-")); // no number
        assert!(!is_reserved_turn_id("turn-1a")); // not all digits
        assert!(!is_reserved_turn_id("turnabout-1"));
        assert!(!is_reserved_turn_id(""));
    }

    fn fake_connection(id: &str, conv_id: Option<i32>) -> AgentConnection {
        let (tx, _rx) = mpsc::channel(1);
        let mut state = SessionState::new(
            id.to_string(),
            crate::models::agent::AgentType::ClaudeCode,
            None,
            "test-window".to_string(),
            None,
        );
        state.conversation_id = conv_id;
        state.status = ConnectionStatus::Connected;
        AgentConnection {
            id: id.to_string(),
            agent_type: crate::models::agent::AgentType::ClaudeCode,
            status: ConnectionStatus::Connected,
            owner_window_label: "test-window".to_string(),
            cmd_tx: tx,
            state: Arc::new(RwLock::new(state)),
            emitter: EventEmitter::Noop,
            prompt_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_fingerprint: String::new(),
            last_observed_fingerprint: String::new(),
            child_pid: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }
    }

    /// Spawn a two-level process tree: `sh` (the stand-in for the agent CLI)
    /// backgrounds a `sleep` grandchild (the stand-in for the agent's own
    /// children — an MCP server, a forked `node`) and records its pid. The
    /// grandchild is what the backstop assertions are about: it is the process
    /// that gets reparented and lingers when only the direct child is killed.
    ///
    /// Returns the direct child — keep it alive, dropping a `Child` does NOT
    /// kill it — and the grandchild pid.
    #[cfg(unix)]
    async fn spawn_process_tree(pidfile: &std::path::Path) -> (std::process::Child, i32) {
        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!(
                "sleep 30 & echo $! > '{}'; wait",
                pidfile.display()
            ))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn sh");
        for _ in 0..150 {
            if let Ok(raw) = std::fs::read_to_string(pidfile) {
                if let Ok(pid) = raw.trim().parse::<i32>() {
                    return (child, pid);
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        // Bail-out path still reaps the tree, so a failing test can't leave a
        // `sleep` behind for 30s.
        let _ = kill_tree::blocking::kill_tree(child.id());
        let _ = child.wait();
        panic!("grandchild never recorded its pid");
    }

    /// True once `pid` is gone. `kill(pid, 0)` sends no signal, it only probes
    /// existence; polling keeps the assertion from racing kernel teardown.
    #[cfg(unix)]
    async fn wait_until_dead(pid: i32) -> bool {
        for _ in 0..150 {
            // SAFETY: signal 0 only probes for existence; it sends no signal.
            if unsafe { libc::kill(pid, 0) } != 0 {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    #[cfg(unix)]
    fn is_alive(pid: i32) -> bool {
        // SAFETY: signal 0 only probes for existence; it sends no signal.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    /// Quit has to kill the agent's whole process TREE. Killing just the direct
    /// child leaves its children reparented and lingering — that orphan window
    /// is the entire reason the backstop exists.
    ///
    /// The test connection's command receiver is dropped, so the graceful path
    /// is unavailable and the kill can only come from the backstop.
    /// Unix-only (relies on `sh` / `kill(2)`).
    #[cfg(unix)]
    #[tokio::test]
    async fn disconnect_all_backstop_kills_the_whole_agent_process_tree() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (mut child, gpid) = spawn_process_tree(&dir.path().join("g.pid")).await;

        let mgr = ConnectionManager::new();
        let conn = fake_connection("conn-tree", None);
        conn.child_pid
            .store(child.id(), std::sync::atomic::Ordering::SeqCst);
        mgr.connections
            .lock()
            .await
            .insert("conn-tree".to_string(), conn);

        assert_eq!(mgr.disconnect_all().await, 1);
        assert!(
            wait_until_dead(gpid).await,
            "grandchild {gpid} survived — the quit backstop did not kill the tree"
        );
        let _ = child.wait();
    }

    /// A connection still `Connecting` when quit begins publishes its pid AFTER
    /// the map is drained. Reading the pids up front would see `0` there and
    /// skip it, leaking exactly the orphan this exists to kill — so the load
    /// has to happen after the grace window, from the live cell.
    #[cfg(unix)]
    #[tokio::test]
    async fn disconnect_all_backstop_reaches_a_child_that_spawns_during_the_grace_window() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (mut child, gpid) = spawn_process_tree(&dir.path().join("g.pid")).await;

        let mgr = ConnectionManager::new();
        let conn = fake_connection("conn-late", None);
        // Still 0 at drain time, exactly like a connection whose agent process
        // hasn't launched yet.
        let cell = Arc::clone(&conn.child_pid);
        mgr.connections
            .lock()
            .await
            .insert("conn-late".to_string(), conn);

        let pid = child.id();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            cell.store(pid, std::sync::atomic::Ordering::SeqCst);
        });

        assert_eq!(mgr.disconnect_all().await, 1);
        assert!(
            wait_until_dead(gpid).await,
            "grandchild {gpid} survived — a pid published during the grace window was missed"
        );
        let _ = child.wait();
    }

    /// The mirror image: once the agent process has been reaped, the `on_exit`
    /// callback zeroes the cell and the backstop must leave that pid alone.
    /// Without the clear, a quit fires `kill_tree` at a pid whose process is
    /// already dead and reaped — and if the OS recycled that number, the victim
    /// is an unrelated process tree.
    #[cfg(unix)]
    #[tokio::test]
    async fn disconnect_all_backstop_leaves_a_cleared_pid_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (mut child, gpid) = spawn_process_tree(&dir.path().join("g.pid")).await;

        let mgr = ConnectionManager::new();
        let conn = fake_connection("conn-cleared", None);
        conn.child_pid
            .store(child.id(), std::sync::atomic::Ordering::SeqCst);
        let cell = Arc::clone(&conn.child_pid);
        mgr.connections
            .lock()
            .await
            .insert("conn-cleared".to_string(), conn);

        // Stands in for the driver unwinding mid-window: the process is gone
        // and the guard has zeroed the cell.
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            cell.store(0, std::sync::atomic::Ordering::SeqCst);
        });

        assert_eq!(mgr.disconnect_all().await, 1);
        // Settle: a wrongly-issued SIGTERM would have landed by now.
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(
            is_alive(gpid),
            "backstop killed a tree whose pid the driver had already cleared"
        );

        let _ = kill_tree::blocking::kill_tree(child.id());
        let _ = child.wait();
    }

    /// Build a broadcaster + subscribed receiver. Subscribing here (not lazily
    /// inside the test) ensures events emitted between construction and the
    /// first `recv` are buffered rather than dropped.
    fn make_test_broadcaster() -> (Arc<WebEventBroadcaster>, broadcast::Receiver<WebEvent>) {
        let bcast = Arc::new(WebEventBroadcaster::new());
        let rx = bcast.subscribe();
        (bcast, rx)
    }

    /// Thin wrapper around `ConnectionManager::insert_test_connection` so the
    /// existing in-crate tests keep their `insert_fake_connection(mgr, ...)`
    /// call shape after the public test helper landed.
    async fn insert_fake_connection(
        mgr: &ConnectionManager,
        id: &str,
        agent_type: crate::models::agent::AgentType,
        working_dir: Option<PathBuf>,
        emitter: EventEmitter,
    ) {
        mgr.insert_test_connection(id, agent_type, working_dir, emitter)
            .await;
    }

    #[tokio::test]
    async fn refresh_connection_staleness_flags_only_drifted_running_sessions() {
        let mgr = ConnectionManager::new();
        // Test connections spawn with an empty fingerprint (insert_test_connection).
        insert_fake_connection(&mgr, "c1", AgentType::Codex, None, EventEmitter::Noop).await;
        // A different agent type that must stay untouched.
        insert_fake_connection(&mgr, "c2", AgentType::ClaudeCode, None, EventEmitter::Noop).await;

        // A real config change for Codex (fresh fp differs from the "" spawn fp).
        let mut fresh = HashMap::new();
        fresh.insert(AgentType::Codex, "codex-v2".to_string());
        let n = mgr
            .refresh_connection_staleness(&fresh, ConfigStaleKind::AgentConfig)
            .await;
        assert_eq!(n, 1, "only the Codex session is stale");
        assert!(
            mgr.get_state("c1").await.unwrap().read().await.config_stale,
            "Codex session flagged stale"
        );
        assert!(
            !mgr.get_state("c2").await.unwrap().read().await.config_stale,
            "ClaudeCode session untouched (agent not in the fresh set)"
        );

        // Re-running with the SAME fingerprint keeps it stale but is idempotent.
        let n2 = mgr
            .refresh_connection_staleness(&fresh, ConfigStaleKind::AgentConfig)
            .await;
        assert_eq!(n2, 1);

        // Reverting Codex back to its spawn fingerprint ("") clears staleness.
        let mut reverted = HashMap::new();
        reverted.insert(AgentType::Codex, String::new());
        let n3 = mgr
            .refresh_connection_staleness(&reverted, ConfigStaleKind::AgentConfig)
            .await;
        assert_eq!(n3, 0, "reverted config is no longer stale");
        assert!(
            !mgr.get_state("c1").await.unwrap().read().await.config_stale,
            "staleness cleared after revert"
        );
    }

    /// Subscribe directly to the per-connection event stream. Phase 4b
    /// removed the dual-broadcast through the global `WebEventBroadcaster`
    /// for ACP events; the per-connection stream is now the only delivery
    /// path tests can observe. Subscribe BEFORE triggering the producing
    /// call so events emitted between subscribe and recv buffer rather
    /// than drop.
    async fn subscribe_conn_stream(
        mgr: &ConnectionManager,
        conn_id: &str,
    ) -> broadcast::Receiver<std::sync::Arc<crate::acp::types::EventEnvelope>> {
        let state = mgr
            .get_state(conn_id)
            .await
            .expect("connection should be registered");
        let stream = state.read().await.event_stream();
        stream.subscribe()
    }

    /// Receive the first envelope from a per-connection stream. Times out
    /// after 200 ms to keep tests honest.
    async fn recv_first_acp_event(
        rx: &mut broadcast::Receiver<std::sync::Arc<crate::acp::types::EventEnvelope>>,
    ) -> crate::acp::types::EventEnvelope {
        let evt = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
            .await
            .expect("timed out waiting for acp event")
            .expect("per-connection stream closed");
        (*evt).clone()
    }

    /// Drain the connection's command receiver (non-blocking) and return one
    /// entry per enqueued `Prompt` command: its attached `user_message` payload
    /// (the cross-client broadcast the loop emits before the agent request),
    /// flattened to `(message_id, text blocks)`. The inner `Option` is `None`
    /// for a `Prompt` carrying no user message (delegation child / unbound).
    /// The vec length is the number of `Prompt` commands enqueued — useful for
    /// asserting the concurrency gate stopped a second one. Call after the
    /// producing await.
    fn drain_prompt_user_messages(
        cmd_rx: &mut mpsc::Receiver<crate::acp::connection::ConnectionCommand>,
    ) -> Vec<Option<(String, Vec<String>)>> {
        let mut out = Vec::new();
        while let Ok(cmd) = cmd_rx.try_recv() {
            if let crate::acp::connection::ConnectionCommand::Prompt { user_message, .. } = cmd {
                out.push(user_message.map(|(id, blocks)| {
                    let texts = blocks
                        .iter()
                        .filter_map(|b| match b {
                            crate::acp::types::UserMessageBlock::Text { text } => {
                                Some(text.clone())
                            }
                            _ => None,
                        })
                        .collect::<Vec<String>>();
                    (id, texts)
                }));
            }
        }
        out
    }

    /// A minimal non-empty prompt for tests that exercise linking / status /
    /// caller-id behavior and don't care about the prompt content. (Empty
    /// prompts are now rejected before any side effects, so these tests must
    /// pass real content to reach the link path.)
    fn one_text_block() -> Vec<PromptInputBlock> {
        vec![PromptInputBlock::Text {
            text: "test prompt".into(),
        }]
    }

    /// Insert a connection with a LIVE command receiver so `send_prompt_inner`'s
    /// enqueue SUCCEEDS (the UserMessage broadcast is deferred until after a
    /// successful enqueue). Returns the receiver — keep it in scope for the
    /// test, otherwise the channel closes and the send fails.
    async fn insert_live_connection(
        mgr: &ConnectionManager,
        conn_id: &str,
        agent_type: AgentType,
        working_dir: Option<PathBuf>,
    ) -> tokio::sync::mpsc::Receiver<crate::acp::connection::ConnectionCommand> {
        use crate::acp::connection::AgentConnection;
        use crate::acp::session_state::SessionState;
        let (tx, rx) = mpsc::channel::<crate::acp::connection::ConnectionCommand>(4);
        let mut state = SessionState::new(
            conn_id.to_string(),
            agent_type,
            working_dir,
            "test-window".to_string(),
            None,
        );
        state.status = ConnectionStatus::Connected;
        let conn = AgentConnection {
            id: conn_id.to_string(),
            agent_type,
            status: ConnectionStatus::Connected,
            owner_window_label: "test-window".to_string(),
            cmd_tx: tx,
            state: Arc::new(RwLock::new(state)),
            emitter: EventEmitter::Noop,
            prompt_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_fingerprint: String::new(),
            last_observed_fingerprint: String::new(),
            child_pid: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        };
        mgr.connections
            .lock()
            .await
            .insert(conn_id.to_string(), conn);
        rx
    }

    #[tokio::test]
    async fn send_prompt_linked_attaches_user_message_to_prompt_for_root() {
        // A root send attaches the projected user-message payload to the
        // enqueued Prompt command (the connection loop emits the UserMessage
        // event itself, ordered before the agent request). With a live receiver
        // the enqueue succeeds and the payload is observable on the command.
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/um-root").await;
        let mgr = ConnectionManager::new();
        let conn_id = "conn-um-root";
        let mut cmd_rx = insert_live_connection(
            &mgr,
            conn_id,
            AgentType::ClaudeCode,
            Some(PathBuf::from("/tmp/um-root")),
        )
        .await;

        let result = mgr
            .send_prompt_linked(
                &db,
                conn_id,
                vec![PromptInputBlock::Text {
                    text: "hello viewers".into(),
                }],
                Some(folder_id),
                None,
            )
            .await;
        assert!(
            result.is_ok(),
            "enqueue should succeed with a live receiver"
        );

        let prompts = drain_prompt_user_messages(&mut cmd_rx);
        assert_eq!(prompts.len(), 1, "exactly one Prompt enqueued");
        let um = prompts[0]
            .as_ref()
            .expect("root Prompt carries a user_message");
        assert!(
            um.0.starts_with("user-"),
            "connection-scoped id fallback, got {:?}",
            um.0
        );
        assert!(
            um.1.iter().any(|t| t == "hello viewers"),
            "user_message must carry the prompt text, got {um:?}"
        );
    }

    #[tokio::test]
    async fn send_prompt_linked_rejects_second_prompt_while_turn_in_flight() {
        // Two clients co-controlling one connection can send near-
        // simultaneously. The first accepted prompt marks the turn in flight;
        // the second must be REJECTED with TurnInProgress (not enqueued behind
        // the active turn and silently dropped by the loop) so the frontend can
        // re-queue it. Only one Prompt reaches the connection loop.
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/um-gate").await;
        let mgr = ConnectionManager::new();
        let conn_id = "conn-um-gate";
        let mut cmd_rx = insert_live_connection(
            &mgr,
            conn_id,
            AgentType::ClaudeCode,
            Some(PathBuf::from("/tmp/um-gate")),
        )
        .await;

        let first = mgr
            .send_prompt_linked(
                &db,
                conn_id,
                vec![PromptInputBlock::Text {
                    text: "first".into(),
                }],
                Some(folder_id),
                None,
            )
            .await;
        assert!(first.is_ok(), "first prompt accepted");

        let second = mgr
            .send_prompt_linked(
                &db,
                conn_id,
                vec![PromptInputBlock::Text {
                    text: "second".into(),
                }],
                Some(folder_id),
                None,
            )
            .await;
        assert!(
            matches!(second, Err(AcpError::TurnInProgress)),
            "second concurrent prompt must be rejected with TurnInProgress, got {second:?}"
        );

        let prompts = drain_prompt_user_messages(&mut cmd_rx);
        assert_eq!(
            prompts.len(),
            1,
            "only the first prompt reaches the loop; the second is rejected, not queued"
        );
    }

    #[tokio::test]
    async fn send_prompt_linked_rejects_empty_prompt_without_wedging_gate() {
        // An empty prompt is rejected BEFORE any side effects: it must NOT
        // create/link a conversation row, must NOT set the concurrency gate
        // (which — with no TurnComplete to clear it — would 409 every future
        // send), and the connection must stay usable for a real prompt.
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/um-empty").await;
        let mgr = ConnectionManager::new();
        let conn_id = "conn-um-empty";
        let mut cmd_rx = insert_live_connection(
            &mgr,
            conn_id,
            AgentType::ClaudeCode,
            Some(PathBuf::from("/tmp/um-empty")),
        )
        .await;

        let rows_before = count_conversation_rows(&db).await;
        let empty = mgr
            .send_prompt_linked(&db, conn_id, vec![], Some(folder_id), None)
            .await;
        assert!(empty.is_err(), "an empty prompt must be rejected");
        assert_eq!(
            count_conversation_rows(&db).await,
            rows_before,
            "a rejected empty prompt must NOT create/link a conversation row"
        );
        assert!(
            !mgr.get_state(conn_id)
                .await
                .unwrap()
                .read()
                .await
                .turn_in_flight,
            "a rejected empty prompt must NOT set the concurrency gate"
        );

        // The connection is not wedged: a real prompt afterwards is accepted.
        let ok = mgr
            .send_prompt_linked(
                &db,
                conn_id,
                vec![PromptInputBlock::Text { text: "hi".into() }],
                Some(folder_id),
                None,
            )
            .await;
        assert!(
            ok.is_ok(),
            "a real prompt after an empty one must still be accepted"
        );
        assert_eq!(
            drain_prompt_user_messages(&mut cmd_rx).len(),
            1,
            "exactly the one real prompt reached the loop"
        );
    }

    #[tokio::test]
    async fn send_prompt_returns_turn_in_progress_when_busy() {
        // The non-linked `send_prompt` path (used by the chat channel) must
        // surface `TurnInProgress` — NOT a connection-loss error — when a turn
        // is already in flight, so the chat channel treats it as a transient
        // busy rejection instead of tearing down the session.
        let mgr = ConnectionManager::new();
        let conn_id = "conn-busy";
        let _rx = insert_live_connection(&mgr, conn_id, AgentType::ClaudeCode, None).await;
        mgr.get_state(conn_id)
            .await
            .unwrap()
            .write()
            .await
            .turn_in_flight = true;

        let res = mgr
            .send_prompt(conn_id, vec![PromptInputBlock::Text { text: "hi".into() }])
            .await;
        assert!(
            matches!(res, Err(AcpError::TurnInProgress)),
            "send_prompt must return TurnInProgress when a turn is in flight, got {res:?}"
        );
    }

    #[tokio::test]
    async fn queue_aware_prompt_cannot_jump_a_durable_followup() {
        use crate::db::service::prompt_queue_service;
        use crate::db::test_helpers;
        use crate::models::{EnqueuePromptQueueItem, PromptQueueDraft};

        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/queue-aware-chat").await;
        let conversation_id =
            test_helpers::seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        prompt_queue_service::enqueue(
            &db.conn,
            EnqueuePromptQueueItem {
                conversation_id,
                id: "queued-first".into(),
                client_dedupe_id: "queued-first".into(),
                draft: PromptQueueDraft {
                    blocks: vec![PromptInputBlock::Text {
                        text: "first".into(),
                    }],
                    display_text: "first".into(),
                },
                mode_id: None,
            },
        )
        .await
        .expect("enqueue");

        let mgr = ConnectionManager::new();
        let conn_id = "conn-queue-aware";
        let mut rx = insert_live_connection(&mgr, conn_id, AgentType::ClaudeCode, None).await;
        let state = mgr.get_state(conn_id).await.expect("state");
        state.write().await.conversation_id = Some(conversation_id);

        let result = mgr
            .send_prompt_queue_aware(
                &db.conn,
                conn_id,
                vec![PromptInputBlock::Text {
                    text: "must wait".into(),
                }],
            )
            .await;
        assert!(matches!(result, Err(AcpError::TurnInProgress)));
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn managed_chat_ingress_attaches_store_only_mail_through_generic_acp() {
        use crate::db::test_helpers;
        use crate::models::{
            CollaborationDeliveryHint, CollaborationDeliveryState, CollaborationInvocationPolicy,
            CollaborationUrgency, SendCollaborationMessageInput,
        };

        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/chat-store-only").await;
        let source = test_helpers::seed_conversation(&db, folder_id, AgentType::OpenCode).await;
        let target = test_helpers::seed_conversation(&db, folder_id, AgentType::Gemini).await;
        let sent = collaboration_service::send(
            &db.conn,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                body: "mail from another Harness".into(),
                client_dedupe_id: "chat-store-only".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .unwrap();
        let mgr = ConnectionManager::new();
        let conn_id = "conn-chat-store-only";
        let mut commands = insert_live_connection(&mgr, conn_id, AgentType::Gemini, None).await;
        mgr.get_state(conn_id)
            .await
            .unwrap()
            .write()
            .await
            .conversation_id = Some(target);

        mgr.send_prompt_queue_aware(
            &db.conn,
            conn_id,
            vec![PromptInputBlock::Text {
                text: "message from managed chat".into(),
            }],
        )
        .await
        .unwrap();
        let crate::acp::connection::ConnectionCommand::Prompt {
            blocks,
            user_message,
        } = commands.try_recv().unwrap()
        else {
            panic!("expected prompt command");
        };
        assert!(user_message.is_none());
        let text = blocks
            .iter()
            .filter_map(|block| match block {
                PromptInputBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(text[0].contains("mail from another Harness"));
        assert_eq!(text[1], "message from managed chat");
        let delivery = collaboration_service::feed(&db.conn, target, None)
            .await
            .unwrap()
            .inbound
            .into_iter()
            .find(|delivery| delivery.event_id == sent.event_id)
            .unwrap();
        assert_eq!(delivery.state, CollaborationDeliveryState::Embedded);
        assert!(delivery
            .embedded_turn_ref
            .as_deref()
            .is_some_and(|turn_ref| turn_ref.starts_with("host-")));
    }

    #[tokio::test]
    async fn cancel_is_ordered_after_a_prompt_that_crossed_admission() {
        use crate::db::test_helpers;

        let db = test_helpers::fresh_in_memory_db().await;
        let mgr = Arc::new(ConnectionManager::new());
        let conn_id = "conn-cancel-order";
        let mut commands = insert_live_connection(&mgr, conn_id, AgentType::ClaudeCode, None).await;
        let prompt_lock = mgr.clone_prompt_lock(conn_id).await.expect("prompt lock");
        let guard = prompt_lock.lock_owned().await;

        let cancel = {
            let mgr = Arc::clone(&mgr);
            let conn = db.conn.clone();
            tokio::spawn(async move { mgr.cancel(&conn, conn_id).await })
        };
        tokio::task::yield_now().await;
        assert!(
            commands.try_recv().is_err(),
            "Cancel must wait behind the prompt admission lock"
        );

        mgr.send_prompt_inner(
            conn_id,
            vec![PromptInputBlock::Text {
                text: "already admitted".into(),
            }],
            None,
        )
        .await
        .expect("enqueue admitted prompt");
        drop(guard);
        cancel.await.expect("cancel task").expect("cancel");

        assert!(matches!(
            commands.recv().await,
            Some(ConnectionCommand::Prompt { .. })
        ));
        assert!(matches!(
            commands.recv().await,
            Some(ConnectionCommand::Cancel)
        ));
    }

    #[tokio::test]
    async fn fork_session_rejects_when_turn_in_flight() {
        // A fork re-points the live session; it must not run while a turn is in
        // flight (a racing send would route to the wrong session, and the Fork
        // command would be dropped by the in-turn loop). It rejects with
        // TurnInProgress so the caller re-queues, WITHOUT enqueuing a Fork.
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let mgr = ConnectionManager::new();
        let conn_id = "conn-fork-busy";
        let mut cmd_rx = insert_live_connection(&mgr, conn_id, AgentType::ClaudeCode, None).await;
        {
            let state = mgr.get_state(conn_id).await.unwrap();
            let mut s = state.write().await;
            s.conversation_id = Some(7); // fork requires a linked row
            s.turn_in_flight = true; // a turn is already running
        }

        let res = mgr.fork_session(&db, conn_id, None, None).await;
        assert!(
            matches!(res, Err(AcpError::TurnInProgress)),
            "fork must reject with TurnInProgress while a turn is in flight, got {res:?}"
        );
        assert!(
            cmd_rx.try_recv().is_err(),
            "a rejected fork must NOT enqueue a Fork command"
        );
    }

    #[tokio::test]
    async fn fork_session_failure_leaves_gate_clear_and_lock_free() {
        // A fork holds `prompt_lock` for its whole critical section and never
        // SETS `turn_in_flight`, so even when the fork FAILS (here: a dead
        // command receiver makes the `Fork` send error) the connection isn't
        // wedged — the gate stays clear and the prompt lock is released on the
        // error path. (A fork emits no TurnComplete, so a gate it had set would
        // have had nothing to clear it.)
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let mgr = ConnectionManager::new();
        let conn_id = "conn-fork-fail";
        // insert_fake_connection drops the cmd receiver → the Fork send fails.
        insert_fake_connection(
            &mgr,
            conn_id,
            AgentType::ClaudeCode,
            None,
            EventEmitter::Noop,
        )
        .await;
        mgr.get_state(conn_id)
            .await
            .unwrap()
            .write()
            .await
            .conversation_id = Some(9);

        let res = mgr.fork_session(&db, conn_id, None, None).await;
        assert!(res.is_err(), "fork with a dead receiver must fail");
        assert!(
            !mgr.get_state(conn_id)
                .await
                .unwrap()
                .read()
                .await
                .turn_in_flight,
            "a failed fork must leave the gate clear"
        );
        let lock = mgr.clone_prompt_lock(conn_id).await.unwrap();
        assert!(
            lock.try_lock().is_ok(),
            "a failed fork must release prompt_lock so the connection stays usable"
        );
    }

    #[tokio::test]
    async fn fork_persists_despite_caller_cancellation() {
        // Cancellation-shield regression. Once `fork_session` enqueues the `Fork`
        // command, the connection loop re-points the live session to S2 and emits
        // `SessionStarted{S2}` REGARDLESS of caller liveness (it ignores a dead
        // reply channel). So the DB persistence that records the two-row layout
        // must NOT be tied to the caller's future. We drop the caller before the
        // provider reply, then assert immutable C1/S1 plus durable C2/S2 still
        // commit and the activation decision still reaches the connection loop.
        use crate::acp::connection::ConnectionCommand;
        use crate::db::test_helpers;
        use sea_orm::EntityTrait;

        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/fork-shield").await;
        let pre = conversation_service::create(
            &db.conn,
            folder_id,
            AgentType::ClaudeCode,
            Some("Topic".into()),
            None,
        )
        .await
        .unwrap();
        conversation_service::update_external_id(&db.conn, pre.id, "session-S1".into())
            .await
            .unwrap();

        // Connection with a GATED fake fork reply: withheld until `go_tx` fires,
        // so we can drop the caller before the reply (and thus the persistence).
        let (tx, mut rx) = mpsc::channel::<ConnectionCommand>(4);
        let mut state = SessionState::new(
            "c-shield".to_string(),
            AgentType::ClaudeCode,
            None,
            "test-window".to_string(),
            None,
        );
        state.conversation_id = Some(pre.id);
        state.status = ConnectionStatus::Connected;
        let conn = AgentConnection {
            id: "c-shield".to_string(),
            agent_type: AgentType::ClaudeCode,
            status: ConnectionStatus::Connected,
            owner_window_label: "test-window".to_string(),
            cmd_tx: tx,
            state: Arc::new(RwLock::new(state)),
            emitter: EventEmitter::Noop,
            prompt_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_fingerprint: String::new(),
            last_observed_fingerprint: String::new(),
            child_pid: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        };
        let mgr = ConnectionManager::new();
        mgr.connections
            .lock()
            .await
            .insert("c-shield".to_string(), conn);

        let (go_tx, go_rx) = tokio::sync::oneshot::channel::<()>();
        let fake_loop = tokio::spawn(async move {
            if let Some(ConnectionCommand::Fork { reply, activation }) = rx.recv().await {
                go_rx.await.ok(); // withhold the reply until the test releases it
                let _ = reply.send(Ok(crate::acp::types::ForkProtocolResult {
                    forked_session_id: "session-S2".into(),
                    original_session_id: "session-S1".into(),
                }));
                if let Ok(ForkActivation::Commit { ack, .. }) = activation.await {
                    let _ = ack.send(Ok(()));
                }
            }
            rx // keep the receiver alive
        });

        // Drive fork under a short timeout: it spawns the shielded task (which
        // enqueues `Fork` and blocks on the withheld reply), then the timeout
        // DROPS this caller future. The detached persistence task must survive.
        let timed = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            mgr.fork_session(&db, "c-shield", None, None),
        )
        .await;
        assert!(
            timed.is_err(),
            "caller must be dropped before the withheld reply is delivered"
        );

        // Nothing persisted yet (reply still withheld) — the row is untouched.
        let mid = conversation_service::get_by_id(&db.conn, pre.id)
            .await
            .unwrap();
        assert_eq!(
            mid.external_id.as_deref(),
            Some("session-S1"),
            "fork must not persist before the protocol reply"
        );

        // Release the reply: the DETACHED task completes the persistence even
        // though the caller is long gone.
        go_tx.send(()).ok();
        let _ = fake_loop.await;

        // Poll (bounded) until immutable C1 + new C2 appears.
        let mut persisted = false;
        for _ in 0..200 {
            let current = conversation_service::get_by_id(&db.conn, pre.id)
                .await
                .unwrap();
            let rows = conversation::Entity::find().all(&db.conn).await.unwrap();
            let has_fork = rows.iter().any(|r| {
                r.id != pre.id
                    && r.external_id.as_deref() == Some("session-S2")
                    && r.title.as_deref() == Some("[Fork] Topic")
            });
            if current.external_id.as_deref() == Some("session-S1")
                && current.title.as_deref() == Some("Topic")
                && has_fork
            {
                persisted = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(
            persisted,
            "fork persistence must complete despite caller cancellation"
        );
    }

    #[tokio::test]
    async fn send_prompt_inner_does_not_set_gate_while_blocked_on_capacity() {
        // Cancellation-safety: the gate is set only AFTER reserving channel
        // capacity, with no await between the set and the send. If the future is
        // dropped while awaiting capacity (channel full), `turn_in_flight` must
        // remain false — otherwise a cancelled send would wedge the connection.
        let mgr = ConnectionManager::new();
        let conn_id = "conn-cancel";
        let _rx = insert_live_connection(&mgr, conn_id, AgentType::ClaudeCode, None).await;
        // Fill the command channel to capacity (4, per insert_live_connection)
        // by sending DIRECTLY on the cloned sender — bypassing the gate — so the
        // next reserve() blocks.
        let tx = mgr
            .connections
            .lock()
            .await
            .get(conn_id)
            .unwrap()
            .cmd_tx
            .clone();
        for _ in 0..4 {
            tx.send(crate::acp::connection::ConnectionCommand::Prompt {
                blocks: vec![PromptInputBlock::Text {
                    text: "filler".into(),
                }],
                user_message: None,
            })
            .await
            .unwrap();
        }

        // send_prompt_inner now blocks on reserve(); drop it via a short timeout.
        let fut = mgr.send_prompt_inner(
            conn_id,
            vec![PromptInputBlock::Text {
                text: "blocked".into(),
            }],
            None,
        );
        let res = tokio::time::timeout(std::time::Duration::from_millis(50), fut).await;
        assert!(
            res.is_err(),
            "send_prompt_inner should still be blocked on channel capacity"
        );
        assert!(
            !mgr.get_state(conn_id)
                .await
                .unwrap()
                .read()
                .await
                .turn_in_flight,
            "the gate must NOT be set while blocked on channel capacity (cancellation-safe)"
        );
    }

    #[tokio::test]
    async fn send_prompt_linked_uses_client_message_id_for_user_message() {
        // The UI threads its optimistic turn id as `client_message_id`; the
        // broadcast UserMessage must carry it verbatim so the sender dedups its
        // own echo by exact id (not a heuristic).
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/um-cmid").await;
        let mgr = ConnectionManager::new();
        let conn_id = "conn-um-cmid";
        let mut cmd_rx = insert_live_connection(
            &mgr,
            conn_id,
            AgentType::ClaudeCode,
            Some(PathBuf::from("/tmp/um-cmid")),
        )
        .await;

        mgr.send_prompt_linked_with_message_id(
            &db,
            conn_id,
            vec![PromptInputBlock::Text { text: "hi".into() }],
            Some(folder_id),
            None,
            Some("optimistic-abc".to_string()),
        )
        .await
        .expect("send");

        let prompts = drain_prompt_user_messages(&mut cmd_rx);
        assert_eq!(
            prompts
                .first()
                .and_then(|um| um.as_ref())
                .map(|(id, _)| id.as_str()),
            Some("optimistic-abc"),
            "Prompt's user_message must carry the client-supplied message_id verbatim"
        );
    }

    #[tokio::test]
    async fn send_prompt_linked_rejects_a_stale_c1_view_after_fork_handoff() {
        use crate::db::test_helpers;

        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/fork-stale-view").await;
        let c1 = test_helpers::seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let c2 = test_helpers::seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let mgr = ConnectionManager::new();
        let conn_id = "conn-forked-to-c2";
        let mut cmd_rx = insert_live_connection(
            &mgr,
            conn_id,
            AgentType::ClaudeCode,
            Some(PathBuf::from("/tmp/fork-stale-view")),
        )
        .await;
        mgr.get_state(conn_id)
            .await
            .unwrap()
            .write()
            .await
            .conversation_id = Some(c2);

        let error = mgr
            .send_prompt_linked_with_message_id(
                &db,
                conn_id,
                vec![PromptInputBlock::Text {
                    text: "must stay on C1".into(),
                }],
                Some(folder_id),
                Some(c1),
                Some("stale-c1-view".into()),
            )
            .await
            .expect_err("a stale C1 View must not send through C2's live connection");

        assert!(error.to_string().contains("linked to conversation"));
        assert!(cmd_rx.try_recv().is_err(), "no prompt may reach C2");
    }

    #[tokio::test]
    async fn natural_turn_attaches_cross_harness_mail_without_forging_the_user_message() {
        use crate::db::test_helpers;
        use crate::models::{
            CollaborationDeliveryHint, CollaborationDeliveryState, CollaborationInvocationPolicy,
            CollaborationUrgency, SendCollaborationMessageInput,
        };

        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/um-collaboration").await;
        let source = test_helpers::seed_conversation(&db, folder_id, AgentType::Codex).await;
        let target = test_helpers::seed_conversation(&db, folder_id, AgentType::Gemini).await;
        let sent = collaboration_service::send(
            &db.conn,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                body: "compare this evidence".to_string(),
                client_dedupe_id: "cross-harness-natural-turn".to_string(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: true,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .unwrap();

        let mgr = ConnectionManager::new();
        let conn_id = "conn-um-collaboration";
        let mut cmd_rx = insert_live_connection(
            &mgr,
            conn_id,
            AgentType::Gemini,
            Some(PathBuf::from("/tmp/um-collaboration")),
        )
        .await;
        mgr.send_prompt_linked_with_message_id(
            &db,
            conn_id,
            vec![PromptInputBlock::Text {
                text: "my own question".into(),
            }],
            Some(folder_id),
            Some(target),
            Some("optimistic-cross-harness".to_string()),
        )
        .await
        .expect("natural Gemini turn should be accepted through generic ACP");

        let crate::acp::connection::ConnectionCommand::Prompt {
            blocks,
            user_message,
        } = cmd_rx.try_recv().expect("one prompt command")
        else {
            panic!("expected a prompt command");
        };
        let harness_text = blocks
            .iter()
            .filter_map(|block| match block {
                PromptInputBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(harness_text.len(), 2);
        assert!(harness_text[0].contains("compare this evidence"));
        assert_eq!(harness_text[1], "my own question");

        let (message_id, visible_blocks) = user_message.expect("viewer user message");
        assert_eq!(message_id, "optimistic-cross-harness");
        assert_eq!(visible_blocks.len(), 1);
        assert!(matches!(
            &visible_blocks[0],
            crate::acp::types::UserMessageBlock::Text { text } if text == "my own question"
        ));

        let delivery = collaboration_service::feed(&db.conn, target, None)
            .await
            .unwrap()
            .inbound
            .into_iter()
            .find(|delivery| delivery.event_id == sent.event_id)
            .unwrap();
        assert_eq!(delivery.state, CollaborationDeliveryState::Embedded);
        assert_eq!(
            delivery.embedded_turn_ref.as_deref(),
            Some("optimistic-cross-harness")
        );
    }

    #[tokio::test]
    async fn failed_natural_turn_restores_attached_mail_to_pending() {
        use crate::db::test_helpers;
        use crate::models::{
            CollaborationDeliveryHint, CollaborationDeliveryState, CollaborationInvocationPolicy,
            CollaborationUrgency, SendCollaborationMessageInput,
        };

        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/um-collaboration-fail").await;
        let source = test_helpers::seed_conversation(&db, folder_id, AgentType::OpenCode).await;
        let target = test_helpers::seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let sent = collaboration_service::send(
            &db.conn,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![target],
                body: "retry after a known failure".to_string(),
                client_dedupe_id: "cross-harness-failed-turn".to_string(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: false,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .unwrap();
        let mgr = ConnectionManager::new();
        let conn_id = "conn-um-collaboration-fail";
        insert_fake_connection(
            &mgr,
            conn_id,
            AgentType::ClaudeCode,
            Some(PathBuf::from("/tmp/um-collaboration-fail")),
            EventEmitter::Noop,
        )
        .await;

        assert!(mgr
            .send_prompt_linked_with_message_id(
                &db,
                conn_id,
                vec![PromptInputBlock::Text {
                    text: "this command cannot be enqueued".into(),
                }],
                Some(folder_id),
                Some(target),
                Some("optimistic-failed-cross-harness".to_string()),
            )
            .await
            .is_err());
        let delivery = collaboration_service::feed(&db.conn, target, None)
            .await
            .unwrap()
            .inbound
            .into_iter()
            .find(|delivery| delivery.event_id == sent.event_id)
            .unwrap();
        assert_eq!(delivery.state, CollaborationDeliveryState::Pending);
        assert!(delivery.embedded_turn_ref.is_none());
        assert_eq!(delivery.attempts, 1);
    }

    #[tokio::test]
    async fn send_prompt_linked_failed_reserve_leaves_gate_clear() {
        // A failed enqueue (dropped cmd receiver) fails at the channel
        // `reserve()` step — which is BEFORE the turn-in-flight gate is set — so
        // the gate is never set, not "rolled back". The connection must stay
        // usable (turn_in_flight false), and the row rolls back to Cancelled.
        // pending_user_message stays None (the loop, which never ran, owns it).
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/um-fail").await;
        let mgr = ConnectionManager::new();
        let conn_id = "conn-um-fail";
        // insert_fake_connection drops the cmd receiver → send_prompt_inner fails.
        insert_fake_connection(
            &mgr,
            conn_id,
            AgentType::ClaudeCode,
            Some(PathBuf::from("/tmp/um-fail")),
            EventEmitter::Noop,
        )
        .await;

        let result = mgr
            .send_prompt_linked(
                &db,
                conn_id,
                vec![PromptInputBlock::Text {
                    text: "never enqueued".into(),
                }],
                Some(folder_id),
                None,
            )
            .await;
        assert!(result.is_err(), "a dropped receiver must fail the enqueue");

        let state = mgr.get_state(conn_id).await.unwrap();
        let snap = state.read().await;
        assert!(
            !snap.turn_in_flight,
            "a failed enqueue must roll back turn_in_flight so the connection isn't wedged"
        );
        let pending = snap.pending_user_message.clone();
        assert!(
            pending.is_none(),
            "a failed enqueue must not strand pending_user_message"
        );
    }

    #[test]
    fn user_prompt_text_preview_joins_and_trims_text_blocks() {
        let blocks = vec![
            PromptInputBlock::Text {
                text: "  hello  ".into(),
            },
            PromptInputBlock::Text {
                text: "world".into(),
            },
        ];
        assert_eq!(
            user_prompt_text_preview(&blocks).as_deref(),
            Some("hello world")
        );
    }

    #[test]
    fn user_prompt_text_preview_is_none_for_empty_or_textless() {
        assert!(user_prompt_text_preview(&[]).is_none());
        assert!(
            user_prompt_text_preview(&[PromptInputBlock::Text { text: "   ".into() }]).is_none()
        );
        let img = vec![PromptInputBlock::Image {
            data: "x".into(),
            mime_type: "image/png".into(),
            uri: None,
        }];
        assert!(user_prompt_text_preview(&img).is_none());
    }

    #[test]
    fn user_prompt_text_preview_truncates_long_input() {
        let long = "a".repeat(USER_PROMPT_PREVIEW_MAX_CHARS + 50);
        let preview = user_prompt_text_preview(&[PromptInputBlock::Text { text: long }]).unwrap();
        // truncate_str keeps MAX chars then appends a 3-char "..." marker.
        assert_eq!(preview.chars().count(), USER_PROMPT_PREVIEW_MAX_CHARS + 3);
        assert!(preview.ends_with("..."));
    }

    /// A successful UI send (delegation = None, text present) emits
    /// `UserPromptSent` carrying the message preview, after the link + status
    /// events.
    #[tokio::test]
    async fn send_prompt_linked_emits_user_prompt_sent_on_success() {
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/ups").await;
        let mgr = ConnectionManager::new();
        let conn_id = "conn-ups-1";
        let _rx = insert_live_connection(
            &mgr,
            conn_id,
            AgentType::ClaudeCode,
            Some(PathBuf::from("/tmp/ups")),
        )
        .await;
        let mut stream = subscribe_conn_stream(&mgr, conn_id).await;

        mgr.send_prompt_linked(
            &db,
            conn_id,
            vec![PromptInputBlock::Text {
                text: "hello world".into(),
            }],
            Some(folder_id),
            None,
        )
        .await
        .expect("send should succeed with a live receiver");

        let mut found = None;
        for _ in 0..5 {
            let env = recv_first_acp_event(&mut stream).await;
            if let AcpEvent::UserPromptSent { text_preview } = env.payload {
                found = Some(text_preview);
                break;
            }
        }
        assert_eq!(found.as_deref(), Some("hello world"));
    }

    /// A textless prompt (image-only) succeeds but emits NO `UserPromptSent` —
    /// the notification fires for text messages only.
    #[tokio::test]
    async fn send_prompt_linked_skips_user_prompt_sent_for_textless_prompt() {
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/ups2").await;
        let mgr = ConnectionManager::new();
        let conn_id = "conn-ups-2";
        let _rx = insert_live_connection(
            &mgr,
            conn_id,
            AgentType::ClaudeCode,
            Some(PathBuf::from("/tmp/ups2")),
        )
        .await;
        let mut stream = subscribe_conn_stream(&mgr, conn_id).await;

        mgr.send_prompt_linked(
            &db,
            conn_id,
            vec![PromptInputBlock::Image {
                data: "deadbeef".into(),
                mime_type: "image/png".into(),
                uri: None,
            }],
            Some(folder_id),
            None,
        )
        .await
        .expect("send should succeed with a live receiver");

        let mut saw_user_prompt = false;
        for _ in 0..4 {
            match tokio::time::timeout(std::time::Duration::from_millis(100), stream.recv()).await {
                Ok(Ok(env)) => {
                    if matches!(env.payload, AcpEvent::UserPromptSent { .. }) {
                        saw_user_prompt = true;
                    }
                }
                _ => break,
            }
        }
        assert!(
            !saw_user_prompt,
            "a textless (image-only) prompt must not emit UserPromptSent"
        );
    }

    #[tokio::test]
    async fn get_state_returns_arc_for_known_connection() {
        let mgr = ConnectionManager::new();
        {
            let mut map = mgr.connections.lock().await;
            map.insert("c1".to_string(), fake_connection("c1", None));
        }
        let state = mgr.get_state("c1").await.expect("state should be found");
        assert_eq!(state.read().await.connection_id, "c1");
    }

    #[tokio::test]
    async fn get_state_returns_none_for_unknown_connection() {
        let mgr = ConnectionManager::new();
        assert!(mgr.get_state("does-not-exist").await.is_none());
    }

    #[tokio::test]
    async fn find_connection_by_conversation_id_matches_when_bound() {
        let mgr = ConnectionManager::new();
        {
            let mut map = mgr.connections.lock().await;
            map.insert("c1".to_string(), fake_connection("c1", Some(42)));
            map.insert("c2".to_string(), fake_connection("c2", None));
        }
        let found = mgr
            .find_connection_by_conversation_id(42)
            .await
            .expect("should find c1");
        assert_eq!(found, "c1");
        assert!(mgr.find_connection_by_conversation_id(999).await.is_none());
    }

    #[tokio::test]
    async fn send_prompt_linked_creates_conversation_on_first_call_only() {
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/test").await;

        let mgr = ConnectionManager::new();
        let conn_id = "c1";
        {
            let mut map = mgr.connections.lock().await;
            // Note: cmd_tx receiver is dropped, so send_prompt's mpsc.send will fail
            // with ProcessExited. That's fine — we only verify the linkage side
            // effect, not the actual prompt forwarding.
            map.insert(conn_id.into(), fake_connection(conn_id, None));
        }

        // First call: creates conversation row, sets state.conversation_id.
        // The mpsc send error after linking is expected and ignored here.
        let _ = mgr
            .send_prompt_linked(&db, conn_id, one_text_block(), Some(folder_id), None)
            .await;
        let snap = mgr
            .get_state(conn_id)
            .await
            .unwrap()
            .read()
            .await
            .to_snapshot();
        assert!(
            snap.conversation_id.is_some(),
            "conversation_id should be set"
        );
        assert_eq!(snap.folder_id, Some(folder_id));
        let first_id = snap.conversation_id.unwrap();

        // Second call: ignores folder_id, does NOT create another row.
        let _ = mgr
            .send_prompt_linked(&db, conn_id, one_text_block(), Some(folder_id), None)
            .await;
        let snap2 = mgr
            .get_state(conn_id)
            .await
            .unwrap()
            .read()
            .await
            .to_snapshot();
        assert_eq!(snap2.conversation_id, Some(first_id));
    }

    #[tokio::test]
    async fn send_prompt_linked_errors_when_no_folder_id() {
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let mgr = ConnectionManager::new();
        let conn_id = "c1";
        {
            let mut map = mgr.connections.lock().await;
            map.insert(conn_id.into(), fake_connection(conn_id, None));
        }
        let result = mgr
            .send_prompt_linked(&db, conn_id, one_text_block(), None, None)
            .await;
        assert!(
            result.is_err(),
            "should error when folder_id is not provided for a new conversation row"
        );
        let err_str = result.unwrap_err().to_string();
        assert!(
            err_str.contains("folder_id"),
            "error should mention missing folder_id, got: {err_str}"
        );
    }

    /// Count of `conversation` rows (ignoring soft-delete) — used by the
    /// caller-supplied conversation_id tests to assert no new row was created.
    async fn count_conversation_rows(db: &crate::db::AppDatabase) -> usize {
        use crate::db::entities::conversation;
        use sea_orm::EntityTrait;
        conversation::Entity::find()
            .all(&db.conn)
            .await
            .unwrap()
            .len()
    }

    #[tokio::test]
    async fn send_prompt_linked_uses_caller_conversation_id_when_provided() {
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/caller-id").await;
        // Pre-create a conversation row the caller will reference.
        let pre_existing =
            conversation_service::create(&db.conn, folder_id, AgentType::ClaudeCode, None, None)
                .await
                .unwrap();

        let mgr = ConnectionManager::new();
        let (broadcaster, _rx) = make_test_broadcaster();
        let conn_id = "conn-caller-id";
        insert_fake_connection(
            &mgr,
            conn_id,
            AgentType::ClaudeCode,
            Some(PathBuf::from("/tmp/caller-id")),
            EventEmitter::test_web_only(broadcaster.clone()),
        )
        .await;
        let mut rx = subscribe_conn_stream(&mgr, conn_id).await;

        // Count rows before
        let before = count_conversation_rows(&db).await;

        // Send with caller-supplied conversation_id + folder_id.
        let _ = mgr
            .send_prompt_linked(
                &db,
                conn_id,
                one_text_block(),
                Some(folder_id),
                Some(pre_existing.id),
            )
            .await;

        // No new conversation row was created.
        let after = count_conversation_rows(&db).await;
        assert_eq!(after, before, "no new row should be created");

        // State now has the caller-supplied conversation_id.
        let state = mgr.get_state(conn_id).await.unwrap();
        assert_eq!(state.read().await.conversation_id, Some(pre_existing.id));

        // ConversationLinked event was emitted with the caller's id.
        let env = recv_first_acp_event(&mut rx).await;
        match env.payload {
            AcpEvent::ConversationLinked {
                conversation_id,
                folder_id: emitted_folder,
                ..
            } => {
                assert_eq!(conversation_id, pre_existing.id);
                assert_eq!(emitted_folder, folder_id);
            }
            other => panic!("expected ConversationLinked, got {other:?}"),
        }
    }

    /// Drain the global broadcaster and report whether a `conversation://changed`
    /// upsert for `id` carrying `external_id` was emitted.
    fn drain_has_upsert_with_external_id(
        rx: &mut broadcast::Receiver<WebEvent>,
        id: i32,
        external_id: &str,
    ) -> bool {
        while let Ok(evt) = rx.try_recv() {
            if evt.channel != crate::web::event_bridge::CONVERSATION_CHANGED_EVENT {
                continue;
            }
            let p = &*evt.payload;
            if p["kind"] == "upsert"
                && p["summary"]["id"] == id
                && p["summary"]["external_id"] == external_id
            {
                return true;
            }
        }
        false
    }

    #[tokio::test]
    async fn send_prompt_linked_session_started_before_link_broadcasts_external_id_branch_b() {
        // SessionStarted-before-link: external_id is already on the live state
        // but no conversation_id yet, so the lifecycle subscriber skipped its
        // broadcast. The synchronous external_id persist inside
        // send_prompt_linked (backend-create Branch B) must itself emit a
        // corrective `conversation://changed` upsert so other clients converge.
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/sess-pre-b").await;
        let mgr = ConnectionManager::new();
        let (broadcaster, mut rx) = make_test_broadcaster();
        let conn_id = "conn-sess-pre-b";
        insert_fake_connection(
            &mgr,
            conn_id,
            AgentType::ClaudeCode,
            Some(PathBuf::from("/tmp/sess-pre-b")),
            EventEmitter::test_web_only(broadcaster.clone()),
        )
        .await;
        {
            let state = mgr.get_state(conn_id).await.unwrap();
            state.write().await.external_id = Some("ext-pre".to_string());
        }

        // cmd_tx receiver is dropped → the prompt send fails after linking, but
        // the link + external_id persist + broadcast already happened.
        let _ = mgr
            .send_prompt_linked(&db, conn_id, one_text_block(), Some(folder_id), None)
            .await;

        let cid = mgr
            .get_state(conn_id)
            .await
            .unwrap()
            .read()
            .await
            .conversation_id
            .expect("conversation should be linked");
        let row = conversation_service::get_by_id(&db.conn, cid)
            .await
            .unwrap();
        assert_eq!(row.external_id.as_deref(), Some("ext-pre"));
        assert!(
            drain_has_upsert_with_external_id(&mut rx, cid, "ext-pre"),
            "Branch B must broadcast a conversation://changed upsert carrying external_id"
        );
    }

    #[tokio::test]
    async fn send_prompt_linked_session_started_before_link_broadcasts_external_id_branch_a() {
        // Same precondition, caller-supplied conversation_id (adopt Branch A).
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/sess-pre-a").await;
        let pre =
            conversation_service::create(&db.conn, folder_id, AgentType::ClaudeCode, None, None)
                .await
                .unwrap();
        let mgr = ConnectionManager::new();
        let (broadcaster, mut rx) = make_test_broadcaster();
        let conn_id = "conn-sess-pre-a";
        insert_fake_connection(
            &mgr,
            conn_id,
            AgentType::ClaudeCode,
            Some(PathBuf::from("/tmp/sess-pre-a")),
            EventEmitter::test_web_only(broadcaster.clone()),
        )
        .await;
        {
            let state = mgr.get_state(conn_id).await.unwrap();
            state.write().await.external_id = Some("ext-pre-a".to_string());
        }

        let _ = mgr
            .send_prompt_linked(
                &db,
                conn_id,
                one_text_block(),
                Some(folder_id),
                Some(pre.id),
            )
            .await;

        let row = conversation_service::get_by_id(&db.conn, pre.id)
            .await
            .unwrap();
        assert_eq!(row.external_id.as_deref(), Some("ext-pre-a"));
        assert!(
            drain_has_upsert_with_external_id(&mut rx, pre.id, "ext-pre-a"),
            "Branch A must broadcast a conversation://changed upsert carrying external_id"
        );
    }

    #[tokio::test]
    async fn send_prompt_linked_rejects_conversation_id_without_folder_id() {
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let mgr = ConnectionManager::new();
        let (broadcaster, _rx) = make_test_broadcaster();
        let conn_id = "conn-bad-args";
        insert_fake_connection(
            &mgr,
            conn_id,
            AgentType::ClaudeCode,
            Some(PathBuf::from("/tmp/x")),
            EventEmitter::test_web_only(broadcaster),
        )
        .await;

        let err = mgr
            .send_prompt_linked(&db, conn_id, one_text_block(), None, Some(42))
            .await
            .expect_err("should reject conversation_id without folder_id");
        assert!(matches!(err, AcpError::Protocol(_)));
    }

    #[tokio::test]
    async fn send_prompt_linked_caller_id_is_noop_when_already_linked() {
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/already").await;
        let pre =
            conversation_service::create(&db.conn, folder_id, AgentType::ClaudeCode, None, None)
                .await
                .unwrap();

        let mgr = ConnectionManager::new();
        let (broadcaster, _rx) = make_test_broadcaster();
        let conn_id = "conn-already";
        insert_fake_connection(
            &mgr,
            conn_id,
            AgentType::ClaudeCode,
            Some(PathBuf::from("/tmp/already")),
            EventEmitter::test_web_only(broadcaster.clone()),
        )
        .await;
        // Pre-link the connection state.
        {
            let state = mgr.get_state(conn_id).await.unwrap();
            state.write().await.conversation_id = Some(pre.id);
        }
        let mut rx = subscribe_conn_stream(&mgr, conn_id).await;

        let before = count_conversation_rows(&db).await;
        let _ = mgr
            .send_prompt_linked(
                &db,
                conn_id,
                one_text_block(),
                Some(folder_id),
                Some(pre.id),
            )
            .await;
        let after = count_conversation_rows(&db).await;
        assert_eq!(after, before);

        // No ConversationLinked event was emitted (already linked). The
        // centralized status transition fires InProgress; then because the
        // dropped cmd_tx receiver makes `send_prompt_inner` return
        // ProcessExited, the rollback path fires Cancelled. Two events,
        // strictly ordered.
        let env_in_progress = recv_first_acp_event(&mut rx).await;
        match env_in_progress.payload {
            AcpEvent::ConversationStatusChanged {
                conversation_id,
                status,
            } => {
                assert_eq!(conversation_id, pre.id);
                assert_eq!(status, ConversationStatus::InProgress);
            }
            other => {
                panic!("first event must be ConversationStatusChanged(InProgress), got {other:?}")
            }
        }
        let env_cancelled = recv_first_acp_event(&mut rx).await;
        match env_cancelled.payload {
            AcpEvent::ConversationStatusChanged {
                conversation_id,
                status,
            } => {
                assert_eq!(conversation_id, pre.id);
                assert_eq!(status, ConversationStatus::Cancelled);
            }
            other => panic!(
                "second event must be ConversationStatusChanged(Cancelled) after send failure, got {other:?}"
            ),
        }
    }

    // ---------- Phase: status centralization ----------

    #[tokio::test]
    async fn send_prompt_linked_writes_in_progress_and_emits_event() {
        use crate::db::entities::conversation;
        use crate::db::test_helpers;
        use sea_orm::EntityTrait;

        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/status").await;

        let mgr = ConnectionManager::new();
        let (broadcaster, _rx) = make_test_broadcaster();
        let conn_id = "conn-status-1";
        insert_fake_connection(
            &mgr,
            conn_id,
            AgentType::ClaudeCode,
            Some(PathBuf::from("/tmp/status")),
            EventEmitter::test_web_only(broadcaster.clone()),
        )
        .await;
        let mut rx = subscribe_conn_stream(&mgr, conn_id).await;

        // First call: backend creates the conversation row and links it.
        // The cmd_tx receiver in `insert_fake_connection` has been dropped,
        // so `send_prompt_inner` returns ProcessExited — exercising the new
        // Cancelled-rollback path. We expect THREE events in order:
        //   1. ConversationLinked
        //   2. ConversationStatusChanged(InProgress)  [pre-send write]
        //   3. ConversationStatusChanged(Cancelled)   [rollback after send failure]
        let _ = mgr
            .send_prompt_linked(&db, conn_id, one_text_block(), Some(folder_id), None)
            .await;

        let env1 = recv_first_acp_event(&mut rx).await;
        let conv_id = match env1.payload {
            AcpEvent::ConversationLinked {
                conversation_id,
                folder_id: emitted_folder,
                ..
            } => {
                assert_eq!(emitted_folder, folder_id);
                conversation_id
            }
            other => panic!("first event must be ConversationLinked, got {other:?}"),
        };
        let env2 = recv_first_acp_event(&mut rx).await;
        match env2.payload {
            AcpEvent::ConversationStatusChanged {
                conversation_id,
                status,
            } => {
                assert_eq!(conversation_id, conv_id);
                assert_eq!(status, ConversationStatus::InProgress);
            }
            other => {
                panic!("second event must be ConversationStatusChanged(InProgress), got {other:?}")
            }
        }
        let env3 = recv_first_acp_event(&mut rx).await;
        match env3.payload {
            AcpEvent::ConversationStatusChanged {
                conversation_id,
                status,
            } => {
                assert_eq!(conversation_id, conv_id);
                assert_eq!(status, ConversationStatus::Cancelled);
            }
            other => panic!(
                "third event must be ConversationStatusChanged(Cancelled) on send failure, got {other:?}"
            ),
        }
        // Ordering invariant: ConversationLinked < InProgress < Cancelled.
        assert!(
            env2.seq > env1.seq && env3.seq > env2.seq,
            "event seqs must be strictly monotonic: linked={} in_progress={} cancelled={}",
            env1.seq,
            env2.seq,
            env3.seq
        );

        // DB row settles at Cancelled (the rollback after send failure). The
        // intermediate InProgress write is observable only via the event,
        // not by the time the test reads the row.
        let row = conversation::Entity::find_by_id(conv_id)
            .one(&db.conn)
            .await
            .unwrap()
            .expect("conversation row exists");
        assert_eq!(row.status, ConversationStatus::Cancelled);

        // Second send: already-linked path also writes + emits InProgress
        // and then Cancelled (same send-failure rollback). Pre-flip the row
        // to PendingReview to observe the transition flip forward — mirrors
        // the "follow-up turn after a TurnComplete" scenario.
        conversation_service::update_status(&db.conn, conv_id, ConversationStatus::PendingReview)
            .await
            .unwrap();

        let _ = mgr
            .send_prompt_linked(&db, conn_id, one_text_block(), Some(folder_id), None)
            .await;

        let env4 = recv_first_acp_event(&mut rx).await;
        match env4.payload {
            AcpEvent::ConversationStatusChanged {
                conversation_id,
                status,
            } => {
                assert_eq!(conversation_id, conv_id);
                assert_eq!(status, ConversationStatus::InProgress);
            }
            other => panic!(
                "second send must re-emit ConversationStatusChanged(InProgress) first, got {other:?}"
            ),
        }
        let env5 = recv_first_acp_event(&mut rx).await;
        match env5.payload {
            AcpEvent::ConversationStatusChanged {
                conversation_id,
                status,
            } => {
                assert_eq!(conversation_id, conv_id);
                assert_eq!(status, ConversationStatus::Cancelled);
            }
            other => {
                panic!("second send must rollback to Cancelled after send failure, got {other:?}")
            }
        }
        let row2 = conversation::Entity::find_by_id(conv_id)
            .one(&db.conn)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row2.status, ConversationStatus::Cancelled);
    }

    // ---------- Phase: connection dedup ----------

    #[tokio::test]
    async fn find_connection_for_reuse_returns_none_when_session_id_is_none() {
        let mgr = ConnectionManager::new();
        let (broadcaster, _rx) = make_test_broadcaster();
        // Insert a connection that *would* match if session_id were Some.
        let id = "c1";
        insert_fake_connection(
            &mgr,
            id,
            AgentType::ClaudeCode,
            Some(PathBuf::from("/tmp/reuse")),
            EventEmitter::test_web_only(broadcaster),
        )
        .await;
        {
            let state = mgr.get_state(id).await.unwrap();
            state.write().await.external_id = Some("ext-1".into());
        }
        let found = mgr
            .find_connection_for_reuse(
                AgentType::ClaudeCode,
                Some(&PathBuf::from("/tmp/reuse")),
                None,
            )
            .await;
        assert!(
            found.is_none(),
            "no session_id means we never dedup speculative connects"
        );
    }

    #[tokio::test]
    async fn spawn_agent_reuses_existing_connection_when_session_id_matches() {
        // Direct unit test for the lookup helper that spawn_agent calls
        // before its (process-spawning) block. We test the helper directly so
        // the test never tries to launch an agent process.
        let mgr = ConnectionManager::new();
        let (broadcaster, _rx) = make_test_broadcaster();
        let existing_id = "preexisting-conn";
        let working_dir = PathBuf::from("/tmp/reuse-match");
        insert_fake_connection(
            &mgr,
            existing_id,
            AgentType::ClaudeCode,
            Some(working_dir.clone()),
            EventEmitter::test_web_only(broadcaster.clone()),
        )
        .await;
        {
            let state = mgr.get_state(existing_id).await.unwrap();
            let mut s = state.write().await;
            s.external_id = Some("ext-1".into());
            s.status = ConnectionStatus::Connected;
        }

        // Same session_id + same agent + same working_dir -> reuse.
        let found = mgr
            .find_connection_for_reuse(AgentType::ClaudeCode, Some(&working_dir), Some("ext-1"))
            .await;
        assert_eq!(found.as_deref(), Some(existing_id));

        // Different session_id -> no reuse.
        assert!(mgr
            .find_connection_for_reuse(AgentType::ClaudeCode, Some(&working_dir), Some("other-ext"))
            .await
            .is_none());

        // Different working_dir -> no reuse.
        assert!(mgr
            .find_connection_for_reuse(
                AgentType::ClaudeCode,
                Some(&PathBuf::from("/tmp/different")),
                Some("ext-1")
            )
            .await
            .is_none());

        // Different agent_type -> no reuse.
        assert!(mgr
            .find_connection_for_reuse(AgentType::Codex, Some(&working_dir), Some("ext-1"))
            .await
            .is_none());
    }

    #[tokio::test]
    async fn find_connection_for_reuse_skips_disconnected_or_errored() {
        let mgr = ConnectionManager::new();
        let (broadcaster, _rx) = make_test_broadcaster();
        let working_dir = PathBuf::from("/tmp/torn-down");
        insert_fake_connection(
            &mgr,
            "torn",
            AgentType::ClaudeCode,
            Some(working_dir.clone()),
            EventEmitter::test_web_only(broadcaster.clone()),
        )
        .await;
        {
            let state = mgr.get_state("torn").await.unwrap();
            let mut s = state.write().await;
            s.external_id = Some("ext-1".into());
            s.status = ConnectionStatus::Disconnected;
        }
        assert!(
            mgr.find_connection_for_reuse(
                AgentType::ClaudeCode,
                Some(&working_dir),
                Some("ext-1"),
            )
            .await
            .is_none(),
            "Disconnected connection must not be reused"
        );

        // Flip to Error — also excluded.
        {
            let state = mgr.get_state("torn").await.unwrap();
            state.write().await.status = ConnectionStatus::Error;
        }
        assert!(
            mgr.find_connection_for_reuse(
                AgentType::ClaudeCode,
                Some(&working_dir),
                Some("ext-1"),
            )
            .await
            .is_none(),
            "Errored connection must not be reused"
        );
    }

    /// Helper that backdates a connection's `last_activity_at` so the
    /// idle sweep sees it as having crossed its threshold.
    async fn backdate_last_activity(mgr: &ConnectionManager, conn_id: &str, secs_ago: i64) {
        let state = mgr.get_state(conn_id).await.expect("connection exists");
        let mut s = state.write().await;
        s.last_activity_at = chrono::Utc::now() - chrono::Duration::seconds(secs_ago);
    }

    #[tokio::test]
    async fn sweep_idle_disconnects_idle_connected_connections() {
        let mgr = ConnectionManager::new();
        insert_fake_connection(
            &mgr,
            "stale",
            AgentType::ClaudeCode,
            Some(PathBuf::from("/tmp/stale")),
            EventEmitter::Noop,
        )
        .await;
        backdate_last_activity(&mgr, "stale", 600).await;

        let n = mgr.sweep_idle(Duration::from_secs(300)).await;
        assert_eq!(n, 1);
        assert!(
            mgr.connections.lock().await.get("stale").is_none(),
            "Idle connection must be removed after sweep"
        );
    }

    #[tokio::test]
    async fn sweep_idle_skips_recently_active_connection() {
        let mgr = ConnectionManager::new();
        insert_fake_connection(
            &mgr,
            "fresh",
            AgentType::ClaudeCode,
            None,
            EventEmitter::Noop,
        )
        .await;
        // last_activity_at defaults to "now" inside SessionState::new — no
        // backdating, so it should NOT be swept.
        let n = mgr.sweep_idle(Duration::from_secs(300)).await;
        assert_eq!(n, 0);
        assert!(mgr.connections.lock().await.contains_key("fresh"));
    }

    #[tokio::test]
    async fn sweep_idle_skips_prompting_connection() {
        let mgr = ConnectionManager::new();
        insert_fake_connection(
            &mgr,
            "prompting",
            AgentType::ClaudeCode,
            None,
            EventEmitter::Noop,
        )
        .await;
        backdate_last_activity(&mgr, "prompting", 600).await;
        // Override status to Prompting — a turn is in flight; never sweep.
        {
            let state = mgr.get_state("prompting").await.unwrap();
            state.write().await.status = ConnectionStatus::Prompting;
        }
        let n = mgr.sweep_idle(Duration::from_secs(300)).await;
        assert_eq!(n, 0);
        assert!(mgr.connections.lock().await.contains_key("prompting"));
    }

    #[tokio::test]
    async fn sweep_idle_skips_pending_permission() {
        use crate::acp::session_state::PendingPermissionState;
        let mgr = ConnectionManager::new();
        insert_fake_connection(
            &mgr,
            "permission",
            AgentType::ClaudeCode,
            None,
            EventEmitter::Noop,
        )
        .await;
        backdate_last_activity(&mgr, "permission", 600).await;
        {
            let state = mgr.get_state("permission").await.unwrap();
            state.write().await.pending_permission = Some(PendingPermissionState {
                request_id: "req-1".into(),
                tool_call_id: "tc-1".into(),
                tool_call: serde_json::json!({ "toolCallId": "tc-1", "title": "test" }),
                options: vec![],
                created_at: chrono::Utc::now(),
                queued: 0,
            });
        }
        let n = mgr.sweep_idle(Duration::from_secs(300)).await;
        assert_eq!(
            n, 0,
            "Connection with pending permission must not be swept (user is mid-decision)"
        );
        assert!(mgr.connections.lock().await.contains_key("permission"));
    }

    #[tokio::test]
    async fn sweep_idle_skips_active_background_work() {
        let mgr = ConnectionManager::new();
        insert_fake_connection(
            &mgr,
            "background",
            AgentType::ClaudeCode,
            None,
            EventEmitter::Noop,
        )
        .await;
        backdate_last_activity(&mgr, "background", 600).await;
        {
            let state = mgr.get_state("background").await.unwrap();
            let mut state = state.write().await;
            // Mirror what apply_event(BackgroundActivity) records: pending
            // work plus a recent watcher heartbeat.
            state.background_outstanding = 1;
            state.background_activity_at = Some(chrono::Utc::now());
        }
        let n = mgr.sweep_idle(Duration::from_secs(300)).await;
        assert_eq!(
            n, 0,
            "Connection with unresolved background work must not be swept \
             (disconnecting kills the agent CLI and the background task with it)"
        );
        assert!(mgr.connections.lock().await.contains_key("background"));

        // Once the watcher settles the work (outstanding back to 0), the same
        // connection becomes sweepable again.
        {
            let state = mgr.get_state("background").await.unwrap();
            let mut state = state.write().await;
            state.background_outstanding = 0;
            state.last_activity_at = chrono::Utc::now() - chrono::Duration::seconds(600);
        }
        let n = mgr.sweep_idle(Duration::from_secs(300)).await;
        assert_eq!(n, 1, "settled background work no longer exempts the sweep");
    }

    #[tokio::test]
    async fn sweep_idle_picks_only_qualifying_subset() {
        let mgr = ConnectionManager::new();
        for id in ["a", "b", "c"] {
            insert_fake_connection(&mgr, id, AgentType::ClaudeCode, None, EventEmitter::Noop).await;
        }
        // a: idle (sweep target), b: fresh (not idle), c: idle but Prompting (skipped).
        backdate_last_activity(&mgr, "a", 600).await;
        backdate_last_activity(&mgr, "c", 600).await;
        {
            let state = mgr.get_state("c").await.unwrap();
            state.write().await.status = ConnectionStatus::Prompting;
        }
        let n = mgr.sweep_idle(Duration::from_secs(300)).await;
        assert_eq!(n, 1);
        let map = mgr.connections.lock().await;
        assert!(!map.contains_key("a"));
        assert!(map.contains_key("b"));
        assert!(map.contains_key("c"));
    }

    /// When two `spawn_agent` calls race for the same logical session id,
    /// the per-key dedup mutex makes the second one observe the first's
    /// freshly-spawned connection and reuse it. Without the mutex, both
    /// would have missed dedup during the connecting window.
    ///
    /// Simulates the race by pre-inserting a "first call's connection" with
    /// `external_id` set; what's tested is that two concurrent
    /// `find_connection_for_reuse` calls under the same lock see consistent
    /// state. The `spawn_locks` map being shared via `clone_ref` is the
    /// invariant we need.
    #[tokio::test]
    async fn spawn_locks_are_shared_across_clone_ref() {
        let mgr = ConnectionManager::new();
        let cloned = mgr.clone_ref();
        // Both clones must reference the same map. Insert via one,
        // observe via the other.
        let key = SpawnDedupKey {
            agent_type: AgentType::ClaudeCode,
            working_dir: Some(PathBuf::from("/tmp/dedup-test")),
            session_id: "ext-shared".into(),
        };
        {
            let mut locks = mgr.spawn_locks.lock().await;
            locks
                .entry(key.clone())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())));
        }
        let cloned_locks = cloned.spawn_locks.lock().await;
        assert!(
            cloned_locks.contains_key(&key),
            "spawn_locks must be shared between original and clone_ref"
        );
    }

    /// Two concurrent `send_prompt_linked` calls on the SAME connection
    /// must serialize through the per-connection `prompt_lock` so the
    /// backend-creates branch can't fire twice and produce duplicate
    /// conversation rows. The second call observes `already_linked == true`
    /// (set by the first under the lock) and skips creation.
    #[tokio::test]
    async fn send_prompt_linked_serializes_concurrent_callers() {
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/race").await;

        let mgr = Arc::new(ConnectionManager::new());
        let conn_id = "race-conn";
        {
            let mut map = mgr.connections.lock().await;
            map.insert(conn_id.into(), fake_connection(conn_id, None));
        }

        let before = count_conversation_rows(&db).await;
        // tokio::join! polls the two futures concurrently in the SAME
        // task — they can borrow `&db` and `mgr` without the 'static
        // requirement that `tokio::spawn` would impose.
        let mgr_ref = mgr.as_ref();
        tokio::join!(
            async {
                let _ = mgr_ref
                    .send_prompt_linked(&db, conn_id, one_text_block(), Some(folder_id), None)
                    .await;
            },
            async {
                let _ = mgr_ref
                    .send_prompt_linked(&db, conn_id, one_text_block(), Some(folder_id), None)
                    .await;
            },
        );

        let after = count_conversation_rows(&db).await;
        assert_eq!(
            after - before,
            1,
            "exactly one new conversation row across two concurrent send_prompt_linked"
        );
    }

    // ---------- Phase: spawn handshake wait helper ----------

    #[tokio::test]
    async fn wait_for_session_started_returns_ready_when_sender_fires() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        // Fire immediately on a separate task so the wait future actually
        // gets to register.
        tokio::spawn(async move {
            let _ = tx.send(());
        });
        let (outcome, elapsed) = wait_for_session_started(rx, Duration::from_millis(500)).await;
        assert_eq!(outcome, HandshakeWaitOutcome::Ready);
        assert!(
            elapsed < Duration::from_millis(500),
            "Ready outcome must resolve well before timeout, got {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn wait_for_session_started_returns_aborted_when_sender_drops() {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        // Drop the sender — emulates "connection died before SessionStarted",
        // i.e. SessionState's tx was dropped during cleanup.
        drop(tx);
        let (outcome, elapsed) = wait_for_session_started(rx, Duration::from_millis(500)).await;
        assert_eq!(outcome, HandshakeWaitOutcome::Aborted);
        assert!(
            elapsed < Duration::from_millis(500),
            "Aborted outcome must resolve well before timeout, got {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn wait_for_session_started_returns_timed_out_when_neither_happens() {
        let (_tx, rx) = tokio::sync::oneshot::channel::<()>();
        // Hold the sender alive but never fire and never drop. Tight
        // timeout so the test stays fast; production timeout is 60s.
        let (outcome, elapsed) = wait_for_session_started(rx, Duration::from_millis(40)).await;
        assert_eq!(outcome, HandshakeWaitOutcome::TimedOut);
        assert!(
            elapsed >= Duration::from_millis(40),
            "TimedOut must wait at least the full timeout, got {elapsed:?}"
        );
    }

    #[test]
    fn spawn_handshake_timeout_from_env_uses_default_when_unset() {
        // Snapshot env, mutate, restore. Single test owns this var to avoid
        // cross-test contention.
        let prev = std::env::var("CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS").ok();
        std::env::remove_var("CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS");
        let default = spawn_handshake_timeout_from_env();
        assert_eq!(default, Duration::from_secs(SPAWN_HANDSHAKE_TIMEOUT_SECS));

        std::env::set_var("CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS", "5");
        assert_eq!(spawn_handshake_timeout_from_env(), Duration::from_secs(5));

        std::env::set_var("CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS", "garbage");
        assert_eq!(
            spawn_handshake_timeout_from_env(),
            Duration::from_secs(SPAWN_HANDSHAKE_TIMEOUT_SECS),
            "invalid value falls back to default"
        );

        // Restore.
        match prev {
            Some(v) => std::env::set_var("CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS", v),
            None => std::env::remove_var("CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS"),
        }
    }

    #[test]
    fn with_spawn_handshake_timeout_overrides_default_for_tests() {
        let mgr = ConnectionManager::with_spawn_handshake_timeout(Duration::from_secs(7));
        assert_eq!(mgr.spawn_handshake_timeout, Duration::from_secs(7));
    }

    /// When `send_prompt_inner` fails (process gone, channel closed) the row
    /// must end up `Cancelled`, NOT stuck on `in_progress`. Without this
    /// rollback the lifecycle subscriber's TurnComplete write never fires
    /// (no turn ever started), so the only thing that could later un-stick
    /// the row is a follow-up prompt happening to succeed — fragile, and on
    /// the server-side / chat-channel paths there may be no follow-up at all.
    #[tokio::test]
    async fn send_prompt_linked_rolls_back_to_cancelled_on_send_failure() {
        use crate::db::entities::conversation;
        use crate::db::test_helpers;
        use sea_orm::EntityTrait;

        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/cancel-rollback").await;

        let mgr = ConnectionManager::new();
        let (broadcaster, _rx) = make_test_broadcaster();
        let conn_id = "conn-cancel";
        // insert_fake_connection drops the cmd_tx receiver, so send_prompt_inner
        // returns ProcessExited — exactly the failure mode this test targets.
        insert_fake_connection(
            &mgr,
            conn_id,
            AgentType::ClaudeCode,
            Some(PathBuf::from("/tmp/cancel-rollback")),
            EventEmitter::test_web_only(broadcaster.clone()),
        )
        .await;
        let mut rx = subscribe_conn_stream(&mgr, conn_id).await;

        // Non-empty blocks so the send reaches `reserve()` (which fails on the
        // dropped receiver → ProcessExited); an empty prompt would be rejected
        // earlier, before the gate, and never exercise this rollback path.
        let result = mgr
            .send_prompt_linked(
                &db,
                conn_id,
                vec![PromptInputBlock::Text {
                    text: "trigger send failure".into(),
                }],
                Some(folder_id),
                None,
            )
            .await;
        assert!(
            matches!(result, Err(AcpError::ProcessExited)),
            "send_prompt_inner must propagate ProcessExited up to the caller; got {result:?}"
        );

        // Drain events: ConversationLinked → InProgress → Cancelled, in order.
        let env_linked = recv_first_acp_event(&mut rx).await;
        let conv_id = match env_linked.payload {
            AcpEvent::ConversationLinked {
                conversation_id, ..
            } => conversation_id,
            other => panic!("expected ConversationLinked first, got {other:?}"),
        };
        let env_in_progress = recv_first_acp_event(&mut rx).await;
        match env_in_progress.payload {
            AcpEvent::ConversationStatusChanged { status, .. } => {
                assert_eq!(status, ConversationStatus::InProgress);
            }
            other => {
                panic!("expected ConversationStatusChanged(InProgress) before send, got {other:?}")
            }
        }
        let env_cancelled = recv_first_acp_event(&mut rx).await;
        match env_cancelled.payload {
            AcpEvent::ConversationStatusChanged {
                conversation_id,
                status,
            } => {
                assert_eq!(conversation_id, conv_id);
                assert_eq!(
                    status,
                    ConversationStatus::Cancelled,
                    "send_prompt failure must roll the row forward to Cancelled, not leave InProgress"
                );
            }
            other => panic!(
                "expected ConversationStatusChanged(Cancelled) on send failure, got {other:?}"
            ),
        }

        // Strict ordering: linked < in_progress < cancelled. The lifecycle
        // contract says the Cancelled emit cannot precede the InProgress one
        // — UIs that animate based on "previous → current" depend on this.
        assert!(
            env_in_progress.seq > env_linked.seq && env_cancelled.seq > env_in_progress.seq,
            "event seq must be strictly monotonic: linked={} in_progress={} cancelled={}",
            env_linked.seq,
            env_in_progress.seq,
            env_cancelled.seq,
        );

        // DB row settles at Cancelled — final ground truth read.
        let row = conversation::Entity::find_by_id(conv_id)
            .one(&db.conn)
            .await
            .unwrap()
            .expect("conversation row exists");
        assert_eq!(row.status, ConversationStatus::Cancelled);
    }

    // ---------- fork_session ----------

    /// Fake the protocol reply plus the manager-controlled activation barrier.
    async fn manager_with_fake_fork(
        conn_id: &str,
        conversation_id: i32,
        forked_session_id: &str,
        original_session_id: &str,
    ) -> (Arc<ConnectionManager>, tokio::task::JoinHandle<()>) {
        use crate::acp::connection::ConnectionCommand;
        let (tx, mut rx) = mpsc::channel::<ConnectionCommand>(4);
        let mut state = SessionState::new(
            conn_id.to_string(),
            AgentType::ClaudeCode,
            None,
            "test-window".to_string(),
            None,
        );
        state.conversation_id = Some(conversation_id);
        state.status = ConnectionStatus::Connected;
        state.external_id = Some(original_session_id.to_string());
        let state = Arc::new(RwLock::new(state));
        let conn = AgentConnection {
            id: conn_id.to_string(),
            agent_type: AgentType::ClaudeCode,
            status: ConnectionStatus::Connected,
            owner_window_label: "test-window".to_string(),
            cmd_tx: tx,
            state: Arc::clone(&state),
            emitter: EventEmitter::Noop,
            prompt_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_fingerprint: String::new(),
            last_observed_fingerprint: String::new(),
            child_pid: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        };
        let mgr = Arc::new(ConnectionManager::new());
        mgr.connections
            .lock()
            .await
            .insert(conn_id.to_string(), conn);

        let forked = forked_session_id.to_string();
        let original = original_session_id.to_string();
        let join = tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                if let ConnectionCommand::Fork { reply, activation } = cmd {
                    let _ = reply.send(Ok(crate::acp::types::ForkProtocolResult {
                        forked_session_id: forked.clone(),
                        original_session_id: original.clone(),
                    }));
                    if let Ok(ForkActivation::Commit {
                        original_conversation_id,
                        forked_conversation_id,
                        folder_id,
                        ack,
                    }) = activation.await
                    {
                        let mut live = state.write().await;
                        live.apply_event(&AcpEvent::ConversationForked {
                            original_conversation_id,
                            forked_conversation_id,
                            original_session_id: original.clone(),
                            forked_session_id: forked.clone(),
                            folder_id,
                        });
                        live.apply_event(&AcpEvent::SessionStarted {
                            session_id: forked.clone(),
                        });
                        let _ = ack.send(Ok(()));
                    }
                    return;
                }
            }
        });
        (mgr, join)
    }

    async fn seed_forkable(
        db: &AppDatabase,
        folder_id: i32,
        title: Option<&str>,
    ) -> conversation::Model {
        let c1 = conversation_service::create(
            &db.conn,
            folder_id,
            AgentType::ClaudeCode,
            title.map(str::to_string),
            Some("feature/x".into()),
        )
        .await
        .unwrap();
        conversation_service::update_external_id(&db.conn, c1.id, "session-S1".into())
            .await
            .unwrap();
        conversation::Entity::find_by_id(c1.id)
            .one(&db.conn)
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn fork_session_writes_atomic_two_row_layout() {
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/fork-happy").await;
        let c1 = seed_forkable(&db, folder_id, Some("Original Topic")).await;

        let (mgr, join) = manager_with_fake_fork("c-fork", c1.id, "session-S2", "session-S1").await;
        let result = mgr.fork_session(&db, "c-fork", None, None).await.unwrap();
        let _ = join.await;

        assert_eq!(result.original_conversation_id, c1.id);
        assert_eq!(result.original_session_id, "session-S1");
        assert_eq!(result.forked_session_id, "session-S2");
        assert_ne!(result.forked_conversation_id, c1.id);
        assert!(result.active_view_handoff.connection_rebound);
        assert_eq!(
            result.active_view_handoff.to_conversation_id,
            result.forked_conversation_id
        );

        let retained = conversation_service::get_by_id(&db.conn, c1.id)
            .await
            .unwrap();
        assert_eq!(retained.external_id.as_deref(), Some("session-S1"));
        assert_eq!(retained.title.as_deref(), Some("Original Topic"));

        let c2 = conversation_service::get_by_id(&db.conn, result.forked_conversation_id)
            .await
            .unwrap();
        assert_eq!(c2.external_id.as_deref(), Some("session-S2"));
        assert_eq!(c2.title.as_deref(), Some("[Fork] Original Topic"));
        assert_eq!(c2.status, "pending_review");
        assert_eq!(c2.folder_id, folder_id);
        assert_eq!(c2.git_branch.as_deref(), Some("feature/x"));

        let live = mgr.get_state("c-fork").await.unwrap();
        let live = live.read().await;
        assert_eq!(live.conversation_id, Some(result.forked_conversation_id));
        assert_eq!(live.external_id.as_deref(), Some("session-S2"));
    }

    #[tokio::test]
    async fn fork_session_copies_collection_but_keeps_queue_and_mailbox_on_c1() {
        use crate::db::service::{collection_service, prompt_queue_service};
        use crate::db::test_helpers;
        use crate::models::{
            CollaborationDeliveryHint, CollaborationInvocationPolicy, CollaborationUrgency,
            EnqueuePromptQueueItem, PromptQueueDraft, SendCollaborationMessageInput,
        };

        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/fork-owned-facts").await;
        let source = test_helpers::seed_conversation(&db, folder_id, AgentType::Codex).await;
        let c1 = seed_forkable(&db, folder_id, Some("Owned Facts")).await;
        let collection =
            collection_service::create(&db.conn, "Fork Context".into(), None, Some(folder_id))
                .await
                .unwrap();
        collection_service::assign_conversations(&db.conn, vec![c1.id], Some(collection.id))
            .await
            .unwrap();

        prompt_queue_service::enqueue(
            &db.conn,
            EnqueuePromptQueueItem {
                conversation_id: c1.id,
                id: "c1-followup".into(),
                client_dedupe_id: "c1-followup".into(),
                draft: PromptQueueDraft {
                    blocks: vec![PromptInputBlock::Text {
                        text: "still belongs to C1".into(),
                    }],
                    display_text: "still belongs to C1".into(),
                },
                mode_id: None,
            },
        )
        .await
        .unwrap();
        collaboration_service::send(
            &db.conn,
            SendCollaborationMessageInput {
                source_conversation_id: source,
                target_conversation_ids: vec![c1.id],
                body: "mail addressed to immutable C1".into(),
                client_dedupe_id: "fork-mail-c1".into(),
                invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                delivery_hint: CollaborationDeliveryHint::Default,
                expects_reply: true,
                urgency: CollaborationUrgency::Normal,
                reply_to_event_id: None,
            },
        )
        .await
        .unwrap();

        let (mgr, join) =
            manager_with_fake_fork("c-owned-facts", c1.id, "session-S2", "session-S1").await;
        let result = mgr
            .fork_session(&db, "c-owned-facts", None, None)
            .await
            .unwrap();
        let _ = join.await;
        let c2 = result.forked_conversation_id;

        assert_eq!(
            prompt_queue_service::snapshot(&db.conn, c1.id)
                .await
                .unwrap()
                .items
                .len(),
            1
        );
        assert!(prompt_queue_service::snapshot(&db.conn, c2)
            .await
            .unwrap()
            .items
            .is_empty());

        assert_eq!(
            collaboration_service::feed(&db.conn, c1.id, None)
                .await
                .unwrap()
                .inbound
                .len(),
            1
        );
        let c2_feed = collaboration_service::feed(&db.conn, c2, None)
            .await
            .unwrap();
        assert!(c2_feed.inbound.is_empty());
        assert!(c2_feed.outbound.is_empty());

        let refs = collection_service::list_conversation_refs(&db.conn, vec![c1.id, c2])
            .await
            .unwrap();
        assert_eq!(refs.len(), 2);
        assert!(refs.iter().all(|item| item.collection_id == collection.id));
    }

    #[tokio::test]
    async fn fork_session_normalizes_the_new_branch_title_without_mutating_c1() {
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/fork-title").await;
        let c1 = seed_forkable(&db, folder_id, Some("[Fork]NoSpaceTitle")).await;

        let (mgr, join) =
            manager_with_fake_fork("c-title", c1.id, "session-S2", "session-S1").await;
        let result = mgr.fork_session(&db, "c-title", None, None).await.unwrap();
        let _ = join.await;

        let retained = conversation_service::get_by_id(&db.conn, c1.id)
            .await
            .unwrap();
        assert_eq!(retained.title.as_deref(), Some("[Fork]NoSpaceTitle"));
        let c2 = conversation_service::get_by_id(&db.conn, result.forked_conversation_id)
            .await
            .unwrap();
        assert_eq!(c2.title.as_deref(), Some("[Fork] NoSpaceTitle"));
        assert!(c2.title_locked);
    }

    #[tokio::test]
    async fn fork_session_reads_latest_committed_row_not_a_cached_snapshot() {
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/fork-latest").await;
        let c1 = seed_forkable(&db, folder_id, Some("Stale Original")).await;
        conversation_service::update_title(&db.conn, c1.id, "Renamed By User".into())
            .await
            .unwrap();

        let (mgr, join) =
            manager_with_fake_fork("c-latest", c1.id, "session-S2", "session-S1").await;
        let result = mgr.fork_session(&db, "c-latest", None, None).await.unwrap();
        let _ = join.await;

        assert_eq!(
            conversation_service::get_by_id(&db.conn, c1.id)
                .await
                .unwrap()
                .title
                .as_deref(),
            Some("Renamed By User")
        );
        let c2 = conversation_service::get_by_id(&db.conn, result.forked_conversation_id)
            .await
            .unwrap();
        assert_eq!(c2.title.as_deref(), Some("[Fork] Renamed By User"));
        assert!(c2.title_locked);
    }

    #[tokio::test]
    async fn fork_session_leaves_a_titleless_c2_unlocked() {
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/fork-untitled").await;
        let c1 = seed_forkable(&db, folder_id, None).await;

        let (mgr, join) =
            manager_with_fake_fork("c-untitled", c1.id, "session-S2", "session-S1").await;
        let result = mgr
            .fork_session(&db, "c-untitled", None, None)
            .await
            .unwrap();
        let _ = join.await;

        let c2 = conversation_service::get_by_id(&db.conn, result.forked_conversation_id)
            .await
            .unwrap();
        assert_eq!(c2.title, None);
        assert!(!c2.title_locked);
        assert!(
            conversation_service::refresh_auto_title(&db.conn, c2.id, "First Name".into())
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn fork_session_errors_without_orphan_when_row_missing_or_deleted() {
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/fork-missing").await;

        let (mgr, join) =
            manager_with_fake_fork("c-missing", 99_999, "session-S2", "session-S1").await;
        assert!(mgr
            .fork_session(&db, "c-missing", None, None)
            .await
            .is_err());
        let _ = join.await;
        assert!(conversation::Entity::find()
            .all(&db.conn)
            .await
            .unwrap()
            .is_empty());

        let c1 = seed_forkable(&db, folder_id, Some("Deleted")).await;
        conversation_service::soft_delete(&db.conn, c1.id)
            .await
            .unwrap();
        let (mgr, join) =
            manager_with_fake_fork("c-deleted", c1.id, "session-S2", "session-S1").await;
        assert!(mgr
            .fork_session(&db, "c-deleted", None, None)
            .await
            .is_err());
        let _ = join.await;
        let rows = conversation::Entity::find().all(&db.conn).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, c1.id);
        assert_eq!(rows[0].external_id.as_deref(), Some("session-S1"));
        assert!(rows[0].deleted_at.is_some());
    }

    #[tokio::test]
    async fn fork_session_rejects_a_stale_native_binding_without_creating_c2() {
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/fork-stale-binding").await;
        let c1 = seed_forkable(&db, folder_id, Some("Binding changed")).await;
        conversation_service::update_external_id(&db.conn, c1.id, "session-S3".into())
            .await
            .unwrap();

        let (mgr, join) =
            manager_with_fake_fork("c-stale-binding", c1.id, "session-S2", "session-S1").await;
        let error = mgr
            .fork_session(&db, "c-stale-binding", None, None)
            .await
            .unwrap_err();
        let _ = join.await;
        assert!(error.to_string().contains("binding changed"));
        let rows = conversation::Entity::find().all(&db.conn).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].external_id.as_deref(), Some("session-S3"));
    }

    #[tokio::test]
    async fn fork_session_rejects_unbound_connection() {
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let mgr = ConnectionManager::new();
        mgr.connections
            .lock()
            .await
            .insert("c-unbound".into(), fake_connection("c-unbound", None));
        let error = mgr
            .fork_session(&db, "c-unbound", None, None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("linked conversation row"));
    }

    #[tokio::test]
    async fn unsupported_provider_fork_creates_no_c2() {
        use crate::acp::connection::ConnectionCommand;
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/fork-unsupported").await;
        let c1 = seed_forkable(&db, folder_id, Some("Original")).await;

        let mgr = ConnectionManager::new();
        let mut commands =
            insert_live_connection(&mgr, "c-unsupported", AgentType::ClaudeCode, None).await;
        mgr.get_state("c-unsupported")
            .await
            .unwrap()
            .write()
            .await
            .conversation_id = Some(c1.id);
        let provider = tokio::spawn(async move {
            if let Some(ConnectionCommand::Fork { reply, .. }) = commands.recv().await {
                let _ = reply.send(Err(AcpError::protocol(
                    "This agent does not support session/fork",
                )));
            }
        });

        let error = mgr
            .fork_session(&db, "c-unsupported", None, None)
            .await
            .unwrap_err();
        let _ = provider.await;
        assert!(error.to_string().contains("does not support"));
        let rows = conversation::Entity::find().all(&db.conn).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, c1.id);
        assert_eq!(rows[0].external_id.as_deref(), Some("session-S1"));
    }

    #[tokio::test]
    async fn fork_session_links_unbound_row_from_caller_ids() {
        use crate::acp::connection::ConnectionCommand;
        use crate::db::test_helpers;
        let db = test_helpers::fresh_in_memory_db().await;
        let folder_id = test_helpers::seed_folder(&db, "/tmp/fork-relink").await;
        let c1 = seed_forkable(&db, folder_id, Some("History")).await;

        let (tx, mut rx) = mpsc::channel::<ConnectionCommand>(4);
        let mut state = SessionState::new(
            "c-relink".to_string(),
            AgentType::ClaudeCode,
            None,
            "test-window".to_string(),
            None,
        );
        state.status = ConnectionStatus::Connected;
        state.external_id = Some("session-S1".into());
        let state = Arc::new(RwLock::new(state));
        let conn = AgentConnection {
            id: "c-relink".to_string(),
            agent_type: AgentType::ClaudeCode,
            status: ConnectionStatus::Connected,
            owner_window_label: "test-window".to_string(),
            cmd_tx: tx,
            state: Arc::clone(&state),
            emitter: EventEmitter::Noop,
            prompt_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_fingerprint: String::new(),
            last_observed_fingerprint: String::new(),
            child_pid: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        };
        let mgr = ConnectionManager::new();
        mgr.connections
            .lock()
            .await
            .insert("c-relink".to_string(), conn);

        let join = tokio::spawn(async move {
            if let Some(ConnectionCommand::Fork { reply, activation }) = rx.recv().await {
                let _ = reply.send(Ok(crate::acp::types::ForkProtocolResult {
                    forked_session_id: "session-S2".into(),
                    original_session_id: "session-S1".into(),
                }));
                if let Ok(ForkActivation::Commit {
                    original_conversation_id,
                    forked_conversation_id,
                    folder_id,
                    ack,
                }) = activation.await
                {
                    let mut live = state.write().await;
                    live.apply_event(&AcpEvent::ConversationForked {
                        original_conversation_id,
                        forked_conversation_id,
                        original_session_id: "session-S1".into(),
                        forked_session_id: "session-S2".into(),
                        folder_id,
                    });
                    live.apply_event(&AcpEvent::SessionStarted {
                        session_id: "session-S2".into(),
                    });
                    let _ = ack.send(Ok(()));
                }
            }
        });

        let result = mgr
            .fork_session(&db, "c-relink", Some(c1.id), Some(folder_id))
            .await
            .unwrap();
        let _ = join.await;
        assert_eq!(result.original_conversation_id, c1.id);
        assert_eq!(
            mgr.get_state("c-relink")
                .await
                .unwrap()
                .read()
                .await
                .conversation_id,
            Some(result.forked_conversation_id)
        );
        assert_eq!(
            conversation_service::get_by_id(&db.conn, c1.id)
                .await
                .unwrap()
                .external_id
                .as_deref(),
            Some("session-S1")
        );
        assert_eq!(
            conversation_service::get_by_id(&db.conn, result.forked_conversation_id)
                .await
                .unwrap()
                .external_id
                .as_deref(),
            Some("session-S2")
        );
    }
    // --- wait_for_session_options polling ----------------------------------
    //
    // These tests exercise the probe's wait loop directly by hand-seeding
    // `SessionState` on an injected connection. They avoid spawning a real
    // agent (which is what `probe_agent_options` itself would do) — the goal
    // is to lock in the three behaviors the public API depends on:
    //   1. ready+grace → Ok(snapshot) reflecting current state
    //   2. never-ready within timeout → Err(ProbeTimedOut), not Ok(empty)
    //   3. selectors_ready=true with empty options → Ok(empty snapshot)

    use crate::acp::types::{
        SessionConfigKindInfo, SessionConfigOptionInfo, SessionConfigSelectInfo, SessionModeInfo,
        SessionModeStateInfo,
    };

    fn sample_modes() -> SessionModeStateInfo {
        SessionModeStateInfo {
            current_mode_id: "default".into(),
            available_modes: vec![
                SessionModeInfo {
                    id: "default".into(),
                    name: "Default".into(),
                    description: None,
                },
                SessionModeInfo {
                    id: "yolo".into(),
                    name: "YOLO".into(),
                    description: None,
                },
            ],
        }
    }

    fn sample_config_options() -> Vec<SessionConfigOptionInfo> {
        vec![SessionConfigOptionInfo {
            id: "model".into(),
            name: "Model".into(),
            description: None,
            category: None,
            kind: SessionConfigKindInfo::Select(SessionConfigSelectInfo {
                current_value: "sonnet".into(),
                options: vec![],
                groups: vec![],
            }),
        }]
    }

    #[tokio::test]
    async fn wait_for_session_options_returns_snapshot_after_ready_plus_grace() {
        let mgr = ConnectionManager::new();
        mgr.insert_test_connection(
            "probe-1",
            crate::models::agent::AgentType::ClaudeCode,
            None,
            EventEmitter::Noop,
        )
        .await;
        // Seed the state the probe is waiting on. Done BEFORE the wait
        // starts so the very first poll already sees ready=true and only
        // the 500 ms grace period gates the return.
        {
            let state = mgr.get_state("probe-1").await.expect("state");
            let mut s = state.write().await;
            s.modes = Some(sample_modes());
            s.config_options = Some(sample_config_options());
            s.selectors_ready = true;
        }

        let start = std::time::Instant::now();
        let snapshot = mgr
            .wait_for_session_options("probe-1", Duration::from_secs(2))
            .await
            .expect("ready+grace path must return Ok");
        let elapsed = start.elapsed();

        assert!(
            elapsed >= Duration::from_millis(450),
            "expected ~500ms grace, observed {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_millis(1500),
            "should NOT wait the full 2s timeout, observed {elapsed:?}"
        );
        assert_eq!(snapshot.config_options.len(), 1);
        assert!(snapshot.modes.is_some());
    }

    #[tokio::test]
    async fn wait_for_session_options_times_out_when_selectors_never_ready() {
        let mgr = ConnectionManager::new();
        mgr.insert_test_connection(
            "probe-2",
            crate::models::agent::AgentType::ClaudeCode,
            None,
            EventEmitter::Noop,
        )
        .await;
        // Critical guarantee: even though `config_options` is `Some(...)`,
        // because `selectors_ready` is still false, the wait MUST timeout
        // and return Err — never Ok(empty) which would mislead the UI.
        {
            let state = mgr.get_state("probe-2").await.expect("state");
            let mut s = state.write().await;
            s.config_options = Some(vec![]);
            s.selectors_ready = false;
        }

        let err = mgr
            .wait_for_session_options("probe-2", Duration::from_millis(300))
            .await
            .expect_err("timeout path must return Err");
        assert!(
            matches!(err, AcpError::ProbeTimedOut),
            "expected ProbeTimedOut, got {err:?}"
        );
        assert_eq!(err.code(), Some("probe_timed_out"));
    }

    #[tokio::test]
    async fn wait_for_session_options_returns_empty_when_ready_with_no_options() {
        let mgr = ConnectionManager::new();
        mgr.insert_test_connection(
            "probe-3",
            crate::models::agent::AgentType::ClaudeCode,
            None,
            EventEmitter::Noop,
        )
        .await;
        // Real outcome the UI renders as "agent has nothing to configure":
        // selectors_ready=true, modes=None, config_options=None. Must
        // succeed, not error — this is the path that distinguishes a
        // legitimately empty agent from an unresponsive one.
        {
            let state = mgr.get_state("probe-3").await.expect("state");
            let mut s = state.write().await;
            s.modes = None;
            s.config_options = None;
            s.selectors_ready = true;
        }

        let snapshot = mgr
            .wait_for_session_options("probe-3", Duration::from_secs(2))
            .await
            .expect("ready-empty path must return Ok, not Err");
        assert!(snapshot.modes.is_none());
        assert!(snapshot.config_options.is_empty());
    }

    #[tokio::test]
    async fn wait_for_session_options_unknown_connection_errors_immediately() {
        let mgr = ConnectionManager::new();
        let err = mgr
            .wait_for_session_options("does-not-exist", Duration::from_secs(5))
            .await
            .expect_err("missing connection must error");
        assert!(
            matches!(err, AcpError::ConnectionNotFound(_)),
            "expected ConnectionNotFound, got {err:?}"
        );
    }

    #[tokio::test]
    async fn apply_event_error_populates_last_error_snapshot() {
        // Directly drives SessionState::apply_event to assert the Error
        // arm now writes `last_error` (rather than being a no-op as it
        // was before). The probe path reads this to surface the
        // agent's own error message after cleanup runs.
        use crate::acp::session_state::SessionState;
        let mut s = SessionState::new(
            "c1".into(),
            crate::models::agent::AgentType::ClaudeCode,
            None,
            "test-window".into(),
            None,
        );
        assert!(s.last_error.is_none(), "fresh state has no error");

        s.apply_event(&AcpEvent::Error {
            message: "agent exploded".into(),
            agent_type: "claude_code".into(),
            code: Some("sdk_not_installed".into()),
            details: None,
            terminal: true,
        });
        let captured = s.last_error.as_ref().expect("error must be captured");
        assert_eq!(captured.message, "agent exploded");
        assert_eq!(captured.code.as_deref(), Some("sdk_not_installed"));

        // A second Error event overwrites — `last_error` is "latest",
        // not "first". Keeps post-mortem reads aligned with what the
        // user most recently observed on the event channel.
        s.apply_event(&AcpEvent::Error {
            message: "second failure".into(),
            agent_type: "claude_code".into(),
            code: None,
            details: None,
            terminal: true,
        });
        let captured = s.last_error.as_ref().unwrap();
        assert_eq!(captured.message, "second failure");
        assert!(captured.code.is_none());
    }

    // --- live feedback: submit gate + consume drain --------------------

    /// Make a test connection feedback-capable AND mid-turn (the happy state).
    async fn mark_feedback_ready(mgr: &ConnectionManager, conn_id: &str) {
        let state = mgr.get_state(conn_id).await.unwrap();
        let mut s = state.write().await;
        s.feedback_tool_available = true;
        s.turn_in_flight = true;
    }

    async fn set_feedback_tool_available(mgr: &ConnectionManager, conn_id: &str) {
        let state = mgr.get_state(conn_id).await.unwrap();
        state.write().await.feedback_tool_available = true;
    }

    #[tokio::test]
    async fn submit_feedback_rejected_when_tool_unavailable() {
        let mgr = ConnectionManager::new();
        mgr.insert_test_connection("c1", AgentType::ClaudeCode, None, EventEmitter::Noop)
            .await;
        // feedback_tool_available defaults false: the agent never got the tool
        // (e.g. its session started before the feature was enabled), even mid-turn.
        let state = mgr.get_state("c1").await.unwrap();
        state.write().await.turn_in_flight = true;
        let err = mgr.submit_feedback("c1", "note".into()).await.unwrap_err();
        assert!(matches!(err, AcpError::FeedbackDisabled));
        assert!(state.read().await.feedback.is_empty());
    }

    #[tokio::test]
    async fn submit_feedback_rejected_when_no_turn_in_flight() {
        let mgr = ConnectionManager::new();
        mgr.insert_test_connection("c1", AgentType::ClaudeCode, None, EventEmitter::Noop)
            .await;
        // Tool available but no turn in flight → nothing to steer.
        set_feedback_tool_available(&mgr, "c1").await;
        let err = mgr.submit_feedback("c1", "note".into()).await.unwrap_err();
        assert!(matches!(err, AcpError::NoActiveTurn));
        // And nothing was appended.
        let state = mgr.get_state("c1").await.unwrap();
        assert!(state.read().await.feedback.is_empty());
    }

    #[tokio::test]
    async fn submit_feedback_missing_connection_errors() {
        let mgr = ConnectionManager::new();
        let err = mgr
            .submit_feedback("nope", "note".into())
            .await
            .unwrap_err();
        assert!(matches!(err, AcpError::ConnectionNotFound(_)));
    }

    #[tokio::test]
    async fn submit_feedback_appends_when_turn_in_flight() {
        let mgr = ConnectionManager::new();
        mgr.insert_test_connection("c1", AgentType::ClaudeCode, None, EventEmitter::Noop)
            .await;
        mark_feedback_ready(&mgr, "c1").await;
        let item = mgr
            .submit_feedback("c1", "  use UserService  ".into())
            .await
            .unwrap();
        assert_eq!(item.status, FeedbackStatus::Pending);
        // Stored text is trimmed.
        assert_eq!(item.text, "use UserService");
        let state = mgr.get_state("c1").await.unwrap();
        let s = state.read().await;
        assert_eq!(s.feedback.len(), 1);
        assert_eq!(s.feedback[0].text, "use UserService");
    }

    #[tokio::test]
    async fn submit_feedback_rejects_empty_and_oversized() {
        let mgr = ConnectionManager::new();
        mgr.insert_test_connection("c1", AgentType::ClaudeCode, None, EventEmitter::Noop)
            .await;
        mark_feedback_ready(&mgr, "c1").await;
        // Empty / whitespace-only → rejected, nothing appended.
        for empty in ["", "   ", "\n\t "] {
            let err = mgr.submit_feedback("c1", empty.into()).await.unwrap_err();
            assert!(matches!(err, AcpError::InvalidFeedback(_)));
        }
        // Oversized → rejected.
        let huge = "x".repeat(MAX_FEEDBACK_CHARS + 1);
        let err = mgr.submit_feedback("c1", huge).await.unwrap_err();
        assert!(matches!(err, AcpError::InvalidFeedback(_)));
        // Exactly at the bound is accepted.
        let at_bound = "y".repeat(MAX_FEEDBACK_CHARS);
        assert!(mgr.submit_feedback("c1", at_bound).await.is_ok());
        let state = mgr.get_state("c1").await.unwrap();
        assert_eq!(
            state.read().await.feedback.len(),
            1,
            "only the valid note stuck"
        );
    }

    // --- native steering (push channel) ----------------------------------

    async fn mark_native_steering_ready(mgr: &ConnectionManager, conn_id: &str) {
        let state = mgr.get_state(conn_id).await.unwrap();
        let mut s = state.write().await;
        s.native_steering_available = true;
        s.turn_in_flight = true;
    }

    /// Play the connection loop's role: receive one `Steer` command and reply
    /// the given outcome. Returns the text the command carried.
    fn answer_steer(
        mut rx: tokio::sync::mpsc::Receiver<ConnectionCommand>,
        outcome: Result<SteerOutcome, AcpError>,
    ) -> tokio::task::JoinHandle<String> {
        tokio::spawn(async move {
            match rx.recv().await {
                Some(ConnectionCommand::Steer { text, reply }) => {
                    let _ = reply.send(outcome);
                    text
                }
                _ => panic!("expected a Steer command"),
            }
        })
    }

    #[tokio::test]
    async fn native_submit_injected_records_delivered_and_pull_stays_empty() {
        let mgr = ConnectionManager::new();
        let rx = mgr
            .insert_test_connection_live("c1", AgentType::ClaudeCode, None, EventEmitter::Noop)
            .await;
        mark_native_steering_ready(&mgr, "c1").await;
        // The pull tool coexists by design — and must see NOTHING (the
        // no-double-delivery invariant: native notes are Delivered from birth,
        // `read_pending_feedback` only returns Pending).
        set_feedback_tool_available(&mgr, "c1").await;
        let fake_loop = answer_steer(rx, Ok(SteerOutcome::Injected));

        let item = mgr
            .submit_feedback("c1", "  ship it  ".into())
            .await
            .unwrap();
        assert_eq!(item.status, FeedbackStatus::Delivered);
        assert!(item.delivered_at.is_some());
        assert_eq!(item.text, "ship it");
        // The wire carried the trimmed text.
        assert_eq!(fake_loop.await.unwrap(), "ship it");

        let state = mgr.get_state("c1").await.unwrap();
        {
            let s = state.read().await;
            assert_eq!(s.feedback.len(), 1);
            assert_eq!(s.feedback[0].status, FeedbackStatus::Delivered);
            assert!(s.native_steering_available, "injected must not downgrade");
        }
        assert!(
            mgr.read_pending_feedback("c1").await.is_empty(),
            "a check_user_feedback pull must never re-deliver a natively-injected note"
        );
    }

    #[tokio::test]
    async fn native_submit_prompt_required_maps_to_no_active_turn_and_records_nothing() {
        let mgr = ConnectionManager::new();
        let rx = mgr
            .insert_test_connection_live("c1", AgentType::ClaudeCode, None, EventEmitter::Noop)
            .await;
        mark_native_steering_ready(&mgr, "c1").await;
        let fake_loop = answer_steer(rx, Ok(SteerOutcome::PromptRequired));

        let err = mgr.submit_feedback("c1", "note".into()).await.unwrap_err();
        assert!(matches!(err, AcpError::NoActiveTurn));
        let _ = fake_loop.await;

        let state = mgr.get_state("c1").await.unwrap();
        let s = state.read().await;
        // Content was NOT consumed (honored opt-in): nothing recorded, and the
        // channel stays native — the caller resubmits as a normal prompt.
        assert!(s.feedback.is_empty());
        assert!(s.native_steering_available);
    }

    #[tokio::test]
    async fn native_submit_started_new_turn_records_delivered_and_downgrades_to_pull() {
        let mgr = ConnectionManager::new();
        let rx = mgr
            .insert_test_connection_live("c1", AgentType::ClaudeCode, None, EventEmitter::Noop)
            .await;
        mark_native_steering_ready(&mgr, "c1").await;
        set_feedback_tool_available(&mgr, "c1").await;
        let fake_loop = answer_steer(rx, Ok(SteerOutcome::StartedNewTurn));

        // The adapter ignored the opt-in: content consumed → recorded
        // Delivered (never resent), and the session downgrades to pull.
        let item = mgr.submit_feedback("c1", "note one".into()).await.unwrap();
        assert_eq!(item.status, FeedbackStatus::Delivered);
        let _ = fake_loop.await;
        let state = mgr.get_state("c1").await.unwrap();
        assert!(
            !state.read().await.native_steering_available,
            "startedNewTurn must downgrade native steering for the session"
        );

        // The NEXT note rides the pull path: lands Pending, no Steer command
        // (the loop receiver was consumed above — a native attempt would fail
        // on the dead channel, so an Ok(Pending) proves the pull branch ran).
        let second = mgr.submit_feedback("c1", "note two".into()).await.unwrap();
        assert_eq!(second.status, FeedbackStatus::Pending);
        let pending = mgr.read_pending_feedback("c1").await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].text, "note two");
    }

    #[tokio::test]
    async fn native_submit_records_even_when_turn_settles_before_the_reply() {
        // The ungated-emit invariant: the adapter's `injected` reply is
        // authoritative even if `TurnComplete` lands first. A gated emit here
        // would misreport NoActiveTurn → the frontend would resend the same
        // text as a prompt → duplicate.
        let mgr = ConnectionManager::new();
        let rx = mgr
            .insert_test_connection_live("c1", AgentType::ClaudeCode, None, EventEmitter::Noop)
            .await;
        mark_native_steering_ready(&mgr, "c1").await;
        let state = mgr.get_state("c1").await.unwrap();

        let state_for_loop = state.clone();
        let mut rx = rx;
        let fake_loop = tokio::spawn(async move {
            match rx.recv().await {
                Some(ConnectionCommand::Steer { reply, .. }) => {
                    // The turn settles BETWEEN the injection and the reply.
                    state_for_loop.write().await.turn_in_flight = false;
                    let _ = reply.send(Ok(SteerOutcome::Injected));
                }
                _ => panic!("expected a Steer command"),
            }
        });

        let item = mgr.submit_feedback("c1", "late note".into()).await.unwrap();
        assert_eq!(item.status, FeedbackStatus::Delivered);
        let _ = fake_loop.await;
        assert_eq!(state.read().await.feedback.len(), 1);

        // The next turn's UserMessage clears the (turn-scoped) note — the
        // post-turn residue is bounded and harmless.
        state.write().await.apply_event(&AcpEvent::UserMessage {
            message_id: "m-next".into(),
            blocks: Vec::new(),
        });
        assert!(state.read().await.feedback.is_empty());
    }

    #[tokio::test]
    async fn native_submit_records_despite_caller_cancellation() {
        // Cancellation-shield regression, mirroring
        // `fork_persists_despite_caller_cancellation`: once `Steer` is
        // enqueued the adapter may consume the text regardless of caller
        // liveness, so the Delivered record + broadcast must not be tied to
        // the caller's future.
        let mgr = ConnectionManager::new();
        let rx = mgr
            .insert_test_connection_live("c1", AgentType::ClaudeCode, None, EventEmitter::Noop)
            .await;
        mark_native_steering_ready(&mgr, "c1").await;

        let (go_tx, go_rx) = tokio::sync::oneshot::channel::<()>();
        let mut rx = rx;
        let fake_loop = tokio::spawn(async move {
            if let Some(ConnectionCommand::Steer { reply, .. }) = rx.recv().await {
                go_rx.await.ok(); // withhold the outcome until the caller is gone
                let _ = reply.send(Ok(SteerOutcome::Injected));
            }
            rx // keep the receiver alive
        });

        // Drive the submit under a short timeout: the shielded task enqueues
        // `Steer` and blocks on the withheld reply; the timeout DROPS this
        // caller future.
        let timed = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            mgr.submit_feedback("c1", "shielded note".into()),
        )
        .await;
        assert!(
            timed.is_err(),
            "caller must be dropped before the withheld outcome is delivered"
        );

        // Release the outcome: the DETACHED task records the note anyway.
        go_tx.send(()).ok();
        let _ = fake_loop.await;
        let state = mgr.get_state("c1").await.unwrap();
        let mut recorded = false;
        for _ in 0..200 {
            {
                let s = state.read().await;
                if s.feedback.len() == 1 && s.feedback[0].status == FeedbackStatus::Delivered {
                    recorded = true;
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(
            recorded,
            "native delivery must be recorded despite caller cancellation"
        );
    }

    #[tokio::test]
    async fn native_only_session_accepts_notes_without_the_pull_tool() {
        // The native channel needs no MCP companion (that's the point):
        // eligibility is channel-aware, not tool-only.
        let mgr = ConnectionManager::new();
        let rx = mgr
            .insert_test_connection_live("c1", AgentType::ClaudeCode, None, EventEmitter::Noop)
            .await;
        mark_native_steering_ready(&mgr, "c1").await;
        // feedback_tool_available stays false.
        let fake_loop = answer_steer(rx, Ok(SteerOutcome::Injected));
        let item = mgr
            .submit_feedback("c1", "no tool needed".into())
            .await
            .unwrap();
        assert_eq!(item.status, FeedbackStatus::Delivered);
        let _ = fake_loop.await;
    }

    // --- ask_user_question: register / answer / cancel -------------------

    fn q_spec() -> Vec<QuestionSpec> {
        vec![crate::acp::question::QuestionSpec {
            id: "qa".into(),
            question: "Which approach?".into(),
            header: "Approach".into(),
            multi_select: false,
            options: vec![
                crate::acp::question::QuestionOption {
                    label: "A".into(),
                    description: String::new(),
                },
                crate::acp::question::QuestionOption {
                    label: "B".into(),
                    description: String::new(),
                },
            ],
            is_secret: false,
        }]
    }

    #[tokio::test]
    async fn register_then_answer_question_resolves_and_clears() {
        let mgr = ConnectionManager::new();
        mgr.insert_test_connection("cq", AgentType::ClaudeCode, None, EventEmitter::Noop)
            .await;
        let reg = mgr
            .register_question("cq", q_spec())
            .await
            .expect("registered");
        // SessionState reflects the pending question for snapshot recovery.
        assert!(mgr
            .get_state("cq")
            .await
            .unwrap()
            .read()
            .await
            .pending_question
            .is_some());

        let answer = crate::acp::question::QuestionAnswer {
            answers: vec![crate::acp::question::QuestionAnswerItem {
                question_id: "qa".into(),
                labels: vec!["A".into()],
            }],
            declined: false,
        };
        mgr.answer_question("cq", &reg.question_id, answer)
            .await
            .unwrap();

        // The blocked listener's receiver resolves with the self-describing
        // outcome (question text joined in).
        let outcome = reg.answer_rx.await.expect("answer delivered");
        assert!(!outcome.declined);
        assert_eq!(outcome.answers.len(), 1);
        assert_eq!(outcome.answers[0].question, "Which approach?");
        assert_eq!(outcome.answers[0].selected, vec!["A".to_string()]);
        // pending_question cleared after resolve.
        assert!(mgr
            .get_state("cq")
            .await
            .unwrap()
            .read()
            .await
            .pending_question
            .is_none());

        // Idempotent: answering an already-resolved id is a no-op success.
        mgr.answer_question("cq", &reg.question_id, Default::default())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn cancel_question_clears_and_drops_sender() {
        let mgr = ConnectionManager::new();
        mgr.insert_test_connection("cqx", AgentType::ClaudeCode, None, EventEmitter::Noop)
            .await;
        let reg = mgr.register_question("cqx", q_spec()).await.unwrap();
        mgr.cancel_question("cqx", &reg.question_id).await;
        // Dropping the sender surfaces to the parked listener as a recv error
        // (which it renders as a declined outcome).
        assert!(reg.answer_rx.await.is_err());
        assert!(mgr
            .get_state("cqx")
            .await
            .unwrap()
            .read()
            .await
            .pending_question
            .is_none());
    }

    #[tokio::test]
    async fn cancel_questions_by_parent_drops_only_matching_connection() {
        // The run_connection teardown guard sweeps a tearing-down connection's
        // parked ask without touching other connections' pending questions.
        let mgr = ConnectionManager::new();
        mgr.insert_test_connection("ca", AgentType::ClaudeCode, None, EventEmitter::Noop)
            .await;
        mgr.insert_test_connection("cb", AgentType::ClaudeCode, None, EventEmitter::Noop)
            .await;
        let reg_a = mgr.register_question("ca", q_spec()).await.unwrap();
        let reg_b = mgr.register_question("cb", q_spec()).await.unwrap();

        // Tear down only connection "ca".
        mgr.cancel_questions_by_parent("ca").await;

        // ca's parked listener is unblocked (sender dropped → recv error) and its
        // card cleared; cb is untouched.
        assert!(reg_a.answer_rx.await.is_err());
        assert!(mgr
            .get_state("ca")
            .await
            .unwrap()
            .read()
            .await
            .pending_question
            .is_none());
        assert!(mgr
            .get_state("cb")
            .await
            .unwrap()
            .read()
            .await
            .pending_question
            .is_some());

        // cb still resolves normally afterwards.
        mgr.answer_question("cb", &reg_b.question_id, Default::default())
            .await
            .unwrap();
        assert!(reg_b.answer_rx.await.is_ok());
    }

    #[tokio::test]
    async fn register_then_answer_plan_approval_resolves_and_clears() {
        let mgr = ConnectionManager::new();
        mgr.insert_test_connection("pa", AgentType::Grok, None, EventEmitter::Noop)
            .await;
        let reg = mgr
            .register_plan_approval("pa", "call-1".into(), "# Plan\n- step".into())
            .await
            .expect("registered");
        // SessionState reflects the pending approval for snapshot recovery.
        {
            let state = mgr.get_state("pa").await.unwrap();
            let guard = state.read().await;
            let pending = guard.pending_plan_approval.as_ref().expect("pending set");
            assert_eq!(pending.plan_markdown, "# Plan\n- step");
            assert_eq!(pending.tool_call_id, "call-1");
        }

        mgr.answer_plan_approval(
            "pa",
            &reg.approval_id,
            crate::acp::plan_approval::PlanApprovalAnswer {
                decision: crate::acp::plan_approval::PlanApprovalDecision::RequestChanges,
                feedback: Some("use SSE".into()),
            },
        )
        .await
        .unwrap();

        // The blocked ext handler's receiver resolves with the user's decision.
        let got = reg.answer_rx.await.expect("answer delivered");
        assert_eq!(
            got.decision,
            crate::acp::plan_approval::PlanApprovalDecision::RequestChanges
        );
        assert_eq!(got.feedback.as_deref(), Some("use SSE"));
        // pending_plan_approval cleared after resolve.
        assert!(mgr
            .get_state("pa")
            .await
            .unwrap()
            .read()
            .await
            .pending_plan_approval
            .is_none());

        // Idempotent: answering an already-resolved id is a no-op success.
        mgr.answer_plan_approval(
            "pa",
            &reg.approval_id,
            crate::acp::plan_approval::PlanApprovalAnswer {
                decision: crate::acp::plan_approval::PlanApprovalDecision::Approve,
                feedback: None,
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn register_plan_approval_refuses_second_pending_on_same_connection() {
        // At most one approval per connection (the agent is blocked in exit_plan_mode).
        let mgr = ConnectionManager::new();
        mgr.insert_test_connection("pa2", AgentType::Grok, None, EventEmitter::Noop)
            .await;
        let _reg = mgr
            .register_plan_approval("pa2", "c1".into(), "plan".into())
            .await
            .expect("first registers");
        assert!(mgr
            .register_plan_approval("pa2", "c2".into(), "plan2".into())
            .await
            .is_none());
    }

    #[tokio::test]
    async fn cancel_plan_approvals_by_parent_drops_sender_and_clears() {
        let mgr = ConnectionManager::new();
        mgr.insert_test_connection("pax", AgentType::Grok, None, EventEmitter::Noop)
            .await;
        let reg = mgr
            .register_plan_approval("pax", "c".into(), "plan".into())
            .await
            .unwrap();
        mgr.cancel_plan_approvals_by_parent("pax").await;
        // Dropping the sender surfaces to the parked handler as a recv error
        // (which it renders as a disconnect reply — plan mode stays active).
        assert!(reg.answer_rx.await.is_err());
        assert!(mgr
            .get_state("pax")
            .await
            .unwrap()
            .read()
            .await
            .pending_plan_approval
            .is_none());
    }

    #[tokio::test]
    async fn compensate_clears_card_when_entry_drained_before_request_emit() {
        // Regression for the teardown event-ordering race: register inserts, the
        // sweep drains the entry, THEN register's QuestionRequest emit lands. The
        // post-emit presence check must emit a compensating QuestionResolved so no
        // client keeps a card with no live backend waiter, and signal decline.
        let mgr = ConnectionManager::new();
        mgr.insert_test_connection("cc", AgentType::ClaudeCode, None, EventEmitter::Noop)
            .await;
        let (state, emitter) = mgr.get_state_and_emitter("cc").await.unwrap();

        // Simulate register's QuestionRequest emit for an entry that has already
        // been drained (never inserted here): the card shows, nothing is parked.
        emit_with_state(
            &state,
            &emitter,
            AcpEvent::QuestionRequest {
                question_id: "q1".into(),
                questions: q_spec(),
            },
        )
        .await;
        assert!(state.read().await.pending_question.is_some(), "card shown");

        // Missing entry → compensate clears the card and reports decline.
        assert!(
            mgr.compensate_if_question_drained("q1", &state, &emitter)
                .await,
            "missing entry is compensated"
        );
        assert!(
            state.read().await.pending_question.is_none(),
            "compensating QuestionResolved cleared the card"
        );

        // A genuinely-parked entry is left alone (no false compensation).
        let reg = mgr.register_question("cc", q_spec()).await.unwrap();
        assert!(
            !mgr.compensate_if_question_drained(&reg.question_id, &state, &emitter)
                .await,
            "present entry is not compensated"
        );
        assert!(state.read().await.pending_question.is_some());
    }

    #[tokio::test]
    async fn register_question_unknown_connection_is_none() {
        let mgr = ConnectionManager::new();
        assert!(mgr.register_question("nope", q_spec()).await.is_none());
    }

    #[tokio::test]
    async fn second_concurrent_ask_is_refused_and_first_stays_answerable() {
        // A parallel/misbehaving client could fire two asks on one connection
        // before the first resolves. The single-slot card/snapshot can't hold
        // two, so the second is refused (None → declined) and the FIRST stays
        // intact and answerable — never orphaned.
        let mgr = ConnectionManager::new();
        mgr.insert_test_connection("cc2", AgentType::ClaudeCode, None, EventEmitter::Noop)
            .await;
        let first = mgr
            .register_question("cc2", q_spec())
            .await
            .expect("first registers");
        // Second concurrent ask on the same connection is refused.
        assert!(
            mgr.register_question("cc2", q_spec()).await.is_none(),
            "second concurrent ask must be refused"
        );
        // The first is still the pending one and still answerable.
        let state = mgr.get_state("cc2").await.unwrap();
        assert_eq!(
            state
                .read()
                .await
                .pending_question
                .as_ref()
                .map(|p| p.question_id.clone()),
            Some(first.question_id.clone())
        );
        mgr.answer_question(
            "cc2",
            &first.question_id,
            crate::acp::question::QuestionAnswer {
                answers: vec![crate::acp::question::QuestionAnswerItem {
                    question_id: "qa".into(),
                    labels: vec!["A".into()],
                }],
                declined: false,
            },
        )
        .await
        .unwrap();
        assert!(first.answer_rx.await.is_ok(), "first ask resolves");
        // After resolve, a new ask is accepted again.
        assert!(mgr.register_question("cc2", q_spec()).await.is_some());
    }

    #[tokio::test]
    async fn read_pending_is_readonly_commit_marks_delivered() {
        let mgr = ConnectionManager::new();
        mgr.insert_test_connection("c1", AgentType::ClaudeCode, None, EventEmitter::Noop)
            .await;
        mark_feedback_ready(&mgr, "c1").await;
        let a = mgr.submit_feedback("c1", "a".into()).await.unwrap();
        let b = mgr.submit_feedback("c1", "b".into()).await.unwrap();

        // READ returns both pending notes (insert order) WITHOUT mutating state.
        let pending = mgr.read_pending_feedback("c1").await;
        let texts: Vec<&str> = pending.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(texts, vec!["a", "b"]);
        // A second read still returns them — read is non-destructive, so an
        // abandoned (peer-closed) call leaves the notes retryable.
        assert_eq!(mgr.read_pending_feedback("c1").await.len(), 2);
        {
            let state = mgr.get_state("c1").await.unwrap();
            assert!(state
                .read()
                .await
                .feedback
                .iter()
                .all(|f| f.status == FeedbackStatus::Pending));
        }

        // COMMIT marks the named notes delivered.
        mgr.commit_feedback_delivered("c1", vec![a.id.clone(), b.id.clone()])
            .await;
        // Now READ returns nothing (delivered notes are filtered out).
        assert!(mgr.read_pending_feedback("c1").await.is_empty());
        let state = mgr.get_state("c1").await.unwrap();
        assert!(state
            .read()
            .await
            .feedback
            .iter()
            .all(|f| f.status == FeedbackStatus::Delivered));

        // COMMIT is idempotent — re-committing already-delivered ids is a no-op.
        mgr.commit_feedback_delivered("c1", vec![a.id, b.id]).await;
    }

    #[tokio::test]
    async fn read_pending_missing_connection_returns_empty() {
        let mgr = ConnectionManager::new();
        assert!(mgr.read_pending_feedback("nope").await.is_empty());
        // Commit on a missing connection is a safe no-op.
        mgr.commit_feedback_delivered("nope", vec!["x".into()])
            .await;
    }
}
