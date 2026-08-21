//! Wire format for `codeg-mcp` companion ↔ main process round-trip over UDS
//! (Unix) or named pipe (Windows).
//!
//! The frame is dead simple: a little-endian `u32` byte length followed by
//! that many bytes of UTF-8 JSON. One request, one response — the companion
//! reopens the socket per `tools/call`. This trades a few extra connects for
//! a wire that's trivial to test and that doesn't need multiplexing.
//!
//! Why length-prefix instead of newline-delimited JSON? The LLM-issued
//! `task` arguments can contain newlines, and we'd rather avoid escaping
//! them into a single line. JSON-RPC over stdio uses newlines because
//! Content-Length headers add complexity; for an internal UDS we can do
//! better.
//!
//! ### Message shapes
//!
//! Inbound traffic is a tagged [`BrokerMessage`] enum, one variant per host
//! bridge tool. All arms are authenticated by the same per-launch `token`.
//!
//! ### Version coupling
//!
//! The companion (`codeg-mcp`) and the listener (inside the codeg main
//! process) ship in the SAME release artifact — the Tauri bundle, the
//! server Docker image, and the standalone binary tree all install both
//! binaries at the same path. The MCP config pointing the agent CLI at
//! `codeg-mcp` uses an absolute path that is replaced atomically by the
//! upgrade, so an old-version companion talking to a new-version listener
//! is not a supported configuration. As a consequence this protocol does
//! NOT carry a version field and the tagged-enum cutover from the older
//! plain-`BrokerRequest` frame is deliberately non-backward-compatible —
//! a stale companion would fail to decode and surface as a JSON-RPC
//! error to the LLM, which is preferable to silent misbehavior.

use std::io;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::acp::chat_authoring::{NewAutomationSpec, NewWorkTaskSpec};
use crate::acp::question::QuestionSpec;
use crate::acp::session_collaboration::{RoomPostSpec, SessionMessageSpec};

/// Discover the currently available progressive Host Control actions. Caller
/// identity is intentionally absent: the listener derives it from `token`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerHostControlHelpRequest {
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

/// Execute one action selected from the server-owned capability catalog.
/// `request_id` is minted by the companion from the parent connection and MCP
/// JSON-RPC id; it is not accepted from model arguments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerHostControlUseRequest {
    pub token: String,
    pub request_id: String,
    pub action: String,
    pub input: Value,
}

/// Pull the pending live-feedback notes for the parent session. Backs the
/// `check_user_feedback` MCP tool. Authenticated by the same per-launch
/// `token`; the listener resolves the parent connection from it and scopes the
/// drain to that connection so one parent can't read another's feedback.
/// Always returns an immediate snapshot — no blocking wait.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerFeedbackRequest {
    pub token: String,
}

/// Confirm delivery of feedback notes, marking them `Delivered`. Sent by the
/// companion AFTER its `check_user_feedback` round-trip wins (i.e. it is
/// returning the result to the agent), NOT by the listener at UDS-write time —
/// so a per-request cancel that suppresses the agent-facing response (the agent
/// staying alive) leaves the notes pending for the next check (at-least-once).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerCommitFeedbackRequest {
    pub token: String,
    pub ids: Vec<String>,
}

/// Ask the user one or more multiple-choice questions and BLOCK until they
/// answer. Backs the `ask_user_question` MCP tool. Authenticated by the same
/// per-launch `token`; the listener resolves the parent connection from it,
/// registers the questions (broadcasting the card to every attached client),
/// and parks the response until the user answers (or the tool call is canceled,
/// detected via peer-close on this connection). The companion has already
/// validated the schema, so `questions` is well-formed and carries stable ids.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerAskRequest {
    pub token: String,
    pub questions: Vec<QuestionSpec>,
}

