//! Host bridge for the built-in `codeg-mcp` companion process.
//!
//! The companion exposes codeg-owned feedback, question, Session
//! collaboration, work-task, and authoring tools over stdio MCP and forwards
//! them to this process over a token-authenticated UDS / named pipe.
//!
//! The `delegation` directory name is a historical leftover: this module
//! used to own an agent-delegation workflow that has since been removed
//! entirely. What remains and lives here now is the MCP companion transport
//! and listener plumbing above, unrelated to delegating work between agents.

pub mod companion;
pub mod listener;
pub mod parent_watcher;
pub mod transport;
