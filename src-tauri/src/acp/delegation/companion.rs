//! Companion-side MCP protocol — the bits that live inside the `codeg-mcp`
//! binary but are factored out into the library so they can be unit-tested
//! without spawning the binary.
//!
//! The companion speaks newline-delimited JSON-RPC 2.0 on stdio:
//! one request → one response per line, with concurrent dispatch so
//! `notifications/cancelled` can race an in-flight `tools/call`. It exposes
//! codeg-owned feedback, question, Session collaboration, task-reporting, and
//! authoring tools whose schemas are embedded at compile time from
//! [`TOOL_SCHEMA_JSON`] and gated by independent `--features` groups. Canceling
//! a call suppresses its response; for `check_user_feedback` it also skips the
//! delivery commit, so a cancelled note stays pending.
//!
//! Notifications (id = None) produce no response, matching MCP's expectation
//! that `notifications/initialized` etc. are fire-and-forget.
//!
//! Cancellation flow per the MCP 2024-11-05 / 2025-11-25 cancellation utility:
//!
//! 1. Companion receives `tools/call` with JSON-RPC `id = X`, registers
//!    `X → cancel_tx` in [`InflightCalls`], and starts the host round-trip.
//! 2. If `notifications/cancelled` for `requestId = X` arrives, the
//!    notification handler pops the entry and fires `cancel_tx`.
//! 3. The `tools/call` task abandons its UDS read,
//!    and returns `None` — the binary suppresses the response per spec.
//! 4. If the round-trip completes before the cancel arrives, the entry is
//!    removed normally and the response goes out on stdout; a late cancel
//!    notification finds nothing and is silently ignored.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::{oneshot, Mutex};

use crate::acp::chat_authoring::{
    NewAutomationSpec, NewWorkTaskSpec, MAX_PROMPT_CHARS, MAX_TITLE_CHARS,
};
use crate::acp::delegation::transport::{
    client_ask_round_trip, client_commit_feedback, client_create_automation_round_trip,
    client_create_work_task_round_trip, client_feedback_round_trip,
    client_list_sessions_round_trip, client_send_message_round_trip, client_session_round_trip,
    client_task_complete_round_trip, client_task_progress_round_trip, BrokerAskRequest,
    BrokerCommitFeedbackRequest, BrokerCreateAutomationRequest, BrokerCreateWorkTaskRequest,
    BrokerFeedbackRequest, BrokerListSessionsRequest, BrokerResponse, BrokerSendMessageRequest,
    BrokerSessionRequest, BrokerTaskCompleteRequest, BrokerTaskProgressRequest,
};
use crate::acp::question::parse_questions;
use crate::acp::session_collaboration::{
    SessionMessageDeliveryMode, SessionMessageSpec, DEFAULT_SESSION_LIST_LIMIT,
    MAX_SESSION_LIST_LIMIT, MAX_SESSION_MESSAGE_TARGETS,
};
use crate::acp::session_info::MAX_SESSION_MESSAGES;
use crate::models::AutomationAction;

/// Upper bound on the best-effort feedback delivery commit.
const FEEDBACK_COMMIT_BUDGET: Duration = Duration::from_millis(500);

/// Static MCP tool schema. Lives next to this module so codeg-mcp ships
/// a single embedded copy — no runtime file IO, no version skew with the
/// host listener.
pub const TOOL_SCHEMA_JSON: &str = include_str!("tool_schema.json");

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    /// MCP notifications carry no `id`. We dispatch a response only when this
    /// is `Some`.
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

pub fn ok(id: Value, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id,
        result: Some(result),
        error: None,
    }
}

pub fn err(id: Value, code: i64, message: impl Into<String>) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id,
        result: None,
        error: Some(JsonRpcError {
            code,
            message: message.into(),
            data: None,
        }),
    }
}

/// Which tool groups this companion exposes. Passed in via the `--features`
/// arg at launch; a tool whose group is off is hidden from `tools/list` and
/// rejected on `tools/call`.
#[derive(Debug, Clone, Copy)]
pub struct CompanionFeatures {
    pub feedback: bool,
    pub ask: bool,
    pub sessions: bool,
    pub collaboration: bool,
    /// Work-task reporting tools (`task_progress` / `task_complete`) — injected
    /// only into spawns launched by the task engine.
    pub tasks: bool,
    /// `create_automation` — save a scheduled/manual automation from chat.
    pub automations: bool,
    /// `create_work_task` — queue a card on the work-task board from chat.
    pub taskboard: bool,
}

impl CompanionFeatures {
    /// Parse the comma-joined `--features` value (e.g.
    /// `feedback,ask,sessions,collaboration,tasks,automations,taskboard`).
    /// Unknown tokens are ignored. An absent value enables no tools; the main
    /// process and companion ship together and always pass an explicit value.
    pub fn parse(raw: Option<&str>) -> Self {
        let Some(s) = raw else {
            return Self {
                feedback: false,
                ask: false,
                sessions: false,
                collaboration: false,
                tasks: false,
                automations: false,
                taskboard: false,
            };
        };
        let mut f = Self {
            feedback: false,
            ask: false,
            sessions: false,
            collaboration: false,
            tasks: false,
            automations: false,
            taskboard: false,
        };
        for tok in s.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            match tok {
                "feedback" => f.feedback = true,
                "ask" => f.ask = true,
                "sessions" => f.sessions = true,
                "collaboration" => f.collaboration = true,
                "tasks" => f.tasks = true,
                "automations" => f.automations = true,
                "taskboard" => f.taskboard = true,
                _ => {}
            }
        }
        f
    }

    /// Whether the named MCP tool is exposed under the enabled feature groups.
    pub fn allows_tool(&self, name: &str) -> bool {
        match name {
            "check_user_feedback" => self.feedback,
            "ask_user_question" => self.ask,
            "get_session_info" => self.sessions,
            "list_sessions" | "send_message" => self.collaboration,
            "task_progress" | "task_complete" => self.tasks,
            "create_automation" => self.automations,
            "create_work_task" => self.taskboard,
            _ => false,
        }
    }
}

/// Process arguments threaded through every `tools/call`.
#[derive(Debug, Clone)]
pub struct CompanionContext {
    pub parent_connection_id: String,
    pub socket_path: String,
    pub token: String,
    /// Tool groups this launch exposes (see [`CompanionFeatures`]).
    pub features: CompanionFeatures,
}

/// Per-in-flight-call state. The companion stashes one of these per
/// `tools/call` so a subsequent `notifications/cancelled` for the same
/// JSON-RPC `id` can wake the round-trip task.
pub struct InflightEntry {
    /// Tripped by the cancel handler to wake the round-trip task.
    cancel_tx: oneshot::Sender<()>,
}

/// `request_id_key(id) → InflightEntry`. Keyed by a string form of the
/// JSON-RPC `id` so we can compare against the `requestId` payload of
/// `notifications/cancelled` which is itself a JSON value (numbers serialize
/// as their canonical string form here).
#[derive(Default)]
pub struct InflightCalls {
    inner: Mutex<HashMap<String, InflightEntry>>,
}

