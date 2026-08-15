//! Background reconciliation for agent-native Session stores.
//!
//! Native CLIs keep writing their own JSONL/SQLite history outside Codeg. This
//! task watches those stores, coalesces write bursts, and imports/refreshes only
//! Sessions whose cwd already belongs to an open Codeg Folder. The database and
//! import guard remain authoritative; the watcher is merely a wake-up signal.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use sea_orm::DatabaseConnection;
use tokio::sync::mpsc;
use tokio::time::{self, Instant};

use crate::commands::conversations::sync_registered_local_sessions_core;
use crate::parsers::{self, ExternalSource};
use crate::web::event_bridge::EventEmitter;

const EVENT_QUIET_DELAY: Duration = Duration::from_secs(2);
const EVENT_MAX_COALESCE_DELAY: Duration = Duration::from_secs(10);
const PERIODIC_RECONCILE_INTERVAL: Duration = Duration::from_secs(120);
const PARKED_TIMER_DELAY: Duration = Duration::from_secs(365 * 24 * 60 * 60);

fn coalesced_deadline(batch_started_at: Instant, now: Instant) -> Instant {
    std::cmp::min(
        now + EVENT_QUIET_DELAY,
        batch_started_at + EVENT_MAX_COALESCE_DELAY,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatchDepth {
    Direct,
    Recursive,
}

impl WatchDepth {
    fn notify_mode(self) -> RecursiveMode {
        match self {
            Self::Direct => RecursiveMode::NonRecursive,
            Self::Recursive => RecursiveMode::Recursive,
        }
    }
}

fn insert_target(targets: &mut HashMap<PathBuf, WatchDepth>, path: PathBuf, depth: WatchDepth) {
    targets
        .entry(path)
        .and_modify(|current| {
            if depth == WatchDepth::Recursive {
                *current = depth;
            }
        })
        .or_insert(depth);
}

fn add_source_targets(targets: &mut HashMap<PathBuf, WatchDepth>, source: ExternalSource) {
    if source.is_file {
        // SQLite-backed agents commonly write `-wal`/`-shm` siblings rather
        // than touching the main file on every transaction, so watch the
        // transcript-scoped parent directory rather than only the DB inode.
        if let Some(parent) = source.root.parent() {
            insert_target(targets, parent.to_path_buf(), WatchDepth::Direct);
        }
        return;
    }

    if let Some(allowed) = source.include_top {
        // Shared config homes (Gemini/Kimi/Cursor) contain credentials and
        // caches too. Watch each transcript allowlist entry recursively, plus
        // the home itself non-recursively so a missing entry created later is
        // noticed without observing unrelated subtrees.
        insert_target(targets, source.root.clone(), WatchDepth::Direct);
        for name in allowed {
            let child = source.root.join(name);
            let depth = if child.is_dir() {
                WatchDepth::Recursive
            } else {
                WatchDepth::Direct
            };
            insert_target(targets, child, depth);
        }
    } else {
        insert_target(targets, source.root, WatchDepth::Recursive);
    }
}

fn desired_watch_targets() -> HashMap<PathBuf, WatchDepth> {
    let mut targets = HashMap::new();
    for source in parsers::external_transcript_sources() {
        add_source_targets(&mut targets, source);
    }

    // Codex Desktop names live beside `sessions/`, in an append-style index.
    // Watch the home itself non-recursively rather than the index inode: that
    // catches both later appends and the first creation in a fresh CODEX_HOME,
    // without recursively observing credentials or unrelated subdirectories.
    add_codex_title_index_target(
        &mut targets,
        crate::parsers::codex::resolve_codex_home_dir(),
    );
    targets
}

fn add_codex_title_index_target(targets: &mut HashMap<PathBuf, WatchDepth>, codex_home: PathBuf) {
    insert_target(targets, codex_home, WatchDepth::Direct);
}

fn refresh_watches(watcher: &mut RecommendedWatcher, watched: &mut HashMap<PathBuf, WatchDepth>) {
    let desired = desired_watch_targets();

    let vanished: Vec<PathBuf> = watched
        .keys()
        .filter(|path| !path.exists() || !desired.contains_key(*path))
        .cloned()
        .collect();
    for path in vanished {
        let _ = watcher.unwatch(&path);
        watched.remove(&path);
    }

    for (path, depth) in desired {
        if !path.exists() || watched.get(&path) == Some(&depth) {
            continue;
        }
        if watched.remove(&path).is_some() {
            let _ = watcher.unwatch(&path);
        }
        match watcher.watch(Path::new(&path), depth.notify_mode()) {
            Ok(()) => {
                watched.insert(path, depth);
            }
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "[session-sync] failed to watch native Session source");
            }
        }
    }
}

async fn reconcile_once(conn: &DatabaseConnection, emitter: &EventEmitter) {
    match sync_registered_local_sessions_core(conn, emitter).await {
        Ok(Some(stats)) if stats.imported > 0 || stats.updated > 0 || stats.failed > 0 => {
            tracing::info!(
                imported = stats.imported,
                updated = stats.updated,
                failed = stats.failed,
                "[session-sync] native Session reconciliation complete"
            );
        }
        Ok(Some(_)) => {}
        Ok(None) => {
            tracing::debug!("[session-sync] import busy; reconciliation deferred");
        }
        Err(error) => {
            tracing::warn!(%error, "[session-sync] native Session reconciliation failed");
        }
    }
}

