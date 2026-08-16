//! Main-process side of the `codeg-mcp` round-trip: accept UDS / named-pipe
//! connections from companion processes, validate the per-launch token,
//! resolve the caller's current conversation, and hand off to codeg's host
//! services. This is the token policy boundary for the shared companion.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::RwLock;

use crate::acp::chat_authoring::{AuthoringContext, AuthoringOutcome, ChatAuthoringAccess};
use crate::acp::delegation::transport::{
    read_frame, write_frame, BrokerAskRequest, BrokerCommitFeedbackRequest,
    BrokerCreateAutomationRequest, BrokerCreateWorkTaskRequest, BrokerFeedbackRequest,
    BrokerHostControlHelpRequest, BrokerHostControlUseRequest, BrokerListSessionsRequest,
    BrokerMessage, BrokerResponse, BrokerSendMessageRequest, BrokerSessionRequest,
    BrokerTaskCompleteRequest, BrokerTaskProgressRequest,
};
use crate::acp::feedback::{PendingFeedback, SessionFeedbackAccess};
use crate::acp::host_control::{
    HostControlAccess, HostControlCaller, HostControlHelpOutcome, HostControlUseOutcome,
};
use crate::acp::question::{QuestionOutcome, SessionQuestionAccess};
use crate::acp::session_collaboration::{
    SessionCollaborationAccess, SessionListOutcome, SessionSendOutcome,
};
use crate::acp::session_info::{SessionInfo, SessionInfoAccess};
use crate::acp::work_task_tools::{TaskReportAck, WorkTaskToolAccess};
use serde_json::Value;

/// Pluggable "what conversation is this parent currently in?" lookup. The
/// production impl wraps `ConnectionManager.get_state`; tests use an
/// in-memory map.
///
/// Kept as a trait so the listener can be unit-tested without spinning up a
/// real `ConnectionManager` or RwLock<SessionState>.
#[async_trait]
pub trait ParentSessionLookup: Send + Sync {
    async fn current_conversation_id(&self, parent_connection_id: &str) -> Option<i32>;
}

/// Per-launch token entry. Bound at MCP injection time and revoked on parent
/// connection teardown.
#[derive(Debug, Clone)]
pub struct TokenEntry {
    pub parent_connection_id: String,
    pub working_dir: PathBuf,
    /// Parent launch policy, bound by trusted connection code rather than MCP
    /// arguments. The Host Control core checks this again on every write.
    pub host_control_writes_allowed: bool,
}

#[derive(Default)]
pub struct TokenRegistry {
    inner: RwLock<HashMap<String, TokenEntry>>,
}

impl TokenRegistry {
    pub async fn register(&self, token: String, entry: TokenEntry) {
        self.inner.write().await.insert(token, entry);
    }

    pub async fn revoke(&self, token: &str) {
        self.inner.write().await.remove(token);
    }

    pub async fn lookup(&self, token: &str) -> Option<TokenEntry> {
        self.inner.read().await.get(token).cloned()
    }

    /// Drop every token whose `parent_connection_id` matches. Used on parent
    /// connection teardown so a leaked token can't be reused.
    pub async fn revoke_by_parent(&self, parent_connection_id: &str) {
        let mut map = self.inner.write().await;
        map.retain(|_, entry| entry.parent_connection_id != parent_connection_id);
    }
}

pub struct HostBridgeListener {
    pub tokens: Arc<TokenRegistry>,
    pub parent_lookup: Arc<dyn ParentSessionLookup>,
    /// Progressive Host Control catalog + typed action dispatcher. The
    /// listener supplies its trusted caller context from the token entry.
    pub host_control: Arc<dyn HostControlAccess>,
    /// Pulls pending live-feedback notes for the `check_user_feedback` tool.
    /// Shares the same `tokens` registry and parent-connection scoping as the
    /// other host bridge tools.
    pub feedback: Arc<dyn SessionFeedbackAccess>,
    /// Registers / cancels the blocking `ask_user_question` tool's pending
    /// questions. Same `tokens` registry and parent-connection scoping.
    pub questions: Arc<dyn SessionQuestionAccess>,
    /// Resolves a referenced session for the `get_session_info` tool. Unlike the
    /// other arms this is NOT parent-scoped — it looks any non-deleted session up
    /// by its codeg conversation id (still token-gated against an invalid caller).
    pub session_info: Arc<dyn SessionInfoAccess>,
    /// Stable Session address lookup and persistent cross-Session delivery.
    /// The listener derives the source from the token's parent connection; the
    /// access implementation re-checks the write capability at call time.
    pub collaboration: Arc<dyn SessionCollaborationAccess>,
    /// Records work-task reports (`task_progress` / `task_complete`) against the
    /// task the parent connection is executing. Same token → parent-connection
    /// scoping as the other host bridge tools.
    pub tasks: Arc<dyn WorkTaskToolAccess>,
    /// Creates automations / board tasks on behalf of the chat that asked
    /// (`create_automation` / `create_work_task`). The impl re-checks the
    /// feature flags at call time, so flipping the setting off stops writes
    /// from sessions that were launched while it was on.
    pub authoring: Arc<dyn ChatAuthoringAccess>,
}