impl InflightCalls {
    pub fn new() -> Self {
        Self::default()
    }

    async fn register(&self, id_key: String, entry: InflightEntry) {
        self.inner.lock().await.insert(id_key, entry);
    }

    async fn take(&self, id_key: &str) -> Option<InflightEntry> {
        self.inner.lock().await.remove(id_key)
    }

    /// Drain every in-flight entry, clearing the registry. Called at
    /// companion shutdown so every pending call is woken before the runtime
    /// exits.
    pub async fn drain_all(&self) -> Vec<InflightEntry> {
        let mut map = self.inner.lock().await;
        map.drain().map(|(_k, v)| v).collect()
    }
}

/// Canonicalize a JSON-RPC `id` to a string suitable as a `HashMap` key.
/// JSON-RPC permits string OR number ids; we collapse both via
/// `serde_json::to_string` so a numeric `42` and string `"42"` stay
/// distinct (which the spec also requires).
pub fn request_id_key(id: &Value) -> String {
    serde_json::to_string(id).unwrap_or_else(|_| String::from("null"))
}

/// Dispatch verdict for a single inbound stdin line.
pub enum LineAction {
    /// Synchronous response — write `resp` to stdout immediately.
    Respond(JsonRpcResponse),
    /// Asynchronous tools/call — the binary should spawn the round-trip
    /// task and only write a response if the future returns `Some`.
    Spawn(SpawnedCall),
    /// Notification or no-op (parse errors with `id = null`). Nothing to
    /// emit on stdout.
    Silent,
}

/// Resolution of a spawned `tools/call`: the response to relay to the agent
/// (`None` = cancellation won, so suppress per the MCP spec) plus an optional
/// action the binary runs ONLY after that response is successfully written to
/// the agent's stdout.
///
/// `after_relay` exists for `check_user_feedback`: marking the pulled notes
/// `Delivered` (the broker `CommitFeedback`) must happen strictly AFTER the
/// agent actually receives them. Committing any earlier — at listener read
/// time, or right after the round-trip but before the stdout relay — would mark
/// a note delivered that a failed/never-reached write (or a companion dying mid
/// teardown) never put in front of the agent, breaking at-least-once delivery.
/// Every other tool leaves this `None`.
pub struct SpawnResult {
    pub response: Option<JsonRpcResponse>,
    pub after_relay: Option<futures_util::future::BoxFuture<'static, ()>>,
}

/// Materialized async tools/call ready to drive in a tokio task. The binary
/// awaits `future` to obtain the [`SpawnResult`]: it writes `response` (when
/// `Some`) and, on a successful write, runs `after_relay` (when `Some`).
pub struct SpawnedCall {
    /// JSON-RPC `id` of the original `tools/call` so the binary can stamp
    /// the response.
    pub request_id: Value,
    /// String form of `request_id` for inflight bookkeeping.
    pub request_id_key: String,
    /// The future that performs the UDS round-trip racing the cancel channel
    /// and resolves to the [`SpawnResult`] to relay (and optionally commit).
    pub future: futures_util::future::BoxFuture<'static, SpawnResult>,
}

/// Parse a stdin line and produce a [`LineAction`]. The binary handles the
/// IO side; this function is pure aside from registering the inflight
/// entry on `tools/call` so unit tests can drive it without stdio.
pub async fn dispatch_line(
    ctx: &CompanionContext,
    inflight: Arc<InflightCalls>,
    line: &str,
) -> LineAction {
    let req: JsonRpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            return LineAction::Respond(err(Value::Null, -32700, format!("parse error: {e}")));
        }
    };

    // Notifications carry no id — no response goes out. Cancellation is
    // the only notification we act on.
    if req.id.is_none() {
        if req.method == "notifications/cancelled" {
            handle_cancel_notification(&inflight, &req.params).await;
        }
        return LineAction::Silent;
    }

    let id = req.id.expect("checked is_none");
    match req.method.as_str() {
        "initialize" => LineAction::Respond(ok(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "codeg-mcp",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "capabilities": { "tools": {} },
            }),
        )),
        "tools/list" => {
            // The embedded schema is a JSON array of every tool the companion
            // can carry; filter to the groups enabled for this launch so a
            // disabled feature's tools never surface to the LLM.
            let all: Value = match serde_json::from_str(TOOL_SCHEMA_JSON) {
                Ok(v) => v,
                Err(e) => {
                    return LineAction::Respond(err(
                        id,
                        -32603,
                        format!("embedded schema invalid: {e}"),
                    ));
                }
            };
            let tools = match all.as_array() {
                Some(arr) => Value::Array(
                    arr.iter()
                        .filter(|t| {
                            t.get("name")
                                .and_then(|v| v.as_str())
                                .map(|n| ctx.features.allows_tool(n))
                                .unwrap_or(false)
                        })
                        .cloned()
                        .collect(),
                ),
                None => all,
            };
            LineAction::Respond(ok(id, json!({ "tools": tools })))
        }
        "tools/call" => build_tools_call_spawn(ctx.clone(), inflight, id, req.params).await,
        _ => LineAction::Respond(err(id, -32601, format!("method not found: {}", req.method))),
    }
}

