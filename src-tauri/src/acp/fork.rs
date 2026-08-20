//! ACP `session/fork` support via raw JSON-RPC messages.
//!
//! sacp 11.0.0 exposes `ForkSessionRequest`, but `_meta` is a free-form map
//! (`Meta`) with no typed `claudeCode.options` envelope. We serialize the
//! typed request (so `anchor = None` stays byte-identical to today's payload:
//! no `_meta` field) and, when an anchor is present, splice the undocumented
//! `resumeSessionAt` pass-through in by hand.

use sacp::schema::{ForkSessionRequest, ForkSessionResponse, SessionId};
use sacp::{Agent, ConnectionTo, UntypedMessage};

use crate::acp::error::AcpError;

/// Build the `session/fork` params object.
///
/// `anchor = None` (or blank) produces the same JSON as
/// `ForkSessionRequest::new` — **no `_meta` key**. A non-empty anchor is
/// spliced in as `_meta.claudeCode.options.resumeSessionAt`.
pub fn fork_request_params(
    session_id: &SessionId,
    cwd: &str,
    anchor: Option<&str>,
) -> Result<serde_json::Value, AcpError> {
    let req = ForkSessionRequest::new(session_id.clone(), cwd);
    let mut params = serde_json::to_value(&req)
        .map_err(|e| AcpError::protocol(format!("Failed to serialize fork request: {e}")))?;
    if let Some(uuid) = anchor.map(str::trim).filter(|s| !s.is_empty()) {
        let obj = params.as_object_mut().ok_or_else(|| {
            AcpError::protocol("fork request serialized to a non-object".to_string())
        })?;
        obj.insert(
            "_meta".to_string(),
            serde_json::json!({
                "claudeCode": {
                    "options": {
                        "resumeSessionAt": uuid
                    }
                }
            }),
        );
    }
    Ok(params)
}

/// JSON stored on `fork_relation.anchor` for a `fork_at_message` edge.
pub fn fork_at_message_anchor_json(resume_session_at: &str) -> String {
    serde_json::json!({ "resumeSessionAt": resume_session_at }).to_string()
}

/// Send a `session/fork` request over an existing ACP connection.
///
/// Returns the full `ForkSessionResponse` so the caller can attach directly
/// without a separate `session/load` round-trip, plus the raw top-level `models`
/// value (captured before the typed deserialize drops it) so the Grok path can
/// parse per-model reasoning-effort data. `None` when the response has no
/// `models` field.
pub async fn fork_session(
    cx: &ConnectionTo<Agent>,
    session_id: &SessionId,
    cwd: &str,
    anchor: Option<&str>,
) -> Result<(ForkSessionResponse, Option<serde_json::Value>), AcpError> {
    let params = fork_request_params(session_id, cwd, anchor)?;
    let untyped_req = UntypedMessage::new("session/fork", &params)
        .map_err(|e| AcpError::protocol(format!("Failed to build fork request: {e}")))?;

    let raw_response: serde_json::Value = cx
        .send_request_to(Agent, untyped_req)
        .block_task()
        .await
        .map_err(AcpError::from_session_fork_failure)?;

    let models = raw_response.get("models").cloned();
    let response: ForkSessionResponse = serde_json::from_value(raw_response)
        .map_err(|e| AcpError::protocol(format!("Failed to parse fork response: {e}")))?;

    Ok((response, models))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sid(id: &str) -> SessionId {
        SessionId::new(id.to_string())
    }

    #[test]
    fn head_fork_params_omit_meta() {
        let params = fork_request_params(&sid("s-parent"), "/tmp/proj", None).unwrap();
        assert!(
            params.get("_meta").is_none(),
            "head fork must not send _meta, got {params}"
        );
        assert_eq!(params["sessionId"], "s-parent");
        assert_eq!(params["cwd"], "/tmp/proj");
    }

    #[test]
    fn blank_anchor_is_treated_as_head_fork() {
        for blank in [Some(""), Some("   "), Some("\t")] {
            let params = fork_request_params(&sid("s-parent"), "/tmp/proj", blank).unwrap();
            assert!(
                params.get("_meta").is_none(),
                "blank anchor {blank:?} must omit _meta, got {params}"
            );
        }
    }

    #[test]
    fn anchored_fork_splices_resume_session_at() {
        let params =
            fork_request_params(&sid("s-parent"), "/tmp/proj", Some("uuid-kept-tail")).unwrap();
        assert_eq!(
            params["_meta"]["claudeCode"]["options"]["resumeSessionAt"],
            "uuid-kept-tail"
        );
        assert_eq!(params["sessionId"], "s-parent");
    }
}