/// Process-wide native Session watcher. Spawn exactly once per Codeg backend
/// (desktop process or standalone server), never once per WebView/client.
pub async fn run_local_session_sync(conn: DatabaseConnection, emitter: EventEmitter) {
    let (event_tx, mut event_rx) = mpsc::channel::<()>(1);
    let mut watcher = match RecommendedWatcher::new(
        move |result: notify::Result<notify::Event>| {
            let Ok(event) = result else {
                return;
            };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            let _ = event_tx.try_send(());
        },
        Config::default(),
    ) {
        Ok(watcher) => watcher,
        Err(error) => {
            tracing::warn!(%error, "[session-sync] filesystem watcher unavailable; using periodic reconciliation");
            // Keep the task alive with the same periodic safety pass even when
            // the platform watcher cannot be constructed.
            loop {
                reconcile_once(&conn, &emitter).await;
                time::sleep(PERIODIC_RECONCILE_INTERVAL).await;
            }
        }
    };

    let mut watched = HashMap::new();
    refresh_watches(&mut watcher, &mut watched);
    reconcile_once(&conn, &emitter).await;

    let mut periodic = time::interval(PERIODIC_RECONCILE_INTERVAL);
    periodic.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
    // Consume interval's immediate first tick; the explicit pass above already
    // performed startup reconciliation.
    periodic.tick().await;

    let mut dirty = false;
    let mut batch_started_at = None;
    let debounce = time::sleep(PARKED_TIMER_DELAY);
    tokio::pin!(debounce);

    loop {
        tokio::select! {
            event = event_rx.recv() => {
                if event.is_none() {
                    return;
                }
                // Wait for a short quiet gap so one streaming response does not
                // trigger a whole-machine parse every few seconds, but cap the
                // wait so a continuously-writing agent cannot starve sync.
                let now = Instant::now();
                let started = *batch_started_at.get_or_insert(now);
                dirty = true;
                debounce.as_mut().reset(coalesced_deadline(started, now));
            }
            _ = &mut debounce, if dirty => {
                dirty = false;
                batch_started_at = None;
                // A parent-directory event may be the first creation of an
                // allowlisted `sessions/` subtree. Promote that new child to
                // its recursive watch before reconciling, so later appends do
                // not wait for the periodic safety refresh.
                refresh_watches(&mut watcher, &mut watched);
                reconcile_once(&conn, &emitter).await;
                debounce.as_mut().reset(Instant::now() + PARKED_TIMER_DELAY);
            }
            _ = periodic.tick() => {
                dirty = false;
                batch_started_at = None;
                refresh_watches(&mut watcher, &mut watched);
                reconcile_once(&conn, &emitter).await;
                debounce.as_mut().reset(Instant::now() + PARKED_TIMER_DELAY);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_sources_watch_the_parent_for_sqlite_sidecars() {
        let mut targets = HashMap::new();
        add_source_targets(
            &mut targets,
            ExternalSource {
                agent: "test",
                root: PathBuf::from("/tmp/native/state.db"),
                is_file: true,
                include_top: None,
            },
        );

        assert_eq!(
            targets.get(Path::new("/tmp/native")),
            Some(&WatchDepth::Direct)
        );
        assert!(!targets.contains_key(Path::new("/tmp/native/state.db")));
    }

    #[test]
    fn allowlisted_sources_do_not_watch_unrelated_subtrees_recursively() {
        let mut targets = HashMap::new();
        add_source_targets(
            &mut targets,
            ExternalSource {
                agent: "test",
                root: PathBuf::from("/tmp/shared-home"),
                is_file: false,
                include_top: Some(&["sessions", "session_index.jsonl"]),
            },
        );

        assert_eq!(
            targets.get(Path::new("/tmp/shared-home")),
            Some(&WatchDepth::Direct)
        );
        assert!(targets.contains_key(Path::new("/tmp/shared-home/sessions")));
        assert!(!targets.contains_key(Path::new("/tmp/shared-home/credentials")));
    }

    #[test]
    fn codex_title_index_watches_its_parent_so_first_creation_is_visible() {
        let mut targets = HashMap::new();
        add_codex_title_index_target(&mut targets, PathBuf::from("/tmp/codex-home"));

        assert_eq!(
            targets.get(Path::new("/tmp/codex-home")),
            Some(&WatchDepth::Direct)
        );
        assert!(!targets.contains_key(Path::new("/tmp/codex-home/session_index.jsonl")));
    }

    #[test]
    fn coalescing_waits_for_quiet_but_never_past_the_batch_cap() {
        let start = Instant::now();
        assert_eq!(coalesced_deadline(start, start), start + EVENT_QUIET_DELAY);
        assert_eq!(
            coalesced_deadline(start, start + Duration::from_secs(1)),
            start + Duration::from_secs(3)
        );
        assert_eq!(
            coalesced_deadline(start, start + Duration::from_secs(9)),
            start + EVENT_MAX_COALESCE_DELAY
        );
    }
}
