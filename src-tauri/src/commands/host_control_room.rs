//! Room lifecycle for Codeg's progressive Host Control MCP.
//!
//! The caller is the token-derived current Session. Creating a room always
//! includes that Session as host. Messaging lives on the dedicated MCP tools
//! `list_rooms` / `read_room` / `read_room_post` / `post_room`, not on this gateway.

use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::acp::host_control::{
    HostControlAccessLevel, HostControlCaller, HostControlCapability, HostControlUseOutcome,
};
use crate::db::service::collaboration_room_service;
use crate::db::AppDatabase;
use crate::models::{AddCollaborationRoomMembersInput, CreateCollaborationRoomInput, RoomChanged};
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
        let mut capabilities = vec![
            capability(
            "room.list",
            "List Rooms on a Workbench. Defaults to Workbench 1 (Main). Alias of room.list_workbench. Agents that only need Rooms they already belong to should call list_rooms instead.",
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
        ),
            capability(
            "room.list_workbench",
            "List every Room on a Workbench (defaults to 1). Not the same as MCP list_rooms, which only returns Rooms this Session already joined.",
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
        ),
        ];
        if writes_allowed {
            capabilities.extend([
                capability(
                    "room.create",
                    "Create a shared Room. The calling Session becomes the host and is always a member. Pass at least one other Session id. After create, post with post_room, not send_message.",
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
                    "room.set_workbench",
                    "Move a Room to another Workbench. Same-workbench is a no-op. Members are not rewritten. The caller must already belong to the Room.",
                    HostControlAccessLevel::Write,
                    json!({
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["room_id", "workbench_id"],
                        "properties": {
                            "room_id": { "type": "string", "minLength": 1 },
                            "workbench_id": { "type": "integer", "minimum": 1 }
                        }
                    }),
                ),
            ]);
        }
        capabilities
    }

    pub fn access_for(action: &str) -> Option<HostControlAccessLevel> {
        match action {
            "room.list" | "room.list_workbench" => Some(HostControlAccessLevel::Read),
            "room.create" | "room.add_member" | "room.set_workbench" => {
                Some(HostControlAccessLevel::Write)
            }
            // room.post was removed, but the action stays addressable so the
            // gateway routes it here and the caller gets the migration hint
            // below instead of a bare "Unknown action".
            "room.post" => Some(HostControlAccessLevel::Write),
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
            "room.list" | "room.list_workbench" => {
                let params = match parse_input::<ListInput>(&action, input) {
                    Ok(params) => params,
                    Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
                };
                let workbench_id = params.workbench_id.unwrap_or(1);
                match collaboration_room_service::list_for_workbench(&self.db.conn, workbench_id)
                    .await
                {
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
                        collection_id: None,
                        root_folder_id: None,
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
            "room.set_workbench" => {
                let params = match parse_input::<SetWorkbenchInput>(&action, input) {
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
                match collaboration_room_service::set_workbench(
                    &self.db.conn,
                    &params.room_id,
                    params.workbench_id,
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
            "room.post" => HostControlUseOutcome::rejected(
                request_id,
                action,
                "room.post was removed. Use the post_room MCP tool to post in a Room.",
            ),
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
struct SetWorkbenchInput {
    room_id: String,
    workbench_id: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::agent::AgentType;
    use std::path::PathBuf;

    /// Returns the gateway plus `(folder, caller Session, other Session)`.
    async fn fixture() -> (RoomHostControl, i32, i32, i32) {
        let db = Arc::new(fresh_in_memory_db().await);
        let folder = seed_folder(&db, "/tmp/codeg-host-control-room").await;
        let caller = seed_conversation(&db, folder, AgentType::Codex).await;
        let other = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        (
            RoomHostControl::new(db, EventEmitter::Noop),
            folder,
            caller,
            other,
        )
    }

    fn caller(id: i32) -> HostControlCaller {
        HostControlCaller {
            current_session_id: id,
            working_dir: PathBuf::from("/tmp/codeg-host-control-room"),
            writes_allowed: true,
        }
    }

    #[test]
    fn access_for_maps_every_room_action() {
        for action in ["room.list", "room.list_workbench"] {
            assert_eq!(
                RoomHostControl::access_for(action),
                Some(HostControlAccessLevel::Read),
                "{action}"
            );
        }
        for action in [
            "room.create",
            "room.add_member",
            "room.set_workbench",
            "room.post",
        ] {
            assert_eq!(
                RoomHostControl::access_for(action),
                Some(HostControlAccessLevel::Write),
                "{action}"
            );
        }
        assert_eq!(RoomHostControl::access_for("room.delete"), None);
    }

    #[tokio::test]
    async fn removed_room_post_reaches_the_migration_hint() {
        let (rooms, _, caller_id, _) = fixture().await;
        let outcome = rooms
            .use_action(
                &caller(caller_id),
                "req-post".into(),
                "room.post".into(),
                json!({}),
            )
            .await;
        assert!(!outcome.accepted);
        let note = outcome.note.unwrap();
        assert!(note.contains("post_room"), "names the replacement: {note}");
    }

    #[tokio::test]
    async fn create_makes_the_caller_a_member_and_lists_the_room() {
        let (rooms, folder_id, caller_id, other_id) = fixture().await;
        let created = rooms
            .use_action(
                &caller(caller_id),
                "req-create".into(),
                "room.create".into(),
                json!({ "title": "Plan", "member_session_ids": [other_id] }),
            )
            .await;
        assert!(created.accepted);
        assert_eq!(created.stage, "persisted");
        let room_id = created.data["room"]["id"].as_str().unwrap().to_string();
        let members: Vec<i64> = created.data["room"]["members"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|member| member["conversationId"].as_i64())
            .collect();
        assert!(members.contains(&i64::from(caller_id)));
        assert!(members.contains(&i64::from(other_id)));

        // room.create carries no placement of its own. Without the service-side
        // fallback the Room lands with neither a Collection nor a Path, and the
        // sidebar Collection tree — which files Rooms by one or the other — has
        // nowhere to draw it.
        assert_eq!(
            created.data["room"]["rootFolderId"].as_i64(),
            Some(i64::from(folder_id)),
            "the Room must adopt the calling Session's Path: {}",
            created.data["room"]
        );

        let listed = rooms
            .use_action(
                &caller(caller_id),
                "req-list".into(),
                "room.list".into(),
                json!({}),
            )
            .await;
        assert!(listed.accepted);
        let ids: Vec<&str> = listed.data["rooms"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|room| room["id"].as_str())
            .collect();
        assert!(ids.contains(&room_id.as_str()));
    }

    #[tokio::test]
    async fn set_workbench_requires_membership_and_moves_the_room() {
        use crate::db::service::workbench_service;
        let (rooms, _, caller_id, other_id) = fixture().await;
        let created = rooms
            .use_action(
                &caller(caller_id),
                "req-create".into(),
                "room.create".into(),
                json!({ "title": "Plan", "member_session_ids": [other_id] }),
            )
            .await;
        assert!(created.accepted);
        let room_id = created.data["room"]["id"].as_str().unwrap().to_string();
        let second = workbench_service::create(&rooms.db.conn, Some("Review".into()))
            .await
            .expect("second workbench");

        let stranger = rooms
            .use_action(
                &caller(other_id + 999),
                "req-stranger".into(),
                "room.set_workbench".into(),
                json!({ "room_id": room_id, "workbench_id": second.id }),
            )
            .await;
        assert!(!stranger.accepted, "a non-member must not move the Room");

        let moved = rooms
            .use_action(
                &caller(caller_id),
                "req-move".into(),
                "room.set_workbench".into(),
                json!({ "room_id": room_id, "workbench_id": second.id }),
            )
            .await;
        assert!(moved.accepted, "{:?}", moved.note);
        assert_eq!(moved.data["room"]["workbenchId"], second.id);

        let same = rooms
            .use_action(
                &caller(caller_id),
                "req-same".into(),
                "room.set_workbench".into(),
                json!({ "room_id": room_id, "workbench_id": second.id }),
            )
            .await;
        assert!(
            same.accepted,
            "same-workbench must be a no-op, not an error"
        );
        assert_eq!(same.data["room"]["workbenchId"], second.id);
    }
}
