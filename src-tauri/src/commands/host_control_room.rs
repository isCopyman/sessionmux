//! Room actions for Codeg's progressive Host Control MCP.
//!
//! The caller is the token-derived current Session. Creating a room always
//! includes that Session as owner. Messaging that must wake another Session
//! still goes through `send_message` with `room_id` so the existing dispatcher
//! can enqueue and wake.

use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::acp::host_control::{
    HostControlAccessLevel, HostControlCaller, HostControlCapability, HostControlUseOutcome,
};
use crate::db::service::{collaboration_room_service, collaboration_service};
use crate::db::AppDatabase;
use crate::models::{
    AddCollaborationRoomMembersInput, CollaborationInvocationPolicy, CreateCollaborationRoomInput,
    PostRoomMessageInput, RoomChanged,
};
use crate::web::event_bridge::{emit_event, EventEmitter, ROOM_CHANGED_EVENT};

pub struct RoomHostControl {
    db: Arc<AppDatabase>,
    emitter: EventEmitter,
}

impl RoomHostControl {
    pub fn new(db: Arc<AppDatabase>, emitter: EventEmitter) -> Self {
        Self { db, emitter }
    }

    pub fn capabilities(writes_allowed: bool) -> Vec<HostControlCapability> {
        let mut capabilities = vec![capability(
            "room.list",
            "List Rooms on a Workbench. Defaults to Workbench 1 (Main).",
            HostControlAccessLevel::Read,
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "workbench_id": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Workbench that owns the Rooms page. Defaults to 1."
                    }
                }
            }),
        )];
        if writes_allowed {
            capabilities.extend([
                capability(
                    "room.create",
                    "Create a shared Room. The calling Session becomes owner and is always a member. Pass at least one other Session id.",
                    HostControlAccessLevel::Write,
                    json!({
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["title", "member_session_ids"],
                        "properties": {
                            "title": { "type": "string", "minLength": 1, "maxLength": 80 },
                            "member_session_ids": {
                                "type": "array",
                                "minItems": 1,
                                "maxItems": 32,
                                "items": { "type": "integer", "minimum": 1 }
                            },
                            "workbench_id": { "type": "integer", "minimum": 1 }
                        }
                    }),
                ),
                capability(
                    "room.add_member",
                    "Add an existing Session to a Room the caller already belongs to.",
                    HostControlAccessLevel::Write,
                    json!({
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["room_id", "session_id"],
                        "properties": {
                            "room_id": { "type": "string", "minLength": 1 },
                            "session_id": { "type": "integer", "minimum": 1 }
                        }
                    }),
                ),
                capability(
                    "room.post",
                    "Post a Room-visible message as the calling Session. Empty mention_session_ids is record-only. This path does not wake targets; use send_message with room_id and delivery_mode=queue to invoke.",
                    HostControlAccessLevel::Write,
                    json!({
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["room_id", "content"],
                        "properties": {
                            "room_id": { "type": "string", "minLength": 1 },
                            "title": { "type": "string", "maxLength": 120 },
                            "content": { "type": "string", "minLength": 1 },
                            "mention_session_ids": {
                                "type": "array",
                                "maxItems": 16,
                                "items": { "type": "integer", "minimum": 1 }
                            },
                            "mention_all": { "type": "boolean", "default": false },
                            "reply_to_event_id": {
                                "type": "string",
                                "description": "Event id this post continues. Required for replies and later supplements on the same Room thread. Omitting it starts a new root."
                            }
                        }
                    }),
                ),
            ]);
        }
        capabilities
    }

    pub fn access_for(action: &str) -> Option<HostControlAccessLevel> {
        match action {
            "room.list" => Some(HostControlAccessLevel::Read),
            "room.create" | "room.add_member" | "room.post" => Some(HostControlAccessLevel::Write),
            _ => None,
        }
    }

    pub async fn use_action(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        match action.as_str() {
            "room.list" => {
                let params = match parse_input::<ListInput>(&action, input) {
                    Ok(params) => params,
                    Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
                };
                let workbench_id = params.workbench_id.unwrap_or(1);
                match collaboration_room_service::list(&self.db.conn, workbench_id).await {
                    Ok(rooms) => accepted(request_id, action, "read", json!({ "rooms": rooms })),
                    Err(error) => rejected(request_id, action, error),
                }
            }
            "room.create" => {
                let params = match parse_input::<CreateInput>(&action, input) {
                    Ok(params) => params,
                    Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
                };
                let mut members = params.member_session_ids;
                if !members.contains(&caller.current_session_id) {
                    members.push(caller.current_session_id);
                }
                match collaboration_room_service::create(
                    &self.db.conn,
                    CreateCollaborationRoomInput {
                        workbench_id: params.workbench_id.unwrap_or(1),
                        title: params.title,
                        member_conversation_ids: members,
                        created_by_conversation_id: caller.current_session_id,
                    },
                )
                .await
                {
                    Ok(room) => {
                        self.publish(&room.id, room.workbench_id);
                        accepted(request_id, action, "persisted", json!({ "room": room }))
                    }
                    Err(error) => rejected(request_id, action, error),
                }
            }
            "room.add_member" => {
                let params = match parse_input::<AddMemberInput>(&action, input) {
                    Ok(params) => params,
                    Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
                };
                if let Err(error) = collaboration_room_service::require_member(
                    &self.db.conn,
                    &params.room_id,
                    caller.current_session_id,
                )
                .await
                {
                    return rejected(request_id, action, error);
                }
                match collaboration_room_service::add_members(
                    &self.db.conn,
                    AddCollaborationRoomMembersInput {
                        room_id: params.room_id,
                        conversation_ids: vec![params.session_id],
                    },
                )
                .await
                {
                    Ok(room) => {
                        self.publish(&room.id, room.workbench_id);
                        accepted(request_id, action, "persisted", json!({ "room": room }))
                    }
                    Err(error) => rejected(request_id, action, error),
                }
            }
            "room.post" => {
                let params = match parse_input::<PostInput>(&action, input) {
                    Ok(params) => params,
                    Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
                };
                match collaboration_service::post_room(
                    &self.db.conn,
                    PostRoomMessageInput {
                        room_id: params.room_id,
                        source_conversation_id: caller.current_session_id,
                        target_conversation_ids: params.mention_session_ids.unwrap_or_default(),
                        mention_all: params.mention_all,
                        subject: params.title.unwrap_or_default(),
                        body: params.content,
                        client_dedupe_id: format!("host-room-{request_id}"),
                        invocation_policy: CollaborationInvocationPolicy::StoreOnly,
                        delivery_hint: Default::default(),
                        expects_reply: false,
                        urgency: Default::default(),
                        reply_to_event_id: params.reply_to_event_id,
                    },
                )
                .await
                {
                    Ok(posted) => {
                        if let Ok(detail) =
                            collaboration_room_service::get(&self.db.conn, &posted.room_id).await
                        {
                            self.publish(&posted.room_id, detail.workbench_id);
                        }
                        accepted(request_id, action, "persisted", json!({ "post": posted }))
                    }
                    Err(error) => rejected(request_id, action, error),
                }
            }
            _ => HostControlUseOutcome::rejected(
                request_id,
                action,
                "Unknown Room Host Control action",
            ),
        }
    }

    fn publish(&self, room_id: &str, workbench_id: i32) {
        emit_event(
            &self.emitter,
            ROOM_CHANGED_EVENT,
            RoomChanged {
                room_id: room_id.to_string(),
                workbench_id,
            },
        );
    }
}