impl HostBridgeListener {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tokens: Arc<TokenRegistry>,
        parent_lookup: Arc<dyn ParentSessionLookup>,
        host_control: Arc<dyn HostControlAccess>,
        feedback: Arc<dyn SessionFeedbackAccess>,
        questions: Arc<dyn SessionQuestionAccess>,
        session_info: Arc<dyn SessionInfoAccess>,
        collaboration: Arc<dyn SessionCollaborationAccess>,
        tasks: Arc<dyn WorkTaskToolAccess>,
        authoring: Arc<dyn ChatAuthoringAccess>,
    ) -> Arc<Self> {
        Arc::new(Self {
            tokens,
            parent_lookup,
            host_control,
            feedback,
            questions,
            session_info,
            collaboration,
            tasks,
            authoring,
        })
    }

    /// Run the accept loop until the socket is unbound. Errors on accept are
    /// logged and the loop continues — a single bad connection can't bring
    /// down the listener.
    #[cfg(unix)]
    pub async fn run(self: Arc<Self>, socket_path: PathBuf) -> std::io::Result<()> {
        let _ = tokio::fs::remove_file(&socket_path).await;
        if let Some(parent) = socket_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let listener = tokio::net::UnixListener::bind(&socket_path)?;
        tracing::info!(
            "[codeg-mcp] host bridge listening on UDS {}",
            socket_path.display()
        );
        loop {
            match listener.accept().await {
                Ok((mut conn, _)) => {
                    let me = Arc::clone(&self);
                    tokio::spawn(async move {
                        if let Err(e) = me.serve_one(&mut conn).await {
                            tracing::error!("[codeg-mcp] host bridge connection failed: {e}");
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("[codeg-mcp] host bridge accept failed: {e}");
                    // Brief backoff so a persistent accept error doesn't pin a core.
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    }

    /// Windows variant: bind a named pipe and follow Tokio's recommended
    /// accept pattern — wait for a connect, immediately create the *next*
    /// server instance, then hand the connected instance off to a worker.
    /// This keeps a pipe instance available at all times, so clients calling
    /// `ClientOptions::open()` between connections don't see `NotFound`.
    #[cfg(windows)]
    pub async fn run(self: Arc<Self>, socket_path: PathBuf) -> std::io::Result<()> {
        use tokio::net::windows::named_pipe::ServerOptions;
        let path_str = socket_path.to_string_lossy().to_string();
        let mut server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&path_str)?;
        tracing::info!("[codeg-mcp] host bridge listening on named pipe {path_str}");
        loop {
            if let Err(e) = server.connect().await {
                tracing::error!("[codeg-mcp] host bridge connect failed: {e}");
                // Re-create the instance so the next iteration has a fresh
                // listener; a failed connect leaves the current one unusable.
                server = ServerOptions::new().create(&path_str)?;
                continue;
            }
            let connected = server;
            // Re-bind BEFORE serving the current client, so a client that
            // opens during this turn finds a server instance to connect to.
            server = ServerOptions::new().create(&path_str)?;
            let me = Arc::clone(&self);
            tokio::spawn(async move {
                let mut conn = connected;
                if let Err(e) = me.serve_one(&mut conn).await {
                    tracing::error!("[codeg-mcp] host bridge connection failed: {e}");
                }
            });
        }
    }

    /// Stream-generic per-connection handler. Exposed so unit tests can drive
    /// it over `tokio::io::duplex` instead of a real socket.
    pub async fn serve_one<C>(&self, conn: &mut C) -> std::io::Result<()>
    where
        C: AsyncReadExt + AsyncWriteExt + Unpin,
    {
        let msg: BrokerMessage = read_frame(conn).await?;
        let resp = match msg {
            BrokerMessage::HostControlHelp(req) => {
                host_control_help_response(self.process_host_control_help(req).await)?
            }
            BrokerMessage::HostControlUse(req) => {
                host_control_use_response(self.process_host_control_use(req).await)?
            }
            BrokerMessage::Feedback(req) => {
                // at-least-once delivery: READ pending notes (no mutation),
                // WRITE the response, and COMMIT them delivered ONLY on a
                // successful write. A dropped/failed write skips the commit, so
                // the notes stay pending for the agent's next check.
                match self.feedback_target(&req).await {
                    None => {
                        // Invalid token: return an empty envelope (no leak of
                        // whether any feedback exists), nothing to commit.
                        write_frame(conn, &feedback_response(&[])?).await?;
                    }
                    Some(parent_conn_id) => {
                        let pending = self.feedback.read_pending_feedback(&parent_conn_id).await;
                        // Read-only: the response carries the note ids
                        // (`_commit_ids`); delivery is committed LATER, by the
                        // companion's `CommitFeedback` once it actually returns
                        // the result to the agent. So a cancel that suppresses
                        // the agent-facing response leaves the notes pending.
                        write_frame(conn, &feedback_response(&pending)?).await?;
                    }
                }
                return Ok(());
            }
            BrokerMessage::CommitFeedback(req) => {
                self.process_commit_feedback(req).await;
                // Empty ack so the companion can confirm the listener saw it.
                BrokerResponse {
                    outcome: Value::Null,
                }
            }
            BrokerMessage::Ask(req) => {
                // Register the question (broadcasting the card) and park until
                // the user answers, racing peer-close.
                // The companion holds this connection open for the whole wait
                // and never writes a second frame, so the probe read only
                // resolves on EOF/error; a canceled tool call drops the
                // companion's future, closing this socket, which we observe and
                // tear the pending question down. An invalid token, a gone
                // connection, or a connection that already has a pending ask
                // (one-at-a-time) yields a `declined` outcome (the LLM proceeds
                // with its own judgment) rather than hanging.
                let Some(parent_conn_id) = self.ask_target(&req).await else {
                    write_frame(conn, &ask_declined_response()?).await?;
                    return Ok(());
                };
                let Some(reg) = self
                    .questions
                    .register_question(&parent_conn_id, req.questions)
                    .await
                else {
                    write_frame(conn, &ask_declined_response()?).await?;
                    return Ok(());
                };
                let question_id = reg.question_id;
                let mut answer_rx = reg.answer_rx;
                // Close the teardown race: `ask_target` validated the token, but the
                // parent connection may have been revoked + swept
                // (`cancel_questions_by_parent`) in the window before the insert
                // above — the sweep would have missed this just-registered entry,
                // leaving it parked until peer-close. The token is revoked before
                // the sweep, so a re-check that now finds it gone means teardown is
                // underway: cancel immediately so the ask can't linger.
                if self.tokens.lookup(&req.token).await.is_none() {
                    self.questions
                        .cancel_question(&parent_conn_id, &question_id)
                        .await;
                    write_frame(conn, &ask_declined_response()?).await?;
                    return Ok(());
                }
                let mut probe = [0u8; 1];
                let outcome = tokio::select! {
                    biased;
                    ans = &mut answer_rx => ans.ok(),
                    _ = conn.read(&mut probe) => {
                        self.questions
                            .cancel_question(&parent_conn_id, &question_id)
                            .await;
                        return Ok(());
                    }
                };
                let resp = match outcome {
                    Some(o) => ask_response(&o)?,
                    // Sender dropped without sending (connection teardown drain):
                    // surface a declined outcome so the tool returns cleanly.
                    None => ask_declined_response()?,
                };
                write_frame(conn, &resp).await?;
                return Ok(());
            }
            BrokerMessage::SessionInfo(req) => {
                // Read-only resolution (DB + a bounded transcript parse). No
                // peer-close race needed: unlike Ask this never blocks on
                // a long-poll or a human — the bounded parse always completes —
                // and there is nothing to tear down on cancel.
                session_response(self.process_session_info(req).await)?
            }
            BrokerMessage::ListSessions(req) => {
                session_list_response(self.process_list_sessions(req).await)?
            }
            BrokerMessage::SendMessage(req) => {
                session_send_response(self.process_send_message(req).await)?
            }
            BrokerMessage::TaskProgress(req) => {
                task_ack_response(self.process_task_progress(req).await)?
            }
            BrokerMessage::TaskComplete(req) => {
                task_ack_response(self.process_task_complete(req).await)?
            }
            BrokerMessage::CreateAutomation(req) => {
                // A bounded DB write. Like SessionInfo it never long-polls, so
                // there is no peer-close race to run — and unlike Ask there is
                // nothing to tear down if the caller cancels: either the row
                // landed or it didn't, and the response is simply dropped.
                authoring_response(self.process_create_automation(req).await)?
            }
            BrokerMessage::CreateWorkTask(req) => {
                authoring_response(self.process_create_work_task(req).await)?
            }
        };
        write_frame(conn, &resp).await?;
        Ok(())
    }

    /// Validate the token and resolve the `check_user_feedback` target: the
    /// caller's parent connection id. `None` on an invalid token — the LLM can't
    /// usefully distinguish "no notes" from "bad token", and we don't leak which.
    async fn feedback_target(&self, req: &BrokerFeedbackRequest) -> Option<String> {
        let entry = self.tokens.lookup(&req.token).await?;
        Some(entry.parent_connection_id)
    }

    /// Derive the Host Control caller exclusively from the opaque launch token
    /// and its live parent connection. No source/current Session identity is
    /// accepted on the MCP or broker request.
    async fn host_control_caller(&self, token: &str) -> Option<HostControlCaller> {
        let entry = self.tokens.lookup(token).await?;
        let current_session_id = self
            .parent_lookup
            .current_conversation_id(&entry.parent_connection_id)
            .await?;
        Some(HostControlCaller {
            current_session_id,
            working_dir: entry.working_dir,
            writes_allowed: entry.host_control_writes_allowed,
        })
    }

    async fn process_host_control_help(
        &self,
        req: BrokerHostControlHelpRequest,
    ) -> HostControlHelpOutcome {
        let Some(caller) = self.host_control_caller(&req.token).await else {
            return HostControlHelpOutcome::unavailable(
                "This Codeg Session identity has expired or is not persistent. Resume the Session before using Host Control.",
            );
        };
        self.host_control.help(caller, req.query, req.action).await
    }

    async fn process_host_control_use(
        &self,
        req: BrokerHostControlUseRequest,
    ) -> HostControlUseOutcome {
        let Some(caller) = self.host_control_caller(&req.token).await else {
            return HostControlUseOutcome::rejected(
                req.request_id,
                req.action,
                "This Codeg Session identity has expired or is not persistent. Resume the Session before using Host Control.",
            );
        };
        self.host_control
            .use_action(caller, req.request_id, req.action, req.input)
            .await
    }

    /// Validate the token and resolve the `ask_user_question` target: the
    /// caller's parent connection id. `None` on an invalid token — the LLM gets
    /// a `declined` outcome (proceed with judgment), and we don't leak which.
    async fn ask_target(&self, req: &BrokerAskRequest) -> Option<String> {
        let entry = self.tokens.lookup(&req.token).await?;
        Some(entry.parent_connection_id)
    }

    /// Mark the named feedback notes delivered, after the companion confirms it
    /// returned them to the agent. Token-scoped to the parent connection. Unknown
    /// tokens are dropped (no LLM on the receiving end to react).
    async fn process_commit_feedback(&self, req: BrokerCommitFeedbackRequest) {
        let Some(entry) = self.tokens.lookup(&req.token).await else {
            return;
        };
        self.feedback
            .commit_feedback_delivered(&entry.parent_connection_id, req.ids)
            .await;
    }

    /// Validate the token and resolve the `get_session_info` target. An invalid
    /// token yields a `found:false` outcome (the LLM can't usefully distinguish it
    /// from a deleted session, and we don't leak which).
    ///
    /// SCOPE (deliberate, user-confirmed): the lookup is by codeg conversation id
    /// and is intentionally NOT scoped to the caller's parent connection or to the
    /// session ids actually referenced in the prompt — any non-deleted session
    /// resolves. This is sound in codeg's single-tenant trust model: there is no
    /// per-user isolation anywhere (desktop is one local user; server mode shares
    /// one `CODEG_TOKEN` + one data dir across an operator's devices), the user can
    /// already open every session in the UI, and the agent already has full
    /// filesystem access to every agent's raw session files via its own tools — so
    /// reading session metadata by id is strictly less capability than the agent
    /// already holds, not an escalation. The token gate above still prevents an
    /// unrelated process from reaching the broker at all.
    async fn process_session_info(&self, req: BrokerSessionRequest) -> SessionInfo {
        if self.tokens.lookup(&req.token).await.is_none() {
            return SessionInfo::not_found(req.session_id);
        }
        self.session_info
            .resolve(req.session_id, req.max_messages.unwrap_or(0))
            .await
    }

    async fn process_list_sessions(&self, req: BrokerListSessionsRequest) -> SessionListOutcome {
        let Some(entry) = self.tokens.lookup(&req.token).await else {
            return SessionListOutcome::unavailable(
                None,
                "This Codeg Session identity has expired. Resume the Session before listing peers.",
            );
        };
        let Some(caller_session_id) = self
            .parent_lookup
            .current_conversation_id(&entry.parent_connection_id)
            .await
        else {
            return SessionListOutcome::unavailable(
                None,
                "The calling connection is not bound to a persistent Codeg Session.",
            );
        };
        self.collaboration
            .list_sessions(
                caller_session_id,
                req.query,
                req.limit
                    .unwrap_or(crate::acp::session_collaboration::DEFAULT_SESSION_LIST_LIMIT),
            )
            .await
    }

    async fn process_send_message(&self, req: BrokerSendMessageRequest) -> SessionSendOutcome {
        let Some(entry) = self.tokens.lookup(&req.token).await else {
            return SessionSendOutcome::rejected(
                None,
                "This Codeg Session identity has expired. Resume the Session before sending.",
            );
        };
        let Some(source_session_id) = self
            .parent_lookup
            .current_conversation_id(&entry.parent_connection_id)
            .await
        else {
            return SessionSendOutcome::rejected(
                None,
                "The calling connection is not bound to a persistent Codeg Session.",
            );
        };
        self.collaboration
            .send_message(source_session_id, req.spec)
            .await
    }

    /// Validate the token and hand the progress report to the task engine,
    /// which resolves the parent connection to its owning task + generation.
    async fn process_task_progress(&self, req: BrokerTaskProgressRequest) -> TaskReportAck {
        let Some(entry) = self.tokens.lookup(&req.token).await else {
            return TaskReportAck::rejected("invalid token");
        };
        self.tasks
            .report_progress(&entry.parent_connection_id, &req.message)
            .await
    }

    /// Validate the token and hand the final verdict to the task engine.
    async fn process_task_complete(&self, req: BrokerTaskCompleteRequest) -> TaskReportAck {
        let Some(entry) = self.tokens.lookup(&req.token).await else {
            return TaskReportAck::rejected("invalid token");
        };
        self.tasks
            .complete(
                &entry.parent_connection_id,
                &req.verdict,
                req.summary.as_deref(),
            )
            .await
    }

    /// Resolve the caller's [`AuthoringContext`] from its per-launch token: the
    /// conversation it is currently in (for defaulting the target project) plus
    /// the working directory recorded at injection. `None` when the token is
    /// invalid — the caller gets a soft refusal, not a leak of whether the token
    /// merely expired.
    async fn authoring_context(&self, token: &str) -> Option<AuthoringContext> {
        let entry = self.tokens.lookup(token).await?;
        let conversation_id = self
            .parent_lookup
            .current_conversation_id(&entry.parent_connection_id)
            .await;
        Some(AuthoringContext {
            conversation_id,
            working_dir: entry.working_dir,
        })
    }

    /// Validate the token and hand the automation spec to the authoring impl,
    /// which re-checks the feature flag before writing.
    async fn process_create_automation(
        &self,
        req: BrokerCreateAutomationRequest,
    ) -> AuthoringOutcome {
        let Some(ctx) = self.authoring_context(&req.token).await else {
            return AuthoringOutcome::rejected("automation", "invalid token");
        };
        self.authoring.create_automation(ctx, req.spec).await
    }

    /// Validate the token and hand the task spec to the authoring impl.
    async fn process_create_work_task(&self, req: BrokerCreateWorkTaskRequest) -> AuthoringOutcome {
        let Some(ctx) = self.authoring_context(&req.token).await else {
            return AuthoringOutcome::rejected("work_task", "invalid token");
        };
        self.authoring.create_work_task(ctx, req.spec).await
    }
}

/// Serialize the pending feedback notes into a
/// `{ "count": N, "feedback": [..], "_commit_ids": [..] }` envelope for the
/// `Feedback` arm. Only the lean `text` + `created_at` reach the agent; the
/// `_commit_ids` are internal — the companion echoes them back in a
/// `CommitFeedback` once it delivers the result, and `render_feedback_result`
/// strips them from the agent-facing output. `count == 0` is "no new feedback".
fn feedback_response(items: &[PendingFeedback]) -> std::io::Result<BrokerResponse> {
    let notes: Vec<Value> = items
        .iter()
        .map(|p| serde_json::json!({ "text": p.text, "created_at": p.created_at }))
        .collect();
    let ids: Vec<&str> = items.iter().map(|p| p.id.as_str()).collect();
    Ok(BrokerResponse {
        outcome: serde_json::json!({
            "count": notes.len(),
            "feedback": notes,
            "_commit_ids": ids,
        }),
    })
}

/// Serialize a resolved [`QuestionOutcome`] into a [`BrokerResponse`] for the
/// `Ask` arm — the `{ answers, declined }` envelope the companion renders.
fn ask_response(outcome: &QuestionOutcome) -> std::io::Result<BrokerResponse> {
    Ok(BrokerResponse {
        outcome: serde_json::to_value(outcome).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("encode: {e}"))
        })?,
    })
}

/// Serialize a resolved [`SessionInfo`] into a [`BrokerResponse`] for the
/// `SessionInfo` arm — the companion renders it into the `get_session_info`
/// tool result.
fn session_response(info: SessionInfo) -> std::io::Result<BrokerResponse> {
    Ok(BrokerResponse {
        outcome: serde_json::to_value(&info).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("encode: {e}"))
        })?,
    })
}

