//! Host bridge for the built-in `codeg-mcp` companion process.
//!
//! The companion exposes codeg-owned feedback, question, Session
//! collaboration, work-task, and authoring tools over stdio MCP and forwards
//! them to this process over a token-authenticated UDS / named pipe.

pub mod companion;
pub mod listener;
pub mod parent_watcher;
pub mod transport;