fn accepted(request_id: String, action: String, stage: &str, data: Value) -> HostControlUseOutcome {
    HostControlUseOutcome {
        accepted: true,
        request_id,
        action,
        stage: stage.into(),
        replayed: false,
        data,
        note: None,
    }
}

fn rejected(
    request_id: String,
    action: String,
    error: crate::db::error::DbError,
) -> HostControlUseOutcome {
    HostControlUseOutcome::rejected(
        request_id,
        action,
        format!("Room operation failed: {error}"),
    )
}

fn capability(
    action: &str,
    description: &str,
    access: HostControlAccessLevel,
    input_schema: Value,
) -> HostControlCapability {
    let stage = match access {
        HostControlAccessLevel::Read => "read",
        HostControlAccessLevel::Write => "persisted",
    };
    HostControlCapability {
        action: action.into(),
        description: description.into(),
        access,
        input_schema,
        result_stages: vec![stage.into()],
    }
}

fn parse_input<T: DeserializeOwned>(action: &str, input: Value) -> Result<T, String> {
    serde_json::from_value(input).map_err(|error| format!("{action} input is invalid: {error}"))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListInput {
    #[serde(default)]
    workbench_id: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateInput {
    title: String,
    member_session_ids: Vec<i32>,
    #[serde(default)]
    workbench_id: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AddMemberInput {
    room_id: String,
    session_id: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PostInput {
    room_id: String,
    #[serde(default)]
    title: Option<String>,
    content: String,
    #[serde(default)]
    mention_session_ids: Option<Vec<i32>>,
    #[serde(default)]
    mention_all: bool,
    #[serde(default)]
    reply_to_event_id: Option<String>,
}
