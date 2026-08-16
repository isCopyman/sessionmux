use std::collections::HashMap;
use std::time::Instant;

use crate::acp::types::PermissionOptionInfo;
use crate::chat_channel::types::{ChannelMessageTarget, SentMessageId};
use crate::models::agent::AgentType;

pub struct PendingPermission {
    pub request_id: String,
    pub tool_description: String,
    pub options: Vec<PermissionOptionInfo>,
    pub sent_message_id: Option<SentMessageId>,
}

pub struct ActiveSession {
    pub channel_id: i32,
    pub sender_id: String,
    pub target: ChannelMessageTarget,
    pub conversation_id: i32,
    pub connection_id: String,
    pub agent_type: AgentType,
    pub content_buffer: String,
    pub tool_calls: Vec<String>,
    /// Stores raw_input by tool_call_id for detail extraction on completion.
    pub tool_call_inputs: HashMap<String, String>,
    pub last_flushed: Instant,
    pub pending_prompt: Option<String>,
    pub permission_pending: Option<PendingPermission>,
}

#[derive(Default)]
pub struct SessionBridge {
    sessions: HashMap<String, ActiveSession>,
}

impl SessionBridge {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, connection_id: String, session: ActiveSession) {
        self.sessions.insert(connection_id, session);
    }

    pub fn remove(&mut self, connection_id: &str) -> Option<ActiveSession> {
        self.sessions.remove(connection_id)
    }

    pub fn get(&self, connection_id: &str) -> Option<&ActiveSession> {
        self.sessions.get(connection_id)
    }

    pub fn get_mut(&mut self, connection_id: &str) -> Option<&mut ActiveSession> {
        self.sessions.get_mut(connection_id)
    }

    pub fn find_by_sender(&self, channel_id: i32, sender_id: &str) -> Option<&ActiveSession> {
        self.sessions
            .values()
            .find(|s| s.channel_id == channel_id && s.sender_id == sender_id)
    }

    pub fn find_by_target(&self, target: &ChannelMessageTarget) -> Option<&ActiveSession> {
        self.sessions
            .values()
            .find(|s| s.target.matches_thread(target))
    }

    pub fn find_by_sender_mut(
        &mut self,
        channel_id: i32,
        sender_id: &str,
    ) -> Option<&mut ActiveSession> {
        self.sessions
            .values_mut()
            .find(|s| s.channel_id == channel_id && s.sender_id == sender_id)
    }

    pub fn find_by_target_mut(
        &mut self,
        target: &ChannelMessageTarget,
    ) -> Option<&mut ActiveSession> {
        self.sessions
            .values_mut()
            .find(|s| s.target.matches_thread(target))
    }

    pub fn all_sessions(&self) -> impl Iterator<Item = &ActiveSession> {
        self.sessions.values()
    }

    pub fn all_sessions_mut(&mut self) -> impl Iterator<Item = &mut ActiveSession> {
        self.sessions.values_mut()
    }
}