/// Build the spawned-call descriptor for a `tools/call` (or, when the
/// arguments are obviously bogus, a synchronous error response). Registers
/// the inflight entry and returns a future the binary should drive.
async fn build_tools_call_spawn(
    ctx: CompanionContext,
    inflight: Arc<InflightCalls>,
    id: Value,
    params: Value,
) -> LineAction {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
    let socket = ctx.socket_path.clone();
    // Defense in depth: tools/list already hides tools whose feature group is
    // off, but a misbehaving client could still call one by name. A disabled
    // tool is rejected uniformly as "unknown tool" — indistinguishable from a
    // genuinely nonexistent one (no leak that the feature exists but is off),
    // and matching the legacy unknown-tool rejection shape.
    if !ctx.features.allows_tool(&name) {
        return LineAction::Respond(err(id, -32602, format!("unknown tool: {name}")));
    }
    match name.as_str() {
        "check_user_feedback" => {
            let req = BrokerFeedbackRequest {
                token: ctx.token.clone(),
            };
            // Feedback uses a dedicated spawn so it can COMMIT delivery only when
            // the round-trip wins the cancel race (i.e. the result actually goes
            // to the agent). A cancel that suppresses the response sends no
            // commit, leaving the notes pending for the next check.
            register_and_spawn_feedback(inflight, id, socket, ctx.token.clone(), req).await
        }
        "ask_user_question" => {
            // Validate + parse the schema HERE so a malformed call gets a
            // synchronous -32602 the LLM can fix, rather than round-tripping bad
            // data. Stable per-question ids are minted now and flow through to
            // the answer correlation.
            let questions = match parse_questions(&arguments) {
                Ok(qs) => qs,
                Err(msg) => return LineAction::Respond(err(id, -32602, msg)),
            };
            let req = BrokerAskRequest {
                token: ctx.token.clone(),
                questions,
            };
            // No external_handle: canceling a blocking ask only suppresses its
            // response. The companion dropping the round-trip future closes the
            // socket, which the listener observes (peer-close) to tear the
            // pending question down — no broker-side cancel to dispatch.
            let round_trip = Box::pin(async move { client_ask_round_trip(&socket, &req).await });
            register_and_spawn(inflight, id, round_trip, render_ask_result).await
        }
        "get_session_info" => {
            // `session_id` is the codeg conversation id the agent read out of a
            // `codeg://session/<id>` reference. Accept a JSON number or a numeric
            // string (some hosts stringify integer args); reject anything else
            // synchronously so the LLM can fix it.
            let session_id = match parse_session_id(&arguments) {
                Some(id) => id,
                None => {
                    return LineAction::Respond(err(
                        id,
                        -32602,
                        "get_session_info requires an integer `session_id` \
                         (the number in the codeg://session/<id> reference)",
                    ));
                }
            };
            // Default to a modest recent-message window; `0` means metadata-only.
            // Robust against stringified / oversized values (see helper).
            let max_messages = parse_max_messages(&arguments);
            let req = BrokerSessionRequest {
                token: ctx.token.clone(),
                session_id,
                max_messages: Some(max_messages),
            };
            // No external_handle: a read-only lookup has nothing to cancel
            // broker-side — canceling only suppresses the response.
            let round_trip =
                Box::pin(async move { client_session_round_trip(&socket, &req).await });
            register_and_spawn(inflight, id, round_trip, render_session_result).await
        }
        "list_sessions" => {
            let query = arguments
                .get("query")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let limit = parse_session_list_limit(&arguments);
            let req = BrokerListSessionsRequest {
                token: ctx.token.clone(),
                query,
                limit: Some(limit),
            };
            let round_trip =
                Box::pin(async move { client_list_sessions_round_trip(&socket, &req).await });
            register_and_spawn(inflight, id, round_trip, render_session_list_result).await
        }
        "send_message" => {
            let target_session_ids = match parse_target_session_ids(&arguments) {
                Ok(ids) => ids,
                Err(message) => return LineAction::Respond(err(id, -32602, message)),
            };
            let content = arguments
                .get("content")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let Some(content) = content else {
                return LineAction::Respond(err(
                    id,
                    -32602,
                    "send_message requires a non-empty `content` string",
                ));
            };
            let delivery_mode = match arguments
                .get("delivery_mode")
                .and_then(|value| value.as_str())
                .unwrap_or("queue")
            {
                "queue" => SessionMessageDeliveryMode::Queue,
                "deliver_only" => SessionMessageDeliveryMode::DeliverOnly,
                _ => {
                    return LineAction::Respond(err(
                        id,
                        -32602,
                        "send_message `delivery_mode` must be queue or deliver_only",
                    ))
                }
            };
            let reply_to_event_id = arguments
                .get("reply_to_event_id")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let steer_if_supported = match arguments
                .get("delivery_hint")
                .and_then(|value| value.as_str())
                .unwrap_or("default")
            {
                "default" => false,
                "steer_if_supported" => true,
                _ => {
                    return LineAction::Respond(err(
                        id,
                        -32602,
                        "send_message `delivery_hint` must be default or steer_if_supported",
                    ))
                }
            };
            if steer_if_supported && delivery_mode == SessionMessageDeliveryMode::DeliverOnly {
                return LineAction::Respond(err(
                    id,
                    -32602,
                    "send_message `steer_if_supported` requires delivery_mode=queue",
                ));
            }
            let client_dedupe_id = mcp_call_dedupe_id(&ctx.parent_connection_id, &id);
            let req = BrokerSendMessageRequest {
                token: ctx.token.clone(),
                spec: SessionMessageSpec {
                    target_session_ids,
                    content,
                    delivery_mode,
                    steer_if_supported,
                    expects_reply: arguments
                        .get("expects_reply")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(true),
                    reply_to_event_id,
                    client_dedupe_id,
                },
            };
            let round_trip =
                Box::pin(async move { client_send_message_round_trip(&socket, &req).await });
            register_and_spawn(inflight, id, round_trip, render_session_send_result).await
        }
        "task_progress" => {
            let message = arguments
                .get("message")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let Some(message) = message else {
                return LineAction::Respond(err(
                    id,
                    -32602,
                    "task_progress requires a non-empty `message` string",
                ));
            };
            let req = BrokerTaskProgressRequest {
                token: ctx.token.clone(),
                message,
            };
            // No external_handle: a fire-and-forget report has nothing to
            // cancel broker-side.
            let round_trip =
                Box::pin(async move { client_task_progress_round_trip(&socket, &req).await });
            register_and_spawn(inflight, id, round_trip, render_task_ack).await
        }
        "task_complete" => {
            let verdict = arguments
                .get("verdict")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .unwrap_or("");
            if !matches!(verdict, "success" | "needs_review" | "blocked") {
                return LineAction::Respond(err(
                    id,
                    -32602,
                    "task_complete requires `verdict` of success | needs_review | blocked",
                ));
            }
            let summary = arguments
                .get("summary")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let req = BrokerTaskCompleteRequest {
                token: ctx.token.clone(),
                verdict: verdict.to_string(),
                summary,
            };
            let round_trip =
                Box::pin(async move { client_task_complete_round_trip(&socket, &req).await });
            register_and_spawn(inflight, id, round_trip, render_task_ack).await
        }
        "create_automation" => {
            // Validate the shape HERE so a malformed call gets a synchronous
            // -32602 the LLM can fix, rather than round-tripping bad data into
            // the DB layer's error path.
            let spec = match parse_automation_spec(&arguments) {
                Ok(s) => s,
                Err(msg) => return LineAction::Respond(err(id, -32602, msg)),
            };
            let req = BrokerCreateAutomationRequest {
                token: ctx.token.clone(),
                spec,
            };
            // No external_handle: a create either lands or it doesn't. Canceling
            // only suppresses the response — there is no in-flight child to tear
            // down broker-side.
            let round_trip =
                Box::pin(async move { client_create_automation_round_trip(&socket, &req).await });
            register_and_spawn(inflight, id, round_trip, render_authoring_result).await
        }
        "create_work_task" => {
            let spec = match parse_work_task_spec(&arguments) {
                Ok(s) => s,
                Err(msg) => return LineAction::Respond(err(id, -32602, msg)),
            };
            let req = BrokerCreateWorkTaskRequest {
                token: ctx.token.clone(),
                spec,
            };
            let round_trip =
                Box::pin(async move { client_create_work_task_round_trip(&socket, &req).await });
            register_and_spawn(inflight, id, round_trip, render_authoring_result).await
        }
        other => LineAction::Respond(err(id, -32602, format!("unknown tool: {other}"))),
    }
}