/// Resolve a session the user referenced (`codeg://session/<id>`) into its
/// metadata + stats, optionally with its recent messages. Backs the
/// `get_session_info` MCP tool. Authenticated by the same per-launch `token`; the
/// lookup is by codeg's internal conversation id (the number in the reference),
/// so any non-deleted session the user references can be read.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerSessionRequest {
    pub token: String,
    /// codeg's internal conversation PK (the number in `codeg://session/<id>`).
    pub session_id: i32,
    /// How many of the most recent turns to include as compacted text. `None` /
    /// `0` → metadata only (no transcript parse); a positive value is clamped to
    /// [`crate::acp::session_info::MAX_SESSION_MESSAGES`] by the resolver.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_messages: Option<u32>,
}

/// Search the stable Session address book from a managed Session. The main
/// process derives the caller from `token`; the query only narrows human-facing
/// candidates and never participates in delivery identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerListSessionsRequest {
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Send one persistent communication event from the token's parent Session.
/// `SessionMessageSpec` deliberately has no source field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerSendMessageRequest {
    pub token: String,
    pub spec: SessionMessageSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerListInboxRequest {
    pub token: String,
    /// Wire key stays `box` (the tool schema's parameter name); the field is
    /// named `scope` internally to match `SessionMailboxScope`, the type this
    /// eventually parses into.
    #[serde(default, rename = "box", skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_session_id: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerReadMessageRequest {
    pub token: String,
    pub event_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerListRoomsRequest {
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerReadRoomRequest {
    pub token: String,
    pub room_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default)]
    pub unread: bool,
    #[serde(default)]
    pub needs_reply: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_event_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerReadRoomPostRequest {
    pub token: String,
    pub event_id: String,
    #[serde(default)]
    pub offset: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_chars: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerPostRoomRequest {
    pub token: String,
    pub spec: RoomPostSpec,
}

/// Report a progress milestone for the work task driving the parent session.
/// Backs the `task_progress` MCP tool. Authenticated by the per-launch `token`;
/// the listener resolves the parent connection from it and the task engine maps
/// that to the owning task + execution generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerTaskProgressRequest {
    pub token: String,
    pub message: String,
}

/// Report the final verdict (+ optional summary) for the work task driving the
/// parent session. Backs the `task_complete` MCP tool; the verdict decides how
/// the task settles when the turn ends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerTaskCompleteRequest {
    pub token: String,
    pub verdict: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// Create an automation (scheduled or manual) from the chat the caller is in.
/// Backs the `create_automation` MCP tool. Authenticated by the per-launch
/// `token`; the listener resolves the caller's conversation + working directory
/// from it so the target folder can default to the caller's own project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerCreateAutomationRequest {
    pub token: String,
    pub spec: NewAutomationSpec,
}

/// Queue a task on the work-task board from the chat the caller is in. Backs the
/// `create_work_task` MCP tool; same token scoping as the automation arm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerCreateWorkTaskRequest {
    pub token: String,
    pub spec: NewWorkTaskSpec,
}

/// Tagged top-level message dispatched by the listener. Adding new variants
/// is the wire-stable way to grow the broker protocol without touching the
/// frame layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrokerMessage {
    HostControlHelp(BrokerHostControlHelpRequest),
    HostControlUse(BrokerHostControlUseRequest),
    Feedback(BrokerFeedbackRequest),
    CommitFeedback(BrokerCommitFeedbackRequest),
    Ask(BrokerAskRequest),
    SessionInfo(BrokerSessionRequest),
    ListSessions(BrokerListSessionsRequest),
    SendMessage(BrokerSendMessageRequest),
    ListInbox(BrokerListInboxRequest),
    ReadMessage(BrokerReadMessageRequest),
    ListRooms(BrokerListRoomsRequest),
    ReadRoom(BrokerReadRoomRequest),
    ReadRoomPost(BrokerReadRoomPostRequest),
    PostRoom(BrokerPostRoomRequest),
    TaskProgress(BrokerTaskProgressRequest),
    TaskComplete(BrokerTaskCompleteRequest),
    CreateAutomation(BrokerCreateAutomationRequest),
    CreateWorkTask(BrokerCreateWorkTaskRequest),
}

/// The wrapped outcome the main process returns over the same socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerResponse {
    pub outcome: Value,
}

