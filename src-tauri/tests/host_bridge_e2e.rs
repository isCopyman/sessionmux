//! Real socket coverage for the shared `codeg-mcp` host bridge.
//!
//! The legacy delegation broker used to own these transport tests. Host
//! Control and Session communication now share the same authenticated bridge,
//! so exercise those durable capabilities over a UDS or Windows named pipe.

#![cfg(any(unix, windows))]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use codeg_lib::acp::chat_authoring::{
    AuthoringContext, AuthoringOutcome, ChatAuthoringAccess, NewAutomationSpec, NewWorkTaskSpec,
};
use codeg_lib::acp::delegation::listener::{
    HostBridgeListener, ParentSessionLookup, TokenEntry, TokenRegistry,
};
use codeg_lib::acp::delegation::transport::{
    client_host_control_use_round_trip, client_send_message_round_trip,
    BrokerHostControlUseRequest, BrokerResponse, BrokerSendMessageRequest,
};
use codeg_lib::acp::feedback::{PendingFeedback, SessionFeedbackAccess};
use codeg_lib::acp::host_control::{
    HostControlAccess, HostControlCaller, HostControlHelpOutcome, HostControlUseOutcome,
};
use codeg_lib::acp::question::{QuestionSpec, RegisteredQuestion, SessionQuestionAccess};
use codeg_lib::acp::session_collaboration::{
    SessionCollaborationAccess, SessionListOutcome, SessionMessageDeliveryMode, SessionMessageSpec,
    SessionSendOutcome,
};
use codeg_lib::acp::session_info::{SessionInfo, SessionInfoAccess};
use codeg_lib::acp::work_task_tools::{TaskReportAck, WorkTaskToolAccess};
use serde_json::{json, Value};

struct FixedParent(i32);

#[async_trait]
impl ParentSessionLookup for FixedParent {
    async fn current_conversation_id(&self, _parent_connection_id: &str) -> Option<i32> {
        Some(self.0)
    }
}

#[derive(Default)]
struct RecordingHostControl {
    calls: tokio::sync::Mutex<Vec<(HostControlCaller, String, String, Value)>>,
}

#[async_trait]
impl HostControlAccess for RecordingHostControl {
    async fn help(
        &self,
        _caller: HostControlCaller,
        _query: Option<String>,
        _action: Option<String>,
    ) -> HostControlHelpOutcome {
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
        input: Value,
    ) -> HostControlUseOutcome {
        self.calls
            .lock()
            .await
            .push((caller.clone(), request_id.clone(), action.clone(), input));
        HostControlUseOutcome {
            accepted: true,
            request_id,
            action,
            stage: "read".to_string(),
            replayed: false,
            data: json!({ "caller_session_id": caller.current_session_id }),
            note: None,
        }
    }
}

#[derive(Default)]
struct RecordingCollaboration {
    sends: tokio::sync::Mutex<Vec<(i32, SessionMessageSpec)>>,
}

#[async_trait]
impl SessionCollaborationAccess for RecordingCollaboration {
    async fn list_sessions(
        &self,
        caller_session_id: i32,
        _query: Option<String>,
        _limit: u32,
    ) -> SessionListOutcome {
        SessionListOutcome {
            available: true,
            caller_session_id: Some(caller_session_id),
            ..Default::default()
        }
    }

    async fn send_message(
        &self,
        source_session_id: i32,
        spec: SessionMessageSpec,
    ) -> SessionSendOutcome {
        self.sends.lock().await.push((source_session_id, spec));
        SessionSendOutcome {
            accepted: true,
            source_session_id: Some(source_session_id),
            event_id: Some("event-e2e".to_string()),
            ..Default::default()
        }
    }
}

struct NoFeedback;

#[async_trait]
impl SessionFeedbackAccess for NoFeedback {
    async fn read_pending_feedback(&self, _parent_connection_id: &str) -> Vec<PendingFeedback> {
        Vec::new()
    }

    async fn commit_feedback_delivered(&self, _parent_connection_id: &str, _ids: Vec<String>) {}
}

struct NoQuestions;

#[async_trait]
impl SessionQuestionAccess for NoQuestions {
    async fn register_question(
        &self,
        _parent_connection_id: &str,
        _questions: Vec<QuestionSpec>,
    ) -> Option<RegisteredQuestion> {
        None
    }

    async fn cancel_question(&self, _parent_connection_id: &str, _question_id: &str) {}

    async fn cancel_questions_by_parent(&self, _parent_connection_id: &str) {}
}

struct NoSessionInfo;

#[async_trait]
impl SessionInfoAccess for NoSessionInfo {
    async fn resolve(&self, session_id: i32, _max_messages: u32) -> SessionInfo {
        SessionInfo::not_found(session_id)
    }
}

struct NoTaskTools;

#[async_trait]
impl WorkTaskToolAccess for NoTaskTools {
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

struct NoAuthoring;

#[async_trait]
impl ChatAuthoringAccess for NoAuthoring {
    async fn create_automation(
        &self,
        _ctx: AuthoringContext,
        _spec: NewAutomationSpec,
    ) -> AuthoringOutcome {
        AuthoringOutcome::default()
    }