/// Register the inflight entry and build the [`SpawnedCall`] that races the
/// host round-trip against the cancel signal. `render` maps the listener's
/// `BrokerResponse.outcome` into the MCP `tools/call` result body.
async fn register_and_spawn(
    inflight: Arc<InflightCalls>,
    id: Value,
    round_trip: futures_util::future::BoxFuture<'static, std::io::Result<BrokerResponse>>,
    render: fn(&Value) -> Value,
) -> LineAction {
    let (cancel_tx, cancel_rx) = oneshot::channel();
    let id_key = request_id_key(&id);
    inflight
        .register(id_key.clone(), InflightEntry { cancel_tx })
        .await;

    let id_for_response = id.clone();
    let id_key_for_task = id_key.clone();
    let inflight_for_task = inflight.clone();
    let future = Box::pin(async move {
        // Race the UDS round-trip against the cancel signal. Cancel wins →
        // suppress the response per MCP spec.
        let response = tokio::select! {
            biased;
            _ = cancel_rx => {
                let _ = inflight_for_task.take(&id_key_for_task).await;
                None
            }
            rt = round_trip => {
                let _ = inflight_for_task.take(&id_key_for_task).await;
                match rt {
                    Ok(resp) => Some(ok(id_for_response, render(&resp.outcome))),
                    Err(e) => Some(err(
                        id_for_response,
                        -32603,
                        format!("broker round-trip failed: {e}"),
                    )),
                }
            }
        };
        SpawnResult {
            response,
            after_relay: None,
        }
    });

    LineAction::Spawn(SpawnedCall {
        request_id: id,
        request_id_key: id_key,
        future,
    })
}

/// `check_user_feedback`-specific spawn. Like [`register_and_spawn`], but it
/// carries an `after_relay` commit — a `CommitFeedback` round-trip marking the
/// pulled notes `Delivered` — that the binary runs ONLY after it successfully
/// writes this response to the agent's stdout (the listener does not commit at
/// read time). Two guards compose to make delivery at-least-once. First, if the
/// cancel branch wins the biased select the result is `response: None` with no
/// `after_relay`, so the check is suppressed and never committed (the notes stay
/// pending for the next check). Second, when the round-trip wins, `after_relay`
/// is built but only fires once the stdout relay succeeds; a failed or
/// never-reached write (a dying companion, a broken agent stdin) skips the
/// commit entirely. So a note flips to `Delivered` only after it was actually
/// put in front of the agent. The sole irreducible boundary is the agent
/// crashing after the bytes are flushed to its stdin but before it reads them —
/// at which point the note is moot (the agent will not act on it), the correct
/// semantics for a delivered best-effort steering side-channel.
async fn register_and_spawn_feedback(
    inflight: Arc<InflightCalls>,
    id: Value,
    socket: String,
    token: String,
    req: BrokerFeedbackRequest,
) -> LineAction {
    let (cancel_tx, cancel_rx) = oneshot::channel();
    let id_key = request_id_key(&id);
    inflight
        .register(id_key.clone(), InflightEntry { cancel_tx })
        .await;

    let id_for_response = id.clone();
    let id_key_for_task = id_key.clone();
    let inflight_for_task = inflight.clone();
    let future = Box::pin(async move {
        tokio::select! {
            biased;
            _ = cancel_rx => {
                // Cancelled before delivery → suppress AND do not commit.
                let _ = inflight_for_task.take(&id_key_for_task).await;
                SpawnResult {
                    response: None,
                    after_relay: None,
                }
            }
            rt = client_feedback_round_trip(&socket, &req) => {
                let _ = inflight_for_task.take(&id_key_for_task).await;
                match rt {
                    Ok(resp) => {
                        // Relay-then-commit: render the agent-facing result now,
                        // but defer the `CommitFeedback` to `after_relay` so it
                        // fires ONLY after the binary writes this response to the
                        // agent's stdout. A dead/failed relay skips the commit,
                        // leaving the notes pending for the next check
                        // (at-least-once at the agent-facing boundary).
                        let outcome = resp.outcome;
                        let response = ok(id_for_response, render_feedback_result(&outcome));
                        let commit: futures_util::future::BoxFuture<'static, ()> =
                            Box::pin(async move {
                                commit_feedback_after_delivery(&socket, &token, &outcome).await;
                            });
                        SpawnResult {
                            response: Some(response),
                            after_relay: Some(commit),
                        }
                    }
                    Err(e) => SpawnResult {
                        response: Some(err(
                            id_for_response,
                            -32603,
                            format!("broker round-trip failed: {e}"),
                        )),
                        after_relay: None,
                    },
                }
            }
        }
    });

    LineAction::Spawn(SpawnedCall {
        request_id: id,
        request_id_key: id_key,
        future,
    })
}

/// Send a `CommitFeedback` for the note ids the listener embedded in the
/// response (`_commit_ids`). Fire-and-forget, bounded by [`FEEDBACK_COMMIT_BUDGET`]:
/// a failed commit just leaves the notes pending for the next check.
async fn commit_feedback_after_delivery(socket: &str, token: &str, outcome: &Value) {
    let ids: Vec<String> = outcome
        .get("_commit_ids")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        return;
    }
    let req = BrokerCommitFeedbackRequest {
        token: token.to_string(),
        ids,
    };
    let _ =
        tokio::time::timeout(FEEDBACK_COMMIT_BUDGET, client_commit_feedback(socket, &req)).await;
}

/// Handle a `notifications/cancelled` notification. Looks up the in-flight
/// call by `requestId` and fires its cancel channel. Unknown ids are
/// silently ignored per MCP spec.
async fn handle_cancel_notification(inflight: &Arc<InflightCalls>, params: &Value) {
    let request_id = match params.get("requestId") {
        Some(v) => v.clone(),
        None => return,
    };
    let id_key = request_id_key(&request_id);
    let Some(entry) = inflight.take(&id_key).await else {
        return;
    };
    let _ = entry.cancel_tx.send(());
}

/// Drain every in-flight `tools/call` entry and wake its task. Called at
/// companion shutdown (stdin EOF or parent-watchdog fire).
pub async fn drain_inflight_calls(inflight: &Arc<InflightCalls>) {
    for entry in inflight.drain_all().await {
        let _ = entry.cancel_tx.send(());
    }
}