/// Maximum allowed frame size, 16 MiB. Guards against a misbehaving peer
/// allocating gigabytes when reading the length prefix.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// Write one length-prefixed JSON frame.
pub async fn write_frame<W, T>(stream: &mut W, value: &T) -> io::Result<()>
where
    W: AsyncWriteExt + Unpin,
    T: Serialize,
{
    let bytes = serde_json::to_vec(value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("encode: {e}")))?;
    let len: u32 = bytes
        .len()
        .try_into()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "frame > u32::MAX"))?;
    stream.write_all(&len.to_le_bytes()).await?;
    stream.write_all(&bytes).await?;
    stream.flush().await?;
    Ok(())
}

/// Read one length-prefixed JSON frame. Rejects frames larger than
/// [`MAX_FRAME_BYTES`].
pub async fn read_frame<R, T>(stream: &mut R) -> io::Result<T>
where
    R: AsyncReadExt + Unpin,
    T: for<'de> Deserialize<'de>,
{
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame {len} bytes exceeds cap {MAX_FRAME_BYTES}"),
        ));
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await?;
    serde_json::from_slice(&body)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("decode: {e}")))
}

/// One-shot client round-trip: connect, write one [`BrokerMessage`], read the
/// response, drop the connection. Public helpers differ only in which message
/// they build, so the connect/write/read is shared here.
#[cfg(unix)]
async fn message_round_trip(socket_path: &str, msg: &BrokerMessage) -> io::Result<BrokerResponse> {
    use tokio::net::UnixStream;
    let mut stream = UnixStream::connect(socket_path).await?;
    write_frame(&mut stream, msg).await?;
    read_frame(&mut stream).await
}

/// Windows path uses named pipes; the address format is `\\.\pipe\<name>`.
#[cfg(windows)]
async fn message_round_trip(socket_path: &str, msg: &BrokerMessage) -> io::Result<BrokerResponse> {
    let mut stream = open_named_pipe_with_retry(socket_path)
        .await
        .map_err(|e| io::Error::other(format!("open pipe: {e}")))?;
    write_frame(&mut stream, msg).await?;
    read_frame(&mut stream).await
}

/// Dispatch a `check_user_feedback` query and read back the
/// `{ "feedback": [..], "count": N }` envelope (the pending notes drained for
/// the parent session, possibly empty).
pub async fn client_feedback_round_trip(
    socket_path: &str,
    req: &BrokerFeedbackRequest,
) -> io::Result<BrokerResponse> {
    message_round_trip(socket_path, &BrokerMessage::Feedback(req.clone())).await
}

/// Confirm delivery of feedback notes (fire-and-forget). Reads the empty ack so
/// the listener can flush before the socket drops; the body carries nothing.
pub async fn client_commit_feedback(
    socket_path: &str,
    req: &BrokerCommitFeedbackRequest,
) -> io::Result<()> {
    let _ = message_round_trip(socket_path, &BrokerMessage::CommitFeedback(req.clone())).await?;
    Ok(())
}

/// Dispatch an `ask_user_question` request and BLOCK reading the response until
/// the user answers (or the question is canceled). The listener holds this
/// connection open for the whole wait — there is no `wait_ms`, the block is
/// inherent (waiting on a human). If the tool call is canceled, the companion
/// drops this future, closing the socket; the listener observes the peer-close
/// and tears the pending question down. Returns a `{ answers, declined }`
/// envelope.
pub async fn client_ask_round_trip(
    socket_path: &str,
    req: &BrokerAskRequest,
) -> io::Result<BrokerResponse> {
    message_round_trip(socket_path, &BrokerMessage::Ask(req.clone())).await
}

/// Dispatch a `get_session_info` request and read back the serialized
/// [`crate::acp::session_info::SessionInfo`] envelope (metadata + stats, and the
/// recent messages when `max_messages > 0`).
pub async fn client_session_round_trip(
    socket_path: &str,
    req: &BrokerSessionRequest,
) -> io::Result<BrokerResponse> {
    message_round_trip(socket_path, &BrokerMessage::SessionInfo(req.clone())).await
}

