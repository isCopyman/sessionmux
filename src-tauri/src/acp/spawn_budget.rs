//! Timeout budgets for the agent-spawn handshake.
//!
//! A launch runs two blocking steps inside ONE outer wall clock, and both of
//! them used to be a flat 60s — which is how a multi-agent launch could blow
//! the wall clock while `session/new` was still legitimately working:
//!
//! ```text
//! spawn_agent (manager)                                 outer wall clock
//! └─ run_connection (connection)
//!    ├─ initialize          bounded by its own 60s      ─┐ both live inside
//!    └─ session/new         bounded by this module      ─┘ the wall clock
//! ```
//!
//! `session/new` is the expensive one: the agent CLI spawns and hand-shakes
//! every MCP server codeg put on the wire before it answers, and codeg itself
//! injects up to [`MAX_COMPANION_STDIO_SERVERS`] companion processes
//! (`codeg-mcp`, plus `codeg-mailbox` + `codeg-room` when Room channels are on).
//! So its budget scales with the number of companions injected into THIS
//! launch, and the outer wall clock absorbs the same scaling — otherwise
//! relaxing the inner budget would just make the outer one fire first and
//! report the vaguer error.
//!
//! Relationship, in one line:
//!
//! ```text
//! wall clock = session_new_budget(base, MAX_COMPANION_STDIO_SERVERS)
//!            + SPAWN_WALL_CLOCK_MARGIN_SECS
//! ```
//!
//! The margin is what makes the inner timeout observable: the outer clock
//! starts earlier (at process spawn), so without head-room it would always
//! pre-empt the inner one and the user would never see the phase-specific
//! "session/new" diagnosis. It does NOT need to cover a hung `initialize` —
//! that step has its own 60s bound, and when it trips the connection dies,
//! which resolves the outer wait immediately as `Aborted` rather than by
//! timeout.
//!
//! The manager cannot know the real companion count when it arms the wall
//! clock: injection happens later, inside the connection task, from live
//! settings toggles (Host Control / feedback / ask / sessions / chat authoring
//! / Room channels) and the per-launch owner label. So it budgets for the
//! worst case, which keeps `wall clock > inner budget` on every path.

use std::time::Duration;

/// Default base for both budgets — the pre-existing spawn-handshake timeout.
/// Picked to comfortably cover cold-start agents (claude-code/codex warm: <2s;
/// npx-fetched cold: 10–30s) without deadlocking the next concurrent
/// acp_connect when an agent is genuinely broken.
pub(crate) const SPAWN_HANDSHAKE_TIMEOUT_SECS: u64 = 60;

/// Extra `session/new` budget granted for every companion MCP server past the
/// first. One companion fits inside the base; each additional one is another
/// process the agent spawns, launches and hand-shakes serially before it can
/// answer.
pub(crate) const SESSION_NEW_PER_EXTRA_SERVER_SECS: u64 = 20;

/// Ceiling on the auto-scaled `session/new` budget at the default base: a
/// pathological server count must not turn a broken launch into a multi-minute
/// hang. Implemented as a cap on the ADDED time, so it bounds the scaling
/// without ever shrinking an explicit operator override — see
/// [`session_new_budget`].
pub(crate) const SESSION_NEW_BUDGET_CAP_SECS: u64 = 180;

/// Largest number of companion stdio servers a single launch can inject
/// (`codeg-mcp` + `codeg-mailbox` + `codeg-room`). Mirrors
/// `connection::companion_stdio_specs`; a test there pins the two together so
/// adding a fourth companion cannot silently under-budget the wall clock.
pub(crate) const MAX_COMPANION_STDIO_SERVERS: usize = 3;

/// Head-room the outer spawn wall clock keeps over the widest inner
/// `session/new` budget, so the inner timeout fires first and the error names
/// the phase that actually stalled.
pub(crate) const SPAWN_WALL_CLOCK_MARGIN_SECS: u64 = 15;