/// Map the `check_user_feedback` round-trip outcome (a `{ count, feedback:[..] }`
/// envelope from the listener) into an MCP `tools/call` result.
///
/// The human-readable `content` text is the steering the LLM acts on: when
/// notes are present it frames them as high-priority user corrections and asks
/// the agent to adjust and acknowledge; when empty it says so plainly. The raw
/// envelope rides along in `structuredContent`. `isError` is always `false` — a
/// successful check with no feedback is a valid result, not an error.
pub fn render_feedback_result(outcome: &Value) -> Value {
    let count = outcome.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
    let text = if count == 0 {
        "No new feedback from the user. Continue with your current plan.".to_string()
    } else {
        let mut s = format!(
            "The user sent {count} message(s) while you were working. Treat this as \
             high-priority steering: adjust your current approach to honor it now, and \
             briefly acknowledge what you changed.\n"
        );
        if let Some(notes) = outcome.get("feedback").and_then(|v| v.as_array()) {
            for (i, note) in notes.iter().enumerate() {
                let body = note.get("text").and_then(|v| v.as_str()).unwrap_or("");
                s.push_str(&format!("{}. {}\n", i + 1, body));
            }
        }
        s
    };
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false,
        // Rebuild the structured payload from count + feedback only — the
        // listener's internal `_commit_ids` must not leak to the agent's host.
        "structuredContent": {
            "count": count,
            "feedback": outcome.get("feedback").cloned().unwrap_or_else(|| json!([])),
        },
    })
}

/// Map the `ask_user_question` round-trip outcome (a `{ answers, declined }`
/// envelope from the listener) into an MCP `tools/call` result.
///
/// The human-readable `content` text reports the user's selections per question
/// so the agent can act on them; a declined / empty answer tells the agent to
/// proceed with its own judgment. The raw envelope rides along in
/// `structuredContent`. `isError` is always `false` — a declined question is a
/// valid result, not an error.
pub fn render_ask_result(outcome: &Value) -> Value {
    let declined = outcome
        .get("declined")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let answers = outcome
        .get("answers")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let text = if declined || answers.is_empty() {
        "The user dismissed the question(s) without choosing an answer. Proceed \
         using your best judgment and reasonable defaults."
            .to_string()
    } else {
        let mut s = String::from("The user answered your question(s):\n");
        for (i, a) in answers.iter().enumerate() {
            let header = a.get("header").and_then(|v| v.as_str()).unwrap_or("");
            let question = a.get("question").and_then(|v| v.as_str()).unwrap_or("");
            let selected: Vec<&str> = a
                .get("selected")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|x| x.as_str()).collect())
                .unwrap_or_default();
            let joined = if selected.is_empty() {
                "(no selection)".to_string()
            } else {
                selected.join(", ")
            };
            s.push_str(&format!(
                "{}. [{header}] {question}\n   → {joined}\n",
                i + 1
            ));
        }
        s
    };
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false,
        "structuredContent": { "answers": answers, "declined": declined },
    })
}

/// Read a required non-empty string argument, trimmed. `Err` carries the
/// `-32602` message the dispatcher returns verbatim.
fn required_string(arguments: &Value, field: &str, tool: &str) -> Result<String, String> {
    arguments
        .get(field)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("{tool} requires a non-empty `{field}` string"))
}

/// Read an optional string argument, trimmed. Absent, non-string, and
/// whitespace-only all collapse to `None` — an LLM passing `""` to mean "use the
/// default" gets the default rather than a validation error.
fn optional_string(arguments: &Value, field: &str) -> Option<String> {
    arguments
        .get(field)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// The only cron arity this tool accepts: `min hour dom mon dow`.
///
/// The evaluator (`automation_service::normalize_cron`) remaps the POSIX
/// day-of-week field ONLY for 5-field input; a 6/7-field expression passes
/// through to the `cron` crate untouched, where `1` means Sunday. So
/// `0 0 9 * * 1-5` — which an LLM would write meaning "weekdays at 09:00" —
/// would silently fire Sunday through Thursday. Rejecting the arity here keeps
/// the tool's advertised contract (5 fields, POSIX weekdays) the only one that
/// can reach the DB, without touching how already-stored schedules are read.
const CRON_FIELDS: usize = 5;

/// Validate `create_automation` arguments into a [`NewAutomationSpec`]. Length
/// caps are applied here (truncating, never rejecting) so an over-long
/// generation still produces the automation the user asked for. Cron *syntax*
/// is NOT parsed here — the main process owns the one authoritative evaluator,
/// and its error comes back as a soft outcome the LLM can correct; only the
/// field count is checked, because that is the one shape that would be accepted
/// and then mean something other than what was asked (see [`CRON_FIELDS`]).
fn parse_automation_spec(arguments: &Value) -> Result<NewAutomationSpec, String> {
    let name = required_string(arguments, "name", "create_automation")?;
    let prompt = required_string(arguments, "prompt", "create_automation")?;
    let action = match optional_string(arguments, "action").as_deref() {
        None | Some("launch_session") => AutomationAction::LaunchSession,
        Some("enqueue_task") => AutomationAction::EnqueueTask,
        Some(other) => {
            return Err(format!(
                "create_automation `action` must be launch_session or enqueue_task (got {other})"
            ));
        }
    };
    let cron = optional_string(arguments, "cron");
    if let Some(expr) = cron.as_deref() {
        let fields = expr.split_whitespace().count();
        if fields != CRON_FIELDS {
            return Err(format!(
                "create_automation `cron` must have exactly {CRON_FIELDS} fields \
                 (min hour day-of-month month day-of-week), got {fields}. A seconds \
                 field is not supported — write '0 9 * * 1-5', not '0 0 9 * * 1-5'."
            ));
        }
    }
    Ok(NewAutomationSpec {
        name: truncate_chars(&name, MAX_TITLE_CHARS),
        prompt: truncate_chars(&prompt, MAX_PROMPT_CHARS),
        cron,
        timezone: optional_string(arguments, "timezone"),
        action,
        agent_type: optional_string(arguments, "agent_type"),
        folder_path: optional_string(arguments, "folder_path"),
        // Absent means "live now" (the common ask); an explicit non-bool is
        // treated as absent rather than failing the whole call.
        enabled: arguments
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
    })
}

/// Validate `create_work_task` arguments into a [`NewWorkTaskSpec`].
fn parse_work_task_spec(arguments: &Value) -> Result<NewWorkTaskSpec, String> {
    let title = required_string(arguments, "title", "create_work_task")?;
    let prompt = required_string(arguments, "prompt", "create_work_task")?;
    Ok(NewWorkTaskSpec {
        title: truncate_chars(&title, MAX_TITLE_CHARS),
        prompt: truncate_chars(&prompt, MAX_PROMPT_CHARS),
        agent_type: optional_string(arguments, "agent_type"),
        folder_path: optional_string(arguments, "folder_path"),
    })
}

/// Character-safe truncation shared by both spec parsers.
fn truncate_chars(s: &str, cap: usize) -> String {
    crate::acp::chat_authoring::truncate_chars(s, cap)
}

/// Extract the `session_id` integer from the `get_session_info` arguments,
/// tolerating a JSON number (int or whole float) or a numeric string — some MCP
/// hosts stringify integer args. `None` for missing / non-integer / out-of-range,
/// which the dispatcher maps to a synchronous `-32602` the LLM can fix.
fn parse_session_id(arguments: &Value) -> Option<i32> {
    let v = arguments.get("session_id")?;
    if let Some(n) = v.as_i64() {
        return i32::try_from(n).ok();
    }
    if let Some(f) = v.as_f64() {
        if f.fract() == 0.0 && f >= f64::from(i32::MIN) && f <= f64::from(i32::MAX) {
            return Some(f as i32);
        }
    }
    if let Some(s) = v.as_str() {
        return s.trim().parse::<i32>().ok();
    }
    None
}

/// Parse the optional `max_messages` tuning arg robustly: a JSON number (integer
/// or whole non-negative float) or a numeric string — consistent with how
/// `session_id` tolerates stringified ints. Clamps in `u64` space BEFORE narrowing
/// to `u32`, so a huge value (e.g. `4294967296`) saturates to the cap instead of
/// wrapping to a small number. An absent OR unparseable value falls back to the
/// default window — it is an optional knob, not a hard error — while an explicit
/// `0` (or `"0"`) is preserved to mean metadata-only.
fn parse_max_messages(arguments: &Value) -> u32 {
    const DEFAULT_MAX_MESSAGES: u32 = 20;
    let Some(v) = arguments.get("max_messages") else {
        return DEFAULT_MAX_MESSAGES;
    };
    let raw: Option<u64> = if let Some(n) = v.as_u64() {
        Some(n)
    } else if let Some(f) = v.as_f64() {
        // Reject negatives / fractions; `f as u64` saturates a huge float.
        (f.fract() == 0.0 && f >= 0.0).then_some(f as u64)
    } else if let Some(s) = v.as_str() {
        s.trim().parse::<u64>().ok()
    } else {
        None
    };
    match raw {
        Some(n) => n.min(u64::from(MAX_SESSION_MESSAGES)) as u32,
        None => DEFAULT_MAX_MESSAGES,
    }
}

fn parse_session_list_limit(arguments: &Value) -> u32 {
    let Some(value) = arguments.get("limit") else {
        return DEFAULT_SESSION_LIST_LIMIT;
    };
    let parsed = value
        .as_u64()
        .or_else(|| value.as_str().and_then(|raw| raw.trim().parse().ok()));
    parsed
        .unwrap_or(u64::from(DEFAULT_SESSION_LIST_LIMIT))
        .clamp(1, u64::from(MAX_SESSION_LIST_LIMIT)) as u32
}

fn parse_target_session_ids(arguments: &Value) -> Result<Vec<i32>, String> {
    let values = arguments
        .get("target_session_ids")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            "send_message requires a non-empty `target_session_ids` array".to_string()
        })?;
    let mut ids = Vec::new();
    for value in values {
        let id = value
            .as_i64()
            .and_then(|raw| i32::try_from(raw).ok())
            .or_else(|| {
                value
                    .as_str()
                    .and_then(|raw| raw.trim().parse::<i32>().ok())
            })
            .filter(|id| *id > 0)
            .ok_or_else(|| {
                "send_message target_session_ids must contain positive integer Session ids"
                    .to_string()
            })?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return Err("send_message requires at least one target Session".to_string());
    }
    if ids.len() > MAX_SESSION_MESSAGE_TARGETS {
        return Err(format!(
            "send_message supports at most {MAX_SESSION_MESSAGE_TARGETS} target Sessions"
        ));
    }
    Ok(ids)
}