pub async fn client_host_control_help_round_trip(
    socket_path: &str,
    req: &BrokerHostControlHelpRequest,
) -> io::Result<BrokerResponse> {
    message_round_trip(socket_path, &BrokerMessage::HostControlHelp(req.clone())).await
}

pub async fn client_host_control_use_round_trip(
    socket_path: &str,
    req: &BrokerHostControlUseRequest,
) -> io::Result<BrokerResponse> {
    message_round_trip(socket_path, &BrokerMessage::HostControlUse(req.clone())).await
}

pub async fn client_list_sessions_round_trip(
    socket_path: &str,
    req: &BrokerListSessionsRequest,
) -> io::Result<BrokerResponse> {
    message_round_trip(socket_path, &BrokerMessage::ListSessions(req.clone())).await
}

pub async fn client_send_message_round_trip(
    socket_path: &str,
    req: &BrokerSendMessageRequest,
) -> io::Result<BrokerResponse> {
    message_round_trip(socket_path, &BrokerMessage::SendMessage(req.clone())).await
}

pub async fn client_list_inbox_round_trip(
    socket_path: &str,
    req: &BrokerListInboxRequest,
) -> io::Result<BrokerResponse> {
    message_round_trip(socket_path, &BrokerMessage::ListInbox(req.clone())).await
}

pub async fn client_read_message_round_trip(
    socket_path: &str,
    req: &BrokerReadMessageRequest,
) -> io::Result<BrokerResponse> {
    message_round_trip(socket_path, &BrokerMessage::ReadMessage(req.clone())).await
}

pub async fn client_list_rooms_round_trip(
    socket_path: &str,
    req: &BrokerListRoomsRequest,
) -> io::Result<BrokerResponse> {
    message_round_trip(socket_path, &BrokerMessage::ListRooms(req.clone())).await
}

pub async fn client_read_room_round_trip(
    socket_path: &str,
    req: &BrokerReadRoomRequest,
) -> io::Result<BrokerResponse> {
    message_round_trip(socket_path, &BrokerMessage::ReadRoom(req.clone())).await
}

pub async fn client_read_room_post_round_trip(
    socket_path: &str,
    req: &BrokerReadRoomPostRequest,
) -> io::Result<BrokerResponse> {
    message_round_trip(socket_path, &BrokerMessage::ReadRoomPost(req.clone())).await
}

pub async fn client_post_room_round_trip(
    socket_path: &str,
    req: &BrokerPostRoomRequest,
) -> io::Result<BrokerResponse> {
    message_round_trip(socket_path, &BrokerMessage::PostRoom(req.clone())).await
}

/// Dispatch a `task_progress` report and read back the `{ recorded }` ack.
pub async fn client_task_progress_round_trip(
    socket_path: &str,
    req: &BrokerTaskProgressRequest,
) -> io::Result<BrokerResponse> {
    message_round_trip(socket_path, &BrokerMessage::TaskProgress(req.clone())).await
}

/// Dispatch a `task_complete` report and read back the `{ recorded }` ack.
pub async fn client_task_complete_round_trip(
    socket_path: &str,
    req: &BrokerTaskCompleteRequest,
) -> io::Result<BrokerResponse> {
    message_round_trip(socket_path, &BrokerMessage::TaskComplete(req.clone())).await
}

/// Dispatch a `create_automation` request and read back the serialized
/// [`crate::acp::chat_authoring::AuthoringOutcome`].
pub async fn client_create_automation_round_trip(
    socket_path: &str,
    req: &BrokerCreateAutomationRequest,
) -> io::Result<BrokerResponse> {
    message_round_trip(socket_path, &BrokerMessage::CreateAutomation(req.clone())).await
}

/// Dispatch a `create_work_task` request and read back the serialized
/// [`crate::acp::chat_authoring::AuthoringOutcome`].
pub async fn client_create_work_task_round_trip(
    socket_path: &str,
    req: &BrokerCreateWorkTaskRequest,
) -> io::Result<BrokerResponse> {
    message_round_trip(socket_path, &BrokerMessage::CreateWorkTask(req.clone())).await
}