fn session_list_response(outcome: SessionListOutcome) -> std::io::Result<BrokerResponse> {
    Ok(BrokerResponse {
        outcome: serde_json::to_value(&outcome).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("encode: {e}"))
        })?,
    })
}

fn session_send_response(outcome: SessionSendOutcome) -> std::io::Result<BrokerResponse> {
    Ok(BrokerResponse {
        outcome: serde_json::to_value(&outcome).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("encode: {e}"))
        })?,
    })
}

fn host_control_help_response(outcome: HostControlHelpOutcome) -> std::io::Result<BrokerResponse> {
    Ok(BrokerResponse {
        outcome: serde_json::to_value(&outcome).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("encode: {e}"))
        })?,
    })
}

fn host_control_use_response(outcome: HostControlUseOutcome) -> std::io::Result<BrokerResponse> {
    Ok(BrokerResponse {
        outcome: serde_json::to_value(&outcome).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("encode: {e}"))
        })?,
    })
}

/// Serialize a [`TaskReportAck`] into a [`BrokerResponse`] for the
/// `TaskProgress` / `TaskComplete` arms — the companion renders it into the
/// tool result.
fn task_ack_response(ack: TaskReportAck) -> std::io::Result<BrokerResponse> {
    Ok(BrokerResponse {
        outcome: serde_json::to_value(&ack).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("encode: {e}"))
        })?,
    })
}