/// Stable for one MCP request (including a transport replay), opaque to the
/// model, and safely below the collaboration event's dedupe-key limit.
fn mcp_call_dedupe_id(parent_connection_id: &str, request_id: &Value) -> String {
    let raw = format!("{parent_connection_id}:{}", request_id_key(request_id));
    let digest = Sha256::digest(raw.as_bytes());
    format!("mcp:{digest:x}")
}

pub fn render_session_list_result(outcome: &Value) -> Value {
    let available = outcome
        .get("available")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let text = if !available {
        outcome
            .get("note")
            .and_then(Value::as_str)
            .unwrap_or("Session collaboration is unavailable.")
            .to_string()
    } else {
        let sessions = outcome
            .get("sessions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if sessions.is_empty() {
            "No matching persistent Sessions were found.".to_string()
        } else {
            let mut lines = vec![format!("Found {} Session(s):", sessions.len())];
            for session in sessions {
                let id = session
                    .get("session_id")
                    .and_then(Value::as_i64)
                    .unwrap_or_default();
                let title = session
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Untitled Session");
                let agent = session
                    .get("agent_type")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let workspace = session
                    .get("workspace_path")
                    .and_then(Value::as_str)
                    .unwrap_or("no workspace");
                lines.push(format!("- {id}: {title} [{agent}] — {workspace}"));
            }
            if outcome
                .get("truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                lines.push("Results were truncated; narrow `query` to find more.".to_string());
            }
            lines.join("\n")
        }
    };
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false,
        "structuredContent": outcome.clone(),
    })
}

pub fn render_session_send_result(outcome: &Value) -> Value {
    let accepted = outcome
        .get("accepted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let text = if accepted {
        let event_id = outcome
            .get("event_id")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let deliveries = outcome
            .get("deliveries")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut lines = vec![format!("Message accepted as event {event_id}.")];
        for delivery in deliveries {
            let id = delivery
                .get("target_session_id")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let title = delivery
                .get("target_title")
                .and_then(Value::as_str)
                .unwrap_or("Untitled Session");
            let state = delivery
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            lines.push(format!("- {id}: {title} — {state}"));
        }
        lines.push(
            "Delivery or queueing does not mean the target Agent has completed the request."
                .to_string(),
        );
        lines.join("\n")
    } else {
        outcome
            .get("note")
            .and_then(Value::as_str)
            .unwrap_or("The message was not accepted.")
            .to_string()
    };
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false,
        "structuredContent": outcome.clone(),
    })
}

/// Map the `get_session_info` round-trip outcome (a serialized
/// [`crate::acp::session_info::SessionInfo`]) into an MCP `tools/call` result. A
/// not-found result is surfaced as readable text with `isError: false` (the LLM
/// reads it and proceeds), never as a tool error. The full structured envelope
/// rides along in `structuredContent` for hosts that keep it.
pub fn render_session_result(outcome: &Value) -> Value {
    let found = outcome
        .get("found")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let text = if found {
        render_session_summary_text(outcome)
    } else {
        outcome
            .get("note")
            .and_then(|v| v.as_str())
            .unwrap_or("No matching session was found.")
            .to_string()
    };
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false,
        "structuredContent": outcome.clone(),
    })
}

/// Map a `task_progress` / `task_complete` round-trip outcome (a
/// `{ recorded, note? }` ack) into an MCP `tools/call` result. A report that
/// could not be attributed (no active work task for this session) is readable
/// text with `isError: false` — the agent just carries on with its work.
pub fn render_task_ack(outcome: &Value) -> Value {
    let recorded = outcome
        .get("recorded")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let text = outcome
        .get("note")
        .and_then(|v| v.as_str())
        .unwrap_or(if recorded {
            "Recorded."
        } else {
            "Not recorded."
        })
        .to_string();
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false,
        "structuredContent": outcome.clone(),
    })
}

