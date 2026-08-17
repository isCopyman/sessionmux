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
    client_host_control_help_round_trip, client_host_control_use_round_trip,
    client_send_message_round_trip, BrokerHostControlHelpRequest, BrokerHostControlUseRequest,
    BrokerResponse, BrokerSendMessageRequest,
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

    async fn list_inbox(
        &self,
        caller_session_id: i32,
        _filter: codeg_lib::acp::session_collaboration::SessionInboxFilter,
        _limit: u32,
    ) -> codeg_lib::acp::session_collaboration::SessionInboxOutcome {
        codeg_lib::acp::session_collaboration::SessionInboxOutcome {
            available: true,
            caller_session_id: Some(caller_session_id),
            ..Default::default()
        }
    }

    async fn read_message(
        &self,
        caller_session_id: i32,
        _event_id: String,
    ) -> codeg_lib::acp::session_collaboration::SessionMessageReadOutcome {
        codeg_lib::acp::session_collaboration::SessionMessageReadOutcome::unavailable(
            Some(caller_session_id),
            "not used",
        )
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
                title: "Test letter".into(),
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

struct MapParent(std::collections::HashMap<String, i32>);

#[async_trait]
impl ParentSessionLookup for MapParent {
    async fn current_conversation_id(&self, parent_connection_id: &str) -> Option<i32> {
        self.0.get(parent_connection_id).copied()
    }
}

/// Public MCP companion entry (`send_message` over the host bridge) through
/// real Host Core — not a direct `collaboration_service::send` call.
#[tokio::test]
async fn real_host_bridge_round_trips_reply_status_through_host_core() {
    use codeg_lib::acp::session_collaboration::{
        SessionCollaborationConfig, SessionCollaborationRuntimeConfig,
    };
    use codeg_lib::commands::collaboration::{
        collaboration_feed_core, DbSessionCollaboration,
    };
    use codeg_lib::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use codeg_lib::db::AppDatabase;
    use codeg_lib::models::{AgentType, CollaborationObligationState};
    use codeg_lib::prompt_queue::PromptQueueHandle;
    use codeg_lib::web::event_bridge::EventEmitter;

    let db = fresh_in_memory_db().await;
    let folder = seed_folder(&db, "/tmp/codeg-host-bridge-reply").await;
    let session_a = seed_conversation(&db, folder, AgentType::Codex).await;
    let session_b = seed_conversation(&db, folder, AgentType::ClaudeCode).await;

    let config = SessionCollaborationRuntimeConfig::new();
    config
        .set(SessionCollaborationConfig { enabled: true })
        .await;
    let collaboration = Arc::new(DbSessionCollaboration::new(
        Arc::new(AppDatabase {
            conn: db.conn.clone(),
        }),
        EventEmitter::Noop,
        PromptQueueHandle::disconnected_for_test(),
        config,
    ));

    let tokens = Arc::new(TokenRegistry::default());
    tokens
        .register(
            "token-a".to_string(),
            TokenEntry {
                parent_connection_id: "conn-a".to_string(),
                working_dir: PathBuf::from("/tmp/codeg-host-bridge-reply"),
                host_control_writes_allowed: true,
            },
        )
        .await;
    tokens
        .register(
            "token-b".to_string(),
            TokenEntry {
                parent_connection_id: "conn-b".to_string(),
                working_dir: PathBuf::from("/tmp/codeg-host-bridge-reply"),
                host_control_writes_allowed: true,
            },
        )
        .await;

    let mut parents = std::collections::HashMap::new();
    parents.insert("conn-a".to_string(), session_a);
    parents.insert("conn-b".to_string(), session_b);

    let endpoint = unique_endpoint("reply-round-trip");
    let task = {
        let listener = HostBridgeListener::new(
            tokens,
            Arc::new(MapParent(parents)),
            Arc::new(RecordingHostControl::default()),
            Arc::new(NoFeedback),
            Arc::new(NoQuestions),
            Arc::new(NoSessionInfo),
            collaboration,
            Arc::new(NoTaskTools),
            Arc::new(NoAuthoring),
        );
        let endpoint = endpoint.clone();
        tokio::spawn(async move { listener.run(endpoint).await })
    };
    let endpoint_text = endpoint.to_string_lossy().to_string();

    let outbound = send_message_round_trip_with_retry(
        &endpoint_text,
        &BrokerSendMessageRequest {
            token: "token-a".to_string(),
            spec: SessionMessageSpec {
                target_session_ids: vec![session_b],
                title: "Test letter".into(),
                content: "Please review this change.".to_string(),
                delivery_mode: SessionMessageDeliveryMode::DeliverOnly,
                steer_if_supported: false,
                expects_reply: true,
                reply_to_event_id: None,
                client_dedupe_id: "bridge-a-to-b".to_string(),
            },
        },
    )
    .await
    .expect("A -> B host-bridge send");
    assert_eq!(outbound.outcome["accepted"], true);
    assert_eq!(outbound.outcome["source_session_id"], session_a);
    let event_id = outbound.outcome["event_id"]
        .as_str()
        .expect("public send_message returns event_id")
        .to_string();

    let feed_b = collaboration_feed_core(&db.conn, session_b, None)
        .await
        .expect("B feed via public command");
    assert_eq!(feed_b.inbound.len(), 1);
    assert_eq!(feed_b.inbound[0].event_id, event_id);
    assert_eq!(
        feed_b.inbound[0].obligation_state,
        CollaborationObligationState::AwaitingReply
    );
    assert!(!feed_b.inbound[0].reply_received);

    let feed_a = collaboration_feed_core(&db.conn, session_a, None)
        .await
        .expect("A feed via public command");
    assert_eq!(feed_a.outbound.len(), 1);
    assert_eq!(
        feed_a.outbound[0].obligation_state,
        CollaborationObligationState::AwaitingReply
    );
    assert!(!feed_a.outbound[0].reply_received);

    let reply = send_message_round_trip_with_retry(
        &endpoint_text,
        &BrokerSendMessageRequest {
            token: "token-b".to_string(),
            spec: SessionMessageSpec {
                target_session_ids: vec![session_a],
                title: "Test letter".into(),
                content: "Looks good.".to_string(),
                delivery_mode: SessionMessageDeliveryMode::DeliverOnly,
                steer_if_supported: false,
                expects_reply: false,
                reply_to_event_id: Some(event_id.clone()),
                client_dedupe_id: "bridge-b-reply".to_string(),
            },
        },
    )
    .await
    .expect("B -> A host-bridge reply");
    assert_eq!(reply.outcome["accepted"], true);
    assert_eq!(reply.outcome["source_session_id"], session_b);

    let feed_a = collaboration_feed_core(&db.conn, session_a, None)
        .await
        .expect("A feed after reply");
    assert_eq!(feed_a.outbound[0].event_id, event_id);
    assert!(feed_a.outbound[0].reply_received);
    assert_eq!(
        feed_a.outbound[0].obligation_state,
        CollaborationObligationState::Resolved
    );

    task.abort();
    cleanup_endpoint(&endpoint).await;
}

struct LiveParent(tokio::sync::RwLock<std::collections::HashMap<String, i32>>);

#[async_trait]
impl ParentSessionLookup for LiveParent {
    async fn current_conversation_id(&self, parent_connection_id: &str) -> Option<i32> {
        self.0.read().await.get(parent_connection_id).copied()
    }
}

async fn host_use(
    endpoint: &str,
    token: &str,
    request_id: &str,
    action: &str,
    input: Value,
) -> BrokerResponse {
    host_control_round_trip_with_retry(
        endpoint,
        &BrokerHostControlUseRequest {
            token: token.to_string(),
            request_id: request_id.to_string(),
            action: action.to_string(),
            input,
        },
    )
    .await
    .unwrap_or_else(|error| panic!("{action} failed: {error}"))
}

/// One public Skill+MCP path: codeg_help/codeg_use create/rename/organize/place,
/// then send_message / reply_to, with source identity from the host token.
#[tokio::test]
async fn public_mcp_creates_organizes_and_replies_through_host_core() {
    use codeg_lib::acp::session_collaboration::{
        SessionCollaborationConfig, SessionCollaborationRuntimeConfig,
    };
    use codeg_lib::commands::collaboration::{
        collaboration_feed_core, DbSessionCollaboration,
    };
    use codeg_lib::commands::host_control::DbSessionHostControl;
    use codeg_lib::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use codeg_lib::db::AppDatabase;
    use codeg_lib::models::{AgentType, CollaborationObligationState};
    use codeg_lib::prompt_queue::PromptQueueHandle;
    use codeg_lib::web::event_bridge::EventEmitter;

    let cwd = std::env::temp_dir().join(format!(
        "codeg-public-mcp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::create_dir_all(&cwd).expect("public-path cwd");
    let cwd_text = cwd.to_string_lossy().into_owned();

    let db = fresh_in_memory_db().await;
    let folder = seed_folder(&db, &cwd_text).await;
    let session_a = seed_conversation(&db, folder, AgentType::Codex).await;

    let db = Arc::new(AppDatabase {
        conn: db.conn.clone(),
    });
    let host_control = Arc::new(DbSessionHostControl::new_for_tests(
        Arc::clone(&db),
        EventEmitter::Noop,
    ));
    let collab_config = SessionCollaborationRuntimeConfig::new();
    collab_config
        .set(SessionCollaborationConfig { enabled: true })
        .await;
    let collaboration = Arc::new(DbSessionCollaboration::new(
        Arc::clone(&db),
        EventEmitter::Noop,
        PromptQueueHandle::disconnected_for_test(),
        collab_config,
    ));

    let tokens = Arc::new(TokenRegistry::default());
    tokens
        .register(
            "token-a".to_string(),
            TokenEntry {
                parent_connection_id: "conn-a".to_string(),
                working_dir: cwd.clone(),
                host_control_writes_allowed: true,
            },
        )
        .await;
    tokens
        .register(
            "token-b".to_string(),
            TokenEntry {
                parent_connection_id: "conn-b".to_string(),
                working_dir: cwd.clone(),
                host_control_writes_allowed: true,
            },
        )
        .await;
    let parents = Arc::new(LiveParent(tokio::sync::RwLock::new(
        std::collections::HashMap::from([("conn-a".to_string(), session_a)]),
    )));

    let endpoint = unique_endpoint("public-mcp-loop");
    let task = {
        let listener = HostBridgeListener::new(
            tokens,
            parents.clone(),
            host_control,
            Arc::new(NoFeedback),
            Arc::new(NoQuestions),
            Arc::new(NoSessionInfo),
            collaboration,
            Arc::new(NoTaskTools),
            Arc::new(NoAuthoring),
        );
        let endpoint = endpoint.clone();
        tokio::spawn(async move { listener.run(endpoint).await })
    };
    let endpoint_text = endpoint.to_string_lossy().to_string();

    let help = client_host_control_help_round_trip(
        &endpoint_text,
        &BrokerHostControlHelpRequest {
            token: "token-a".to_string(),
            query: Some("session.create".to_string()),
            action: Some("session.create".to_string()),
        },
    )
    .await;
    let help = match help {
        Ok(help) => help,
        Err(_) => {
            let mut last = None;
            for _ in 0..50 {
                match client_host_control_help_round_trip(
                    &endpoint_text,
                    &BrokerHostControlHelpRequest {
                        token: "token-a".to_string(),
                        query: Some("session.create".to_string()),
                        action: Some("session.create".to_string()),
                    },
                )
                .await
                {
                    Ok(help) => {
                        last = Some(help);
                        break;
                    }
                    Err(error) => {
                        last = None;
                        let _ = error;
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                }
            }
            last.expect("codeg_help over host bridge")
        }
    };
    assert_eq!(help.outcome["available"], true);
    assert!(help.outcome["capabilities"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|capability| capability["action"] == "session.create"));

    let created = host_use(
        &endpoint_text,
        "token-a",
        "create-b",
        "session.create",
        json!({
            "harness": "claude_code",
            "title": "Session B"
        }),
    )
    .await;
    assert_eq!(created.outcome["accepted"], true, "{created:?}");
    let session_b = created.outcome["data"]["session_id"]
        .as_i64()
        .expect("create returns session_id") as i32;
    assert_ne!(session_b, session_a);
    parents
        .0
        .write()
        .await
        .insert("conn-b".to_string(), session_b);

    let renamed = host_use(
        &endpoint_text,
        "token-a",
        "rename-b",
        "session.rename",
        json!({
            "session_id": session_b,
            "title": "Reviewer B"
        }),
    )
    .await;
    assert_eq!(renamed.outcome["accepted"], true, "{renamed:?}");
    assert_eq!(renamed.outcome["data"]["title"], "Reviewer B");

    let collection = host_use(
        &endpoint_text,
        "token-a",
        "create-collection",
        "collection.create",
        json!({ "name": "Public path review" }),
    )
    .await;
    assert_eq!(collection.outcome["accepted"], true, "{collection:?}");
    let collection_id = collection.outcome["data"]["collection"]["id"]
        .as_i64()
        .expect("collection id") as i32;
    let added = host_use(
        &endpoint_text,
        "token-a",
        "add-b-collection",
        "collection.add_session",
        json!({
            "collection_id": collection_id,
            "session_id": session_b
        }),
    )
    .await;
    assert_eq!(added.outcome["accepted"], true, "{added:?}");

    let workbench = host_use(
        &endpoint_text,
        "token-a",
        "create-workbench",
        "workbench.create",
        json!({ "name": "Public path bench" }),
    )
    .await;
    assert_eq!(workbench.outcome["accepted"], true, "{workbench:?}");
    let workbench_id = workbench.outcome["data"]["workbench"]["id"]
        .as_i64()
        .expect("workbench id") as i32;
    let placed = host_use(
        &endpoint_text,
        "token-a",
        "place-b",
        "workbench.place_session",
        json!({
            "workbench_id": workbench_id,
            "session_id": session_b,
            "placement": "right"
        }),
    )
    .await;
    assert_eq!(placed.outcome["accepted"], true, "{placed:?}");
    assert_eq!(placed.outcome["stage"], "ui_requested");
    assert_eq!(placed.outcome["data"]["placement"], "right");

    let outbound = send_message_round_trip_with_retry(
        &endpoint_text,
        &BrokerSendMessageRequest {
            token: "token-a".to_string(),
            spec: SessionMessageSpec {
                target_session_ids: vec![session_b],
                title: "Test letter".into(),
                content: "Please review this change.".to_string(),
                delivery_mode: SessionMessageDeliveryMode::DeliverOnly,
                steer_if_supported: false,
                expects_reply: true,
                reply_to_event_id: None,
                client_dedupe_id: "public-a-to-b".to_string(),
            },
        },
    )
    .await
    .expect("A send_message");
    assert_eq!(outbound.outcome["accepted"], true);
    assert_eq!(outbound.outcome["source_session_id"], session_a);
    let event_id = outbound.outcome["event_id"]
        .as_str()
        .expect("event_id")
        .to_string();

    let feed_a = collaboration_feed_core(&db.conn, session_a, None)
        .await
        .expect("A outbox");
    assert_eq!(
        feed_a.outbound[0].obligation_state,
        CollaborationObligationState::AwaitingReply
    );
    assert!(!feed_a.outbound[0].reply_received);

    let reply = send_message_round_trip_with_retry(
        &endpoint_text,
        &BrokerSendMessageRequest {
            token: "token-b".to_string(),
            spec: SessionMessageSpec {
                target_session_ids: vec![session_a],
                title: "Test letter".into(),
                content: "Reviewed.".to_string(),
                delivery_mode: SessionMessageDeliveryMode::DeliverOnly,
                steer_if_supported: false,
                expects_reply: false,
                reply_to_event_id: Some(event_id.clone()),
                client_dedupe_id: "public-b-reply".to_string(),
            },
        },
    )
    .await
    .expect("B reply_to");
    assert_eq!(reply.outcome["accepted"], true);
    assert_eq!(reply.outcome["source_session_id"], session_b);

    let feed_a = collaboration_feed_core(&db.conn, session_a, None)
        .await
        .expect("A outbox after reply");
    assert!(feed_a.outbound[0].reply_received);
    assert_eq!(
        feed_a.outbound[0].obligation_state,
        CollaborationObligationState::Resolved
    );

    task.abort();
    cleanup_endpoint(&endpoint).await;
    let _ = std::fs::remove_dir_all(&cwd);
}