/// Read the spawn-handshake base from `CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS`,
/// falling back to [`SPAWN_HANDSHAKE_TIMEOUT_SECS`].
///
/// Backwards compatible by construction: the env var still means "the base the
/// spawn wall clock is built from". It now also feeds the `session/new` budget,
/// so raising it widens both layers together and the two can't cross.
pub(crate) fn spawn_handshake_timeout_from_env() -> Duration {
    let secs = std::env::var("CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(SPAWN_HANDSHAKE_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

/// Budget for the `session/new` round-trip: `base + per_extra * (servers - 1)`,
/// capped at [`SESSION_NEW_BUDGET_CAP_SECS`].
///
/// `companion_servers` is the number of codeg companion MCP servers injected
/// into this launch (0 when none were — e.g. an agent with `supports_mcp:
/// false`, or every tool group disabled). 0 and 1 both get the plain base.
///
/// The cap bounds the SCALING, not the total: at the default base it lands
/// exactly on [`SESSION_NEW_BUDGET_CAP_SECS`], while
/// `CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS=600` — an explicit "wait longer"
/// instruction — still gets its 600s plus the per-server head-room, rather than
/// being quietly clamped down to a built-in ceiling.
pub(crate) fn session_new_budget(base: Duration, companion_servers: usize) -> Duration {
    let extra_servers = companion_servers.saturating_sub(1) as u64;
    let extra_cap = SESSION_NEW_BUDGET_CAP_SECS.saturating_sub(SPAWN_HANDSHAKE_TIMEOUT_SECS);
    let extra_secs = SESSION_NEW_PER_EXTRA_SERVER_SECS
        .saturating_mul(extra_servers)
        .min(extra_cap);
    base.saturating_add(Duration::from_secs(extra_secs))
}

/// The outer wall clock `spawn_agent` holds the dedup lock for. Sized off the
/// WIDEST inner budget a launch could pick (see the module docs for why the
/// caller can't know the real companion count yet) plus
/// [`SPAWN_WALL_CLOCK_MARGIN_SECS`].
pub(crate) fn spawn_wall_clock_timeout(base: Duration) -> Duration {
    session_new_budget(base, MAX_COMPANION_STDIO_SERVERS)
        .saturating_add(Duration::from_secs(SPAWN_WALL_CLOCK_MARGIN_SECS))
}

/// Message for a `session/new` timeout.
///
/// Deliberately self-describing and distinct from the `initialize` timeout text:
/// both steps live in the same wall clock, so "the agent timed out during
/// startup" is not actionable. It names the phase, what it waited, and the
/// companion MCP servers this launch injected — the usual reason the step is
/// slow, and the number that explains the budget it was measured against.
pub(crate) fn session_new_timeout_message(
    budget: Duration,
    elapsed: Duration,
    companion_servers: &[&str],
) -> String {
    let servers = if companion_servers.is_empty() {
        "none".to_string()
    } else {
        companion_servers.join(", ")
    };
    format!(
        "Native session creation (session/new) timed out after {}s of a {}s budget. \
         This launch injected {} codeg companion MCP server(s) [{servers}]; the agent \
         spawns and hand-shakes every one of them before it answers session/new. Retry, \
         or raise CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS to widen the budget.",
        elapsed.as_secs(),
        budget.as_secs(),
        companion_servers.len(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 0 and 1 companions both get the plain base; every further companion adds
    /// exactly one `per_extra` slice.
    #[test]
    fn session_new_budget_scales_with_companion_count() {
        let base = Duration::from_secs(60);
        assert_eq!(session_new_budget(base, 0), Duration::from_secs(60));
        assert_eq!(session_new_budget(base, 1), Duration::from_secs(60));
        assert_eq!(session_new_budget(base, 2), Duration::from_secs(80));
        // The Room-enabled launch that motivated this: codeg-mcp +
        // codeg-mailbox + codeg-room.
        assert_eq!(session_new_budget(base, 3), Duration::from_secs(100));
    }

    /// The env var stays the base of BOTH layers, so an override scales through
    /// to the per-server arithmetic instead of being replaced by it.
    ///
    /// Snapshot env, mutate, restore. This single test owns
    /// `CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS` for the whole crate (tests share
    /// a process), so it also covers the plain default / garbage-value reads.
    #[test]
    fn session_new_budget_honors_env_base() {
        let prev = std::env::var("CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS").ok();

        std::env::remove_var("CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS");
        assert_eq!(
            spawn_handshake_timeout_from_env(),
            Duration::from_secs(SPAWN_HANDSHAKE_TIMEOUT_SECS)
        );

        std::env::set_var("CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS", "30");
        let base = spawn_handshake_timeout_from_env();
        assert_eq!(base, Duration::from_secs(30));
        assert_eq!(session_new_budget(base, 1), Duration::from_secs(30));
        assert_eq!(session_new_budget(base, 3), Duration::from_secs(70));

        // An override past the cap is honored, not clamped down to it.
        std::env::set_var("CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS", "600");
        let wide = spawn_handshake_timeout_from_env();
        assert_eq!(session_new_budget(wide, 3), Duration::from_secs(600 + 40));

        std::env::set_var("CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS", "garbage");
        assert_eq!(
            spawn_handshake_timeout_from_env(),
            Duration::from_secs(SPAWN_HANDSHAKE_TIMEOUT_SECS),
            "invalid value falls back to default"
        );

        match prev {
            Some(v) => std::env::set_var("CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS", v),
            None => std::env::remove_var("CODEG_ACP_SPAWN_HANDSHAKE_TIMEOUT_SECS"),
        }
    }

    /// A pathological server count is clamped at the ceiling.
    #[test]
    fn session_new_budget_caps_at_ceiling() {
        let base = Duration::from_secs(SPAWN_HANDSHAKE_TIMEOUT_SECS);
        // 60 + 9*20 = 240 → capped at 180.
        assert_eq!(
            session_new_budget(base, 10),
            Duration::from_secs(SESSION_NEW_BUDGET_CAP_SECS)
        );
        assert_eq!(
            session_new_budget(base, usize::MAX),
            Duration::from_secs(SESSION_NEW_BUDGET_CAP_SECS)
        );
        // The cap bounds the added time, so a smaller base stays below it and a
        // larger one is never clamped down to it.
        assert_eq!(
            session_new_budget(Duration::from_secs(10), usize::MAX),
            Duration::from_secs(10 + SESSION_NEW_BUDGET_CAP_SECS - SPAWN_HANDSHAKE_TIMEOUT_SECS)
        );
        assert_eq!(
            session_new_budget(Duration::from_secs(600), usize::MAX),
            Duration::from_secs(600 + SESSION_NEW_BUDGET_CAP_SECS - SPAWN_HANDSHAKE_TIMEOUT_SECS)
        );
    }

    /// The invariant the two layers depend on: whatever the inner step picks,
    /// the wall clock outlives it, so the phase-specific error is the one the
    /// user sees.
    #[test]
    fn wall_clock_outlives_every_inner_budget() {
        for base_secs in [1u64, 30, 60, 600] {
            let base = Duration::from_secs(base_secs);
            let wall = spawn_wall_clock_timeout(base);
            for servers in 0..=MAX_COMPANION_STDIO_SERVERS {
                assert!(
                    wall > session_new_budget(base, servers),
                    "wall clock {wall:?} must exceed the session/new budget for \
                     {servers} companion(s) at base {base:?}"
                );
            }
        }
        assert_eq!(
            spawn_wall_clock_timeout(Duration::from_secs(60)),
            Duration::from_secs(100 + SPAWN_WALL_CLOCK_MARGIN_SECS)
        );
    }

    /// The timeout text must name the phase and the injected servers — that is
    /// the whole point of separating it from the `initialize` timeout.
    #[test]
    fn session_new_timeout_message_names_phase_and_servers() {
        let msg = session_new_timeout_message(
            Duration::from_secs(100),
            Duration::from_secs(100),
            &["codeg-mcp", "codeg-mailbox", "codeg-room"],
        );
        assert!(msg.contains("session/new"), "{msg}");
        assert!(msg.contains("Native session creation"), "{msg}");
        assert!(msg.contains("3 codeg companion MCP server(s)"), "{msg}");
        assert!(msg.contains("codeg-mailbox"), "{msg}");
        assert!(msg.contains("codeg-room"), "{msg}");
        assert!(msg.contains("100s"), "{msg}");
        // Must not read like the Initialize timeout.
        assert!(!msg.contains("Initialize"), "{msg}");
    }

    /// No companions injected: still a session/new timeout, and the text says
    /// plainly that MCP fan-out is not the explanation this time.
    #[test]
    fn session_new_timeout_message_handles_no_companions() {
        let msg =
            session_new_timeout_message(Duration::from_secs(60), Duration::from_secs(61), &[]);
        assert!(msg.contains("session/new"), "{msg}");
        assert!(msg.contains("0 codeg companion MCP server(s)"), "{msg}");
        assert!(msg.contains("[none]"), "{msg}");
    }
}