/// Map a `create_automation` / `create_work_task` round-trip outcome (a
/// serialized [`crate::acp::chat_authoring::AuthoringOutcome`]) into an MCP
/// `tools/call` result.
///
/// A refusal (feature off, folder not resolvable, bad cron) renders as readable
/// text with `isError: false`: the LLM reads the note, tells the user, or
/// retries with corrected arguments. Making it a tool error would abort the turn
/// over something the model can recover from on its own.
pub fn render_authoring_result(outcome: &Value) -> Value {
    let created = outcome
        .get("created")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let s = |k: &str| outcome.get(k).and_then(|v| v.as_str());
    let noun = match s("kind") {
        Some("work_task") => "task",
        _ => "automation",
    };
    let text = if created {
        let id = outcome.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        let title = s("title").unwrap_or("(untitled)");
        let mut out = format!("Created {noun} #{id}: {title}");
        if let Some(folder) = s("folder_name") {
            out.push_str(&format!("\nProject: {folder}"));
        }
        if let Some(agent) = s("agent_type") {
            out.push_str(&format!("\nAgent: {agent}"));
        }
        match (s("cron"), s("timezone")) {
            (Some(cron), Some(tz)) => out.push_str(&format!("\nSchedule: {cron} ({tz})")),
            (Some(cron), None) => out.push_str(&format!("\nSchedule: {cron}")),
            _ => {}
        }
        if let Some(next) = s("next_run_at") {
            out.push_str(&format!("\nNext run: {next}"));
        }
        if let Some(note) = s("note") {
            out.push_str(&format!("\n{note}"));
        }
        out
    } else {
        s("note")
            .unwrap_or("Could not create it; no reason was reported.")
            .to_string()
    };
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false,
        "structuredContent": outcome.clone(),
    })
}