    async fn create_work_task(
        &self,
        _ctx: AuthoringContext,
        _spec: NewWorkTaskSpec,
    ) -> AuthoringOutcome {
        AuthoringOutcome::default()
    }
}

fn listener(
    tokens: Arc<TokenRegistry>,
    host_control: Arc<RecordingHostControl>,
    collaboration: Arc<RecordingCollaboration>,
) -> Arc<HostBridgeListener> {
    HostBridgeListener::new(
        tokens,
        Arc::new(FixedParent(42)),
        host_control,
        Arc::new(NoFeedback),
        Arc::new(NoQuestions),
        Arc::new(NoSessionInfo),
        collaboration,
        Arc::new(NoTaskTools),
        Arc::new(NoAuthoring),
    )
}

fn unique_endpoint(tag: &str) -> PathBuf {
    let suffix = format!(
        "{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    );
    #[cfg(windows)]
    {
        PathBuf::from(format!(r"\\.\pipe\codeg-host-e2e-{suffix}"))
    }
    #[cfg(unix)]
    {
        std::env::temp_dir().join(format!("codeg-host-e2e-{suffix}.sock"))
    }
}

async fn host_control_round_trip_with_retry(
    endpoint: &str,
    request: &BrokerHostControlUseRequest,
) -> std::io::Result<BrokerResponse> {
    let mut last_error = None;
    for _ in 0..50 {
        match client_host_control_use_round_trip(endpoint, request).await {
            Ok(response) => return Ok(response),
            Err(error) => {
                last_error = Some(error);
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    }
    Err(last_error
        .unwrap_or_else(|| std::io::Error::other("host control round-trip retries exhausted")))
}

async fn send_message_round_trip_with_retry(
    endpoint: &str,
    request: &BrokerSendMessageRequest,
) -> std::io::Result<BrokerResponse> {
    let mut last_error = None;
    for _ in 0..50 {
        match client_send_message_round_trip(endpoint, request).await {
            Ok(response) => return Ok(response),
            Err(error) => {
                last_error = Some(error);
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    }
    Err(last_error
        .unwrap_or_else(|| std::io::Error::other("send message round-trip retries exhausted")))
}

async fn cleanup_endpoint(endpoint: &PathBuf) {
    #[cfg(unix)]
    {
        let _ = tokio::fs::remove_file(endpoint).await;
    }
    #[cfg(windows)]
    {
        let _ = endpoint;
    }
}

#[tokio::test]
async fn real_host_bridge_derives_identity_and_accepts_back_to_back_calls() {
    let tokens = Arc::new(TokenRegistry::default());
    tokens
        .register(
            "trusted-token".to_string(),
            TokenEntry {
                parent_connection_id: "parent-connection".to_string(),
                working_dir: PathBuf::from(r"C:\trusted\workspace"),
                host_control_writes_allowed: false,
            },
        )
        .await;
    let host_control = Arc::new(RecordingHostControl::default());
    let collaboration = Arc::new(RecordingCollaboration::default());
    let endpoint = unique_endpoint("host-control");
    let task = {
        let listener = listener(tokens, host_control.clone(), collaboration);
        let endpoint = endpoint.clone();
        tokio::spawn(async move { listener.run(endpoint).await })
    };
    let endpoint_text = endpoint.to_string_lossy().to_string();

    for index in 0..2 {
        let response = host_control_round_trip_with_retry(
            &endpoint_text,
            &BrokerHostControlUseRequest {
                token: "trusted-token".to_string(),
                request_id: format!("request-{index}"),
                action: "session.list".to_string(),
                input: json!({ "limit": 1 }),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("round-trip {index} failed: {error}"));
        assert_eq!(response.outcome["accepted"], true);
        assert_eq!(response.outcome["data"]["caller_session_id"], 42);
    }

    task.abort();
    cleanup_endpoint(&endpoint).await;

    let calls = host_control.calls.lock().await;
    assert_eq!(calls.len(), 2);
    for (index, (caller, request_id, action, input)) in calls.iter().enumerate() {
        assert_eq!(caller.current_session_id, 42);
        assert_eq!(caller.working_dir, PathBuf::from(r"C:\trusted\workspace"));
        assert!(!caller.writes_allowed);
        assert_eq!(request_id, &format!("request-{index}"));
        assert_eq!(action, "session.list");
        assert_eq!(input, &json!({ "limit": 1 }));
    }
}

#[tokio::test]
async fn real_host_bridge_routes_session_message_from_token_bound_source() {
    let tokens = Arc::new(TokenRegistry::default());
    tokens
        .register(
            "trusted-token".to_string(),
            TokenEntry {
                parent_connection_id: "parent-connection".to_string(),
                working_dir: PathBuf::from(r"C:\trusted\workspace"),
                host_control_writes_allowed: true,
            },
        )
        .await;
    let host_control = Arc::new(RecordingHostControl::default());
    let collaboration = Arc::new(RecordingCollaboration::default());
    let endpoint = unique_endpoint("session-message");
    let task = {
        let listener = listener(tokens, host_control, collaboration.clone());
        let endpoint = endpoint.clone();
        tokio::spawn(async move { listener.run(endpoint).await })
    };
    let endpoint_text = endpoint.to_string_lossy().to_string();

    let response = send_message_round_trip_with_retry(
        &endpoint_text,
        &BrokerSendMessageRequest {
            token: "trusted-token".to_string(),
            spec: SessionMessageSpec {
                target_session_ids: vec![7, 8],
                content: "Please compare the evidence.".to_string(),
                delivery_mode: SessionMessageDeliveryMode::Queue,
                steer_if_supported: false,
                expects_reply: true,
                reply_to_event_id: None,
                client_dedupe_id: "e2e-message".to_string(),
            },
        },
    )
    .await
    .expect("session message round-trip");

    task.abort();
    cleanup_endpoint(&endpoint).await;

    assert_eq!(response.outcome["accepted"], true);
    assert_eq!(response.outcome["source_session_id"], 42);
    assert_eq!(response.outcome["event_id"], "event-e2e");
    let sends = collaboration.sends.lock().await;
    assert_eq!(sends.len(), 1);
    assert_eq!(sends[0].0, 42);
    assert_eq!(sends[0].1.target_session_ids, vec![7, 8]);
    assert_eq!(sends[0].1.content, "Please compare the evidence.");
}