/// Serialize an [`AuthoringOutcome`] into a [`BrokerResponse`] for the
/// `CreateAutomation` / `CreateWorkTask` arms — the companion renders it into
/// the tool result.
fn authoring_response(outcome: AuthoringOutcome) -> std::io::Result<BrokerResponse> {
    Ok(BrokerResponse {
        outcome: serde_json::to_value(&outcome).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("encode: {e}"))
        })?,
    })
}

/// The `declined` outcome — used when the token is invalid, the connection is
/// gone, or the answer one-shot was dropped without a response. The LLM reads it
/// as "the user didn't answer; proceed with your own judgment".
fn ask_declined_response() -> std::io::Result<BrokerResponse> {
    ask_response(&QuestionOutcome {
        answers: Vec::new(),
        declined: true,
    })
}

/// Default socket path for the running process, scoped to PID so multiple
/// codeg instances on the same machine don't collide.
///
/// Unix: a `.sock` file inside `temp_dir`.
/// Windows: a named pipe address `\\.\pipe\codeg-mcp-<pid>`. Windows
/// named pipes live in their own kernel namespace and ignore `temp_dir`; the
/// argument is kept for signature parity across platforms.
#[cfg(unix)]
pub fn default_socket_path(temp_dir: &Path) -> PathBuf {
    temp_dir.join(format!("codeg-mcp-{}.sock", std::process::id()))
}