/// Build the human-readable summary block for a found session: a metadata header
/// plus, when present, a "Recent messages" section.
fn render_session_summary_text(o: &Value) -> String {
    let s = |k: &str| o.get(k).and_then(|v| v.as_str());
    let id = o.get("session_id").and_then(|v| v.as_i64()).unwrap_or(0);
    let agent = s("agent_type").unwrap_or("unknown");
    let mut out = format!("Session #{id} ({agent})\n");
    if let Some(t) = s("title") {
        out.push_str(&format!("Title: {t}\n"));
    }
    let mut meta: Vec<String> = Vec::new();
    if let Some(v) = s("status") {
        meta.push(format!("status: {v}"));
    }
    if let Some(v) = s("git_branch") {
        meta.push(format!("branch: {v}"));
    }
    if let Some(v) = s("model") {
        meta.push(format!("model: {v}"));
    }
    if !meta.is_empty() {
        out.push_str(&meta.join(" | "));
        out.push('\n');
    }
    if let Some(v) = s("workspace_path") {
        out.push_str(&format!("Workspace: {v}\n"));
    }
    if let Some(n) = o.get("message_count").and_then(|v| v.as_u64()) {
        out.push_str(&format!("Messages: {n}\n"));
    }
    if o.get("is_delegation_child")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        if let Some(p) = o.get("parent_id").and_then(|v| v.as_i64()) {
            out.push_str(&format!("Delegation child of session #{p}\n"));
        }
    }
    if let Some(tokens) = o
        .get("stats")
        .and_then(|st| st.get("total_tokens"))
        .and_then(|v| v.as_u64())
    {
        out.push_str(&format!("Total tokens: {tokens}\n"));
    }
    if let Some(note) = s("note") {
        out.push_str(&format!("Note: {note}\n"));
    }
    if let Some(messages) = o.get("messages") {
        let total = messages.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
        let included = messages
            .get("included")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let truncated = messages
            .get("truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let suffix = if truncated {
            ", older turns omitted"
        } else {
            ""
        };
        out.push_str(&format!(
            "\nRecent messages ({included}/{total}{suffix}):\n"
        ));
        if let Some(items) = messages.get("items").and_then(|v| v.as_array()) {
            for item in items {
                let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("?");
                let body = item.get("text").and_then(|v| v.as_str()).unwrap_or("");
                let tools: Vec<&str> = item
                    .get("tools")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
                    .unwrap_or_default();
                out.push_str(&format!("- [{role}] {body}"));
                if !tools.is_empty() {
                    out.push_str(&format!(" (tools: {})", tools.join(", ")));
                }
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_features() -> CompanionFeatures {
        CompanionFeatures {
            feedback: true,
            ask: true,
            sessions: true,
            collaboration: true,
            tasks: true,
            automations: true,
            taskboard: true,
        }
    }

    fn collaboration_features() -> CompanionFeatures {
        CompanionFeatures {
            feedback: false,
            ask: false,
            sessions: false,
            collaboration: true,
            tasks: false,
            automations: false,
            taskboard: false,
        }
    }

    fn ctx(features: CompanionFeatures) -> CompanionContext {
        CompanionContext {
            parent_connection_id: "parent-1".into(),
            socket_path: "/tmp/codeg-mcp-test.sock".into(),
            token: "token".into(),
            features,
        }
    }

    async fn response(action: LineAction) -> JsonRpcResponse {
        match action {
            LineAction::Respond(response) => response,
            LineAction::Spawn(spawned) => spawned
                .future
                .await
                .response
                .expect("spawned call should respond"),
            LineAction::Silent => panic!("expected response"),
        }
    }

    #[tokio::test]
    async fn initialize_reports_mcp_capabilities() {
        let action = dispatch_line(
            &ctx(all_features()),
            Arc::new(InflightCalls::new()),
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        )
        .await;
        let response = response(action).await;
        assert_eq!(response.result.unwrap()["serverInfo"]["name"], "codeg-mcp");
    }

    #[tokio::test]
    async fn tools_list_contains_only_shared_host_bridge_tools() {
        let action = dispatch_line(
            &ctx(all_features()),
            Arc::new(InflightCalls::new()),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#,
        )
        .await;
        let response = response(action).await;
        let result = response.result.unwrap();
        let names: Vec<&str> = result["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert_eq!(
            names,
            vec![
                "check_user_feedback",
                "ask_user_question",
                "get_session_info",
                "list_sessions",
                "send_message",
                "create_automation",
                "create_work_task",
                "task_progress",
                "task_complete",
            ]
        );
        assert!(!TOOL_SCHEMA_JSON.contains("delegate_to_agent"));
        assert!(!TOOL_SCHEMA_JSON.contains("get_delegation_status"));
        assert!(!TOOL_SCHEMA_JSON.contains("cancel_delegation"));
    }

    #[tokio::test]
    async fn tools_list_gates_session_collaboration_as_one_group() {
        let action = dispatch_line(
            &ctx(collaboration_features()),
            Arc::new(InflightCalls::new()),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#,
        )
        .await;
        let response = response(action).await;
        let result = response.result.unwrap();
        let names: Vec<&str> = result["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert_eq!(names, vec!["list_sessions", "send_message"]);
    }

    #[tokio::test]
    async fn collaboration_calls_validate_before_spawning() {
        let list = serde_json::json!({
            "jsonrpc": "2.0", "id": 40, "method": "tools/call",
            "params": { "name": "list_sessions", "arguments": { "query": "review", "limit": 20 } }
        })
        .to_string();
        assert!(matches!(
            dispatch_line(
                &ctx(collaboration_features()),
                Arc::new(InflightCalls::new()),
                &list,
            )
            .await,
            LineAction::Spawn(_)
        ));

        let send = serde_json::json!({
            "jsonrpc": "2.0", "id": 41, "method": "tools/call",
            "params": { "name": "send_message", "arguments": {
                "target_session_ids": [7, "8", 7],
                "content": "Please review this.",
                "delivery_mode": "queue",
                "delivery_hint": "steer_if_supported",
                "expects_reply": true
            }}
        })
        .to_string();
        assert!(matches!(
            dispatch_line(
                &ctx(collaboration_features()),
                Arc::new(InflightCalls::new()),
                &send,
            )
            .await,
            LineAction::Spawn(_)
        ));

        for arguments in [
            serde_json::json!({ "target_session_ids": [], "content": "x" }),
            serde_json::json!({ "target_session_ids": ["bad"], "content": "x" }),
            serde_json::json!({ "target_session_ids": [7], "content": " " }),
            serde_json::json!({ "target_session_ids": [7], "content": "x", "delivery_mode": "interrupt" }),
            serde_json::json!({ "target_session_ids": [7], "content": "x", "delivery_hint": "interrupt" }),
            serde_json::json!({ "target_session_ids": [7], "content": "x", "delivery_mode": "deliver_only", "delivery_hint": "steer_if_supported" }),
        ] {
            let line = serde_json::json!({
                "jsonrpc": "2.0", "id": 42, "method": "tools/call",
                "params": { "name": "send_message", "arguments": arguments }
            })
            .to_string();
            let action = dispatch_line(
                &ctx(collaboration_features()),
                Arc::new(InflightCalls::new()),
                &line,
            )
            .await;
            let LineAction::Respond(response) = action else {
                panic!("invalid call must fail before spawning");
            };
            assert_eq!(response.error.expect("invalid call").code, -32602);
        }
    }

    #[test]
    fn collaboration_mcp_dedupe_key_is_stable_per_parent_and_request() {
        let first = mcp_call_dedupe_id("parent-1", &serde_json::json!(41));
        assert_eq!(first, mcp_call_dedupe_id("parent-1", &serde_json::json!(41)));
        assert_ne!(first, mcp_call_dedupe_id("parent-1", &serde_json::json!(42)));
        assert_ne!(first, mcp_call_dedupe_id("parent-2", &serde_json::json!(41)));
        assert!(first.starts_with("mcp:"));
    }

    #[test]
    fn collaboration_results_keep_delivery_distinct_from_completion() {
        let list = render_session_list_result(&serde_json::json!({
            "available": true,
            "sessions": [{
                "session_id": 7,
                "title": "Reviewer",
                "agent_type": "claude_code",
                "workspace_path": "D:/paper"
            }],
            "truncated": false
        }));
        assert!(list["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("7: Reviewer"));

        let sent = render_session_send_result(&serde_json::json!({
            "accepted": true,
            "event_id": "event-1",
            "deliveries": [{
                "target_session_id": 7,
                "target_title": "Reviewer",
                "state": "queued",
                "accepted": true
            }]
        }));
        let text = sent["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("event-1"));
        assert!(text.contains("queued"));
        assert!(text.contains("does not mean"));
    }

    #[test]
    fn feature_parser_is_explicit_and_independent() {
        let none = CompanionFeatures::parse(None);
        assert!(!none.feedback && !none.ask && !none.sessions && !none.collaboration);
        let parsed = CompanionFeatures::parse(Some(
            "feedback,ask,sessions,collaboration,tasks,automations,taskboard,unknown",
        ));
        assert!(parsed.feedback);
        assert!(parsed.ask);
        assert!(parsed.sessions);
        assert!(parsed.collaboration);
        assert!(parsed.tasks);
        assert!(parsed.automations);
        assert!(parsed.taskboard);
    }

    #[tokio::test]
    async fn removed_tool_is_rejected_even_if_called_directly() {
        let action = dispatch_line(
            &ctx(all_features()),
            Arc::new(InflightCalls::new()),
            r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"delegate_to_agent","arguments":{}}}"#,
        )
        .await;
        let response = response(action).await;
        assert_eq!(response.error.unwrap().code, -32602);
    }

    #[tokio::test]
    async fn cancellation_suppresses_an_ordinary_inflight_response() {
        let inflight = Arc::new(InflightCalls::new());
        let pending = Box::pin(futures_util::future::pending())
            as futures_util::future::BoxFuture<'static, std::io::Result<BrokerResponse>>;
        let action =
            register_and_spawn(inflight.clone(), json!(42), pending, render_feedback_result).await;
        let LineAction::Spawn(spawned) = action else {
            panic!("expected spawned call");
        };

        handle_cancel_notification(&inflight, &json!({"requestId": 42})).await;
        let result = tokio::time::timeout(Duration::from_secs(1), spawned.future)
            .await
            .expect("cancel wakes call");
        assert!(result.response.is_none());
    }

    #[tokio::test]
    async fn shutdown_drain_wakes_all_inflight_calls() {
        let inflight = Arc::new(InflightCalls::new());
        let (one_tx, one_rx) = oneshot::channel();
        let (two_tx, two_rx) = oneshot::channel();
        inflight
            .register("one".into(), InflightEntry { cancel_tx: one_tx })
            .await;
        inflight
            .register("two".into(), InflightEntry { cancel_tx: two_tx })
            .await;

        drain_inflight_calls(&inflight).await;

        assert!(one_rx.await.is_ok());
        assert!(two_rx.await.is_ok());
        assert!(inflight.drain_all().await.is_empty());
    }

    #[test]
    fn shared_renderers_keep_mcp_content_and_structured_content() {
        let feedback = render_feedback_result(&json!({
            "count": 1,
            "feedback": [{"text": "Please adjust", "created_at": "now"}]
        }));
        assert_eq!(feedback["isError"], false);
        assert!(feedback["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Please adjust"));

        let task = render_task_ack(&json!({"recorded": true, "message": "saved"}));
        assert_eq!(task["structuredContent"]["recorded"], true);

        let authoring = render_authoring_result(&json!({
            "kind": "work_task",
            "created": true,
            "id": 9,
            "message": "queued"
        }));
        assert_eq!(authoring["structuredContent"]["created"], true);
    }
}