/// Total budget for `open()` retries on Windows named pipes.
#[cfg(windows)]
const PIPE_OPEN_RETRY_BUDGET: std::time::Duration = std::time::Duration::from_millis(200);

#[cfg(windows)]
const PIPE_OPEN_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(10);

/// Windows-only: `ClientOptions::open()` can fail with
/// `ERROR_PIPE_BUSY` (231) or `NotFound` during the brief window between
/// the listener accepting one connection and binding the next instance
/// (see `HostBridgeListener::run` on Windows). Retry with small backoff inside
/// a tight budget. Non-busy errors
/// (e.g. listener not running at all) propagate immediately.
#[cfg(windows)]
async fn open_named_pipe_with_retry(
    socket_path: &str,
) -> io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    use tokio::net::windows::named_pipe::ClientOptions;
    let attempt = async {
        loop {
            match ClientOptions::new().open(socket_path) {
                Ok(client) => return Ok::<_, io::Error>(client),
                Err(e) => {
                    let busy = e.raw_os_error() == Some(231);
                    let not_found = e.kind() == io::ErrorKind::NotFound;
                    if !(busy || not_found) {
                        return Err(e);
                    }
                    tokio::time::sleep(PIPE_OPEN_RETRY_DELAY).await;
                }
            }
        }
    };
    match tokio::time::timeout(PIPE_OPEN_RETRY_BUDGET, attempt).await {
        Ok(inner) => inner,
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "named pipe open: retry budget exhausted",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::duplex;

    #[tokio::test]
    async fn frame_round_trip_in_memory() {
        let (mut a, mut b) = duplex(8 * 1024);
        let msg = BrokerMessage::Feedback(BrokerFeedbackRequest {
            token: "tok".into(),
        });
        let writer = tokio::spawn(async move {
            write_frame(&mut a, &msg).await.unwrap();
        });
        let got: BrokerMessage = read_frame(&mut b).await.unwrap();
        writer.await.unwrap();
        match got {
            BrokerMessage::Feedback(req) => assert_eq!(req.token, "tok"),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[tokio::test]
    async fn response_round_trip_in_memory() {
        let (mut a, mut b) = duplex(8 * 1024);
        let response = BrokerResponse {
            outcome: json!({"recorded": true}),
        };
        let writer = tokio::spawn(async move {
            write_frame(&mut a, &response).await.unwrap();
        });
        let got: BrokerResponse = read_frame(&mut b).await.unwrap();
        writer.await.unwrap();
        assert_eq!(got.outcome, json!({"recorded": true}));
    }

    #[tokio::test]
    async fn oversized_frame_is_rejected_before_allocation() {
        let (mut a, mut b) = duplex(8);
        let writer = tokio::spawn(async move {
            let len = (MAX_FRAME_BYTES as u32 + 1).to_le_bytes();
            a.write_all(&len).await.unwrap();
        });
        let result: io::Result<BrokerMessage> = read_frame(&mut b).await;
        writer.await.unwrap();
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unix_socket_round_trip_keeps_shared_host_bridge_working() {
        use tokio::net::UnixListener;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("host-bridge.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let msg: BrokerMessage = read_frame(&mut conn).await.unwrap();
            match msg {
                BrokerMessage::Feedback(req) => assert_eq!(req.token, "tok"),
                other => panic!("unexpected variant: {other:?}"),
            }
            write_frame(
                &mut conn,
                &BrokerResponse {
                    outcome: json!({"feedback": [], "count": 0}),
                },
            )
            .await
            .unwrap();
        });

        let response = client_feedback_round_trip(
            path.to_str().unwrap(),
            &BrokerFeedbackRequest {
                token: "tok".into(),
            },
        )
        .await
        .unwrap();
        server.await.unwrap();
        assert_eq!(response.outcome["count"], 0);
    }
}