#[cfg(windows)]
pub fn default_socket_path(_temp_dir: &Path) -> PathBuf {
    PathBuf::from(format!(r"\\.\pipe\codeg-mcp-{}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct StubHostControl {
        callers: tokio::sync::Mutex<Vec<HostControlCaller>>,
    }

    #[async_trait]
    impl HostControlAccess for StubHostControl {
        async fn help(
            &self,
            caller: HostControlCaller,
            _query: Option<String>,
            _action: Option<String>,
        ) -> HostControlHelpOutcome {
            self.callers.lock().await.push(caller);
            HostControlHelpOutcome {
                available: true,
                ..Default::default()
            }
        }

        async fn use_action(
            &self,
            caller: HostControlCaller,
            request_id: String,
            action: String,
            _input: Value,
        ) -> HostControlUseOutcome {
            self.callers.lock().await.push(caller);
            HostControlUseOutcome {
                accepted: true,
                request_id,
                action,
                stage: "read".to_string(),
                replayed: false,
                data: Value::Null,
                note: None,
            }
        }
    }

    struct StaticParentLookup(Option<i32>);

    #[async_trait]
    impl ParentSessionLookup for StaticParentLookup {
        async fn current_conversation_id(&self, _parent_connection_id: &str) -> Option<i32> {
            self.0
        }
    }

    struct NoopFeedback;

    #[async_trait]
    impl SessionFeedbackAccess for NoopFeedback {
        async fn read_pending_feedback(
            &self,
            _parent_connection_id: &str,
        ) -> Vec<PendingFeedback> {
            Vec::new()
        }

        async fn commit_feedback_delivered(
            &self,
            _parent_connection_id: &str,
            _ids: Vec<String>,
        ) {
        }
    }

    struct NoopQuestion;

    #[async_trait]
    impl SessionQuestionAccess for NoopQuestion {
        async fn register_question(
            &self,
            _parent_connection_id: &str,
            _questions: Vec<crate::acp::question::QuestionSpec>,
        ) -> Option<crate::acp::question::RegisteredQuestion> {
            None
        }

        async fn cancel_question(&self, _parent_connection_id: &str, _question_id: &str) {}

        async fn cancel_questions_by_parent(&self, _parent_connection_id: &str) {}
    }

    struct NoopSessionInfo;

    #[async_trait]
    impl SessionInfoAccess for NoopSessionInfo {
        async fn resolve(&self, session_id: i32, _max_messages: u32) -> SessionInfo {
            SessionInfo::not_found(session_id)
        }
    }

    #[derive(Default)]
    struct StubCollaboration {
        listed_by: tokio::sync::Mutex<Vec<i32>>,
        sent_by:
            tokio::sync::Mutex<Vec<(i32, crate::acp::session_collaboration::SessionMessageSpec)>>,
    }

    #[async_trait]
    impl SessionCollaborationAccess for StubCollaboration {
        async fn list_sessions(
            &self,
            caller_session_id: i32,
            _query: Option<String>,
            _limit: u32,
        ) -> SessionListOutcome {
            self.listed_by.lock().await.push(caller_session_id);
            SessionListOutcome {
                available: true,
                caller_session_id: Some(caller_session_id),
                ..Default::default()
            }
        }

        async fn send_message(
            &self,
            source_session_id: i32,
            spec: crate::acp::session_collaboration::SessionMessageSpec,
        ) -> SessionSendOutcome {
            self.sent_by.lock().await.push((source_session_id, spec));
            SessionSendOutcome {
                accepted: true,
                source_session_id: Some(source_session_id),
                ..Default::default()
            }
        }
    }

    struct NoopTaskTools;

    #[async_trait]
    impl WorkTaskToolAccess for NoopTaskTools {
        async fn report_progress(&self, _parent_connection_id: &str, _message: &str) -> TaskReportAck {
            TaskReportAck::rejected("not used")
        }

        async fn complete(
            &self,
            _parent_connection_id: &str,
            _verdict: &str,
            _summary: Option<&str>,
        ) -> TaskReportAck {
            TaskReportAck::rejected("not used")
        }
    }

    struct NoopAuthoring;

    #[async_trait]
    impl ChatAuthoringAccess for NoopAuthoring {
        async fn create_automation(
            &self,
            _ctx: AuthoringContext,
            _spec: crate::acp::chat_authoring::NewAutomationSpec,
        ) -> AuthoringOutcome {
            AuthoringOutcome::default()
        }

        async fn create_work_task(
            &self,
            _ctx: AuthoringContext,
            _spec: crate::acp::chat_authoring::NewWorkTaskSpec,
        ) -> AuthoringOutcome {
            AuthoringOutcome::default()
        }
    }

    fn collaboration_listener(
        tokens: Arc<TokenRegistry>,
        collaboration: Arc<StubCollaboration>,
        parent_conversation: Option<i32>,
    ) -> Arc<HostBridgeListener> {
        HostBridgeListener::new(
            tokens,
            Arc::new(StaticParentLookup(parent_conversation)),
            Arc::new(StubHostControl::default()),
            Arc::new(NoopFeedback),
            Arc::new(NoopQuestion),
            Arc::new(NoopSessionInfo),
            collaboration,
            Arc::new(NoopTaskTools),
            Arc::new(NoopAuthoring),
        )
    }

    fn entry(parent: &str) -> TokenEntry {
        TokenEntry {
            parent_connection_id: parent.to_string(),
            working_dir: PathBuf::from("/workspace"),
            host_control_writes_allowed: true,
        }
    }

    #[tokio::test]
    async fn token_registry_register_lookup_and_revoke() {
        let registry = TokenRegistry::default();
        registry.register("token".into(), entry("parent")).await;
        let found = registry.lookup("token").await.expect("registered token");
        assert_eq!(found.parent_connection_id, "parent");
        registry.revoke("token").await;
        assert!(registry.lookup("token").await.is_none());
    }

    #[tokio::test]
    async fn token_registry_revokes_only_matching_parent() {
        let registry = TokenRegistry::default();
        registry.register("one".into(), entry("parent-a")).await;
        registry.register("two".into(), entry("parent-b")).await;
        registry.register("three".into(), entry("parent-a")).await;

        registry.revoke_by_parent("parent-a").await;

        assert!(registry.lookup("one").await.is_none());
        assert!(registry.lookup("three").await.is_none());
        assert!(registry.lookup("two").await.is_some());
    }

    #[tokio::test]
    async fn collaboration_source_identity_comes_from_token_parent() {
        let collaboration = Arc::new(StubCollaboration::default());
        let tokens = Arc::new(TokenRegistry::default());
        tokens
            .register(
                "tok".into(),
                TokenEntry {
                    parent_connection_id: "parent-conn".into(),
                    working_dir: PathBuf::from("/tmp"),
                    host_control_writes_allowed: true,
                },
            )
            .await;
        let listener = collaboration_listener(tokens, collaboration.clone(), Some(42));

        let outcome = listener
            .process_send_message(BrokerSendMessageRequest {
                token: "tok".into(),
                spec: crate::acp::session_collaboration::SessionMessageSpec {
                    target_session_ids: vec![7],
                    content: "review".into(),
                    delivery_mode:
                        crate::acp::session_collaboration::SessionMessageDeliveryMode::Queue,
                    steer_if_supported: false,
                    expects_reply: true,
                    reply_to_event_id: None,
                    client_dedupe_id: "mcp:test".into(),
                },
            })
            .await;

        assert!(outcome.accepted);
        assert_eq!(outcome.source_session_id, Some(42));
        let sent = collaboration.sent_by.lock().await;
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, 42);
        assert_eq!(sent[0].1.target_session_ids, vec![7]);
    }

    #[tokio::test]
    async fn collaboration_expired_token_never_reaches_host_core() {
        let collaboration = Arc::new(StubCollaboration::default());
        let listener = collaboration_listener(
            Arc::new(TokenRegistry::default()),
            collaboration.clone(),
            Some(42),
        );

        let outcome = listener
            .process_list_sessions(BrokerListSessionsRequest {
                token: "expired".into(),
                query: None,
                limit: None,
            })
            .await;

        assert!(!outcome.available);
        assert!(outcome.note.unwrap().contains("expired"));
        assert!(collaboration.listed_by.lock().await.is_empty());
    }

    #[tokio::test]
    async fn host_control_identity_and_write_policy_come_from_token_parent() {
        let host = Arc::new(StubHostControl::default());
        let tokens = Arc::new(TokenRegistry::default());
        tokens
            .register(
                "tok".into(),
                TokenEntry {
                    parent_connection_id: "parent-conn".into(),
                    working_dir: PathBuf::from("/trusted/workspace"),
                    host_control_writes_allowed: false,
                },
            )
            .await;
        let listener = HostBridgeListener::new(
            tokens,
            Arc::new(StaticParentLookup(Some(42))),
            host.clone(),
            Arc::new(NoopFeedback),
            Arc::new(NoopQuestion),
            Arc::new(NoopSessionInfo),
            Arc::new(StubCollaboration::default()),
            Arc::new(NoopTaskTools),
            Arc::new(NoopAuthoring),
        );

        let outcome = listener
            .process_host_control_use(BrokerHostControlUseRequest {
                token: "tok".into(),
                request_id: "mcp:parent-conn:7".into(),
                action: "session.rename".into(),
                input: serde_json::json!({ "title": "Trusted caller" }),
            })
            .await;

        assert!(outcome.accepted);
        let callers = host.callers.lock().await;
        assert_eq!(callers.len(), 1);
        assert_eq!(callers[0].current_session_id, 42);
        assert_eq!(callers[0].working_dir, PathBuf::from("/trusted/workspace"));
        assert!(!callers[0].writes_allowed);
    }

    #[test]
    fn default_socket_path_is_process_scoped() {
        let path = default_socket_path(Path::new("/tmp"));
        let rendered = path.to_string_lossy();
        assert!(rendered.contains("codeg-mcp-"));
        assert!(rendered.contains(&std::process::id().to_string()));
    }
}
