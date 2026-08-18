//! SQLite source of truth for per-Session idle continuation timers.

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DbBackend, EntityTrait, QueryResult, Statement};

use crate::db::entities::conversation;
use crate::db::error::DbError;
use crate::models::session_timer::{
    CreateSessionTimerInput, SessionTimerInfo, UpdateSessionTimerInput, MAX_IDLE_GRACE_SECS,
};

fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, values)
}

fn validation(message: impl Into<String>) -> DbError {
    DbError::Validation(message.into())
}

fn parse_timestamp(row: &QueryResult, column: &str) -> Result<DateTime<Utc>, DbError> {
    row.try_get("", column).map_err(DbError::from)
}

fn parse_timer(row: &QueryResult) -> Result<SessionTimerInfo, DbError> {
    Ok(SessionTimerInfo {
        id: row.try_get("", "id")?,
        conversation_id: row.try_get("", "conversation_id")?,
        idle_grace_secs: row.try_get("", "idle_secs")?,
        prompt_text: row.try_get("", "prompt_text")?,
        enabled: row.try_get::<i32>("", "enabled")? != 0,
        last_fired_at: row.try_get("", "last_fired_at")?,
        fire_count: row.try_get("", "fire_count")?,
        strike_count: row.try_get("", "strike_count")?,
        auto_paused_at: row.try_get("", "auto_paused_at")?,
        auto_pause_reason: row.try_get("", "auto_pause_reason")?,
        created_at: parse_timestamp(row, "created_at")?,
        updated_at: parse_timestamp(row, "updated_at")?,
    })
}

fn validate_prompt(value: &str) -> Result<String, DbError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(validation("A timer prompt must not be empty"));
    }
    if value.len() > 8_000 {
        return Err(validation("A timer prompt must be at most 8000 characters"));
    }
    Ok(value.to_string())
}

fn validate_grace(value: i64) -> Result<i64, DbError> {
    if !(1..=MAX_IDLE_GRACE_SECS).contains(&value) {
        return Err(validation(format!(
            "idleGraceSecs must be between 1 and {MAX_IDLE_GRACE_SECS} seconds"
        )));
    }
    Ok(value)
}

async fn ensure_conversation<C: ConnectionTrait>(
    conn: &C,
    conversation_id: i32,
) -> Result<(), DbError> {
    let exists = conversation::Entity::find_by_id(conversation_id)
        .one(conn)
        .await?
        .is_some_and(|row| row.deleted_at.is_none());
    if !exists {
        return Err(DbError::NotFound(format!("Conversation {conversation_id}")));
    }
    Ok(())
}

pub async fn list(
    conn: &impl ConnectionTrait,
    conversation_id: i32,
) -> Result<Vec<SessionTimerInfo>, DbError> {
    let rows = conn
        .query_all(statement(
            "SELECT * FROM conversation_timer WHERE conversation_id = ? \
             ORDER BY created_at ASC, id ASC",
            vec![conversation_id.into()],
        ))
        .await?;
    rows.iter().map(parse_timer).collect()
}

/// Every enabled timer whose Session still exists. A soft-deleted Session
/// keeps its rows, but its timers must stop earning queue entries: the engine
/// scans this list for due candidates, so the liveness join is the bouncer.
pub async fn enabled(conn: &impl ConnectionTrait) -> Result<Vec<SessionTimerInfo>, DbError> {
    let rows = conn
        .query_all(statement(
            "SELECT t.id, t.conversation_id, t.idle_secs, t.prompt_text, t.enabled, \
                    t.last_fired_at, t.fire_count, t.strike_count, \
                    t.auto_paused_at, t.auto_pause_reason, t.created_at, t.updated_at \
             FROM conversation_timer t \
             JOIN conversation c ON c.id = t.conversation_id AND c.deleted_at IS NULL \
             WHERE t.enabled = 1 \
             ORDER BY t.conversation_id ASC, t.created_at ASC",
            vec![],
        ))
        .await?;
    rows.iter().map(parse_timer).collect()
}

pub async fn find(conn: &impl ConnectionTrait, id: &str) -> Result<SessionTimerInfo, DbError> {
    conn.query_one(statement(
        "SELECT * FROM conversation_timer WHERE id = ?",
        vec![id.into()],
    ))
    .await?
    .map(|row| parse_timer(&row))
    .transpose()?
    .ok_or_else(|| DbError::NotFound(format!("Session timer {id}")))
}

pub async fn create(
    conn: &impl ConnectionTrait,
    input: CreateSessionTimerInput,
) -> Result<SessionTimerInfo, DbError> {
    let prompt_text = validate_prompt(&input.prompt_text)?;
    let idle_grace_secs = validate_grace(input.idle_grace_secs)?;
    ensure_conversation(conn, input.conversation_id).await?;

    let id = input
        .client_dedupe_id
        .as_deref()
        .map(str::to_string)
        .unwrap_or_else(|| format!("timer-{}", uuid::Uuid::new_v4()));
    if id.trim().is_empty() || id.len() > 200 {
        return Err(validation("Timer id must contain between 1 and 200 bytes"));
    }
    if conn
        .query_one(statement(
            "SELECT id FROM conversation_timer WHERE id = ?",
            vec![id.clone().into()],
        ))
        .await?
        .is_some()
    {
        return find(conn, &id).await;
    }

    let now = Utc::now();
    // Keep the broad table shape compatible with early developer databases,
    // while the product exposes only the idle continuation behavior.
    conn.execute(statement(
        "INSERT INTO conversation_timer \
         (id, conversation_id, mode, idle_secs, prompt_text, enabled, repeat_idle, \
          fire_count, created_at, updated_at) \
         VALUES (?, ?, 'idle_for', ?, ?, 1, 1, 0, ?, ?)",
        vec![
            id.clone().into(),
            input.conversation_id.into(),
            idle_grace_secs.into(),
            prompt_text.into(),
            now.into(),
            now.into(),
        ],
    ))
    .await?;
    find(conn, &id).await
}

pub async fn update(
    conn: &impl ConnectionTrait,
    id: &str,
    input: UpdateSessionTimerInput,
) -> Result<SessionTimerInfo, DbError> {
    let existing = find(conn, id).await?;
    if existing.updated_at != input.expected_updated_at {
        return Err(DbError::Conflict(
            "This timer changed while you were editing it".into(),
        ));
    }
    let prompt_text = match input.prompt_text {
        Some(value) => validate_prompt(&value)?,
        None => existing.prompt_text,
    };
    let idle_grace_secs = match input.idle_grace_secs {
        Some(value) => validate_grace(value)?,
        None => existing.idle_grace_secs,
    };
    let enabled = input.enabled.unwrap_or(existing.enabled);
    let now = Utc::now();
    // Any deliberate edit is fresh intent from the user: the no-progress
    // strike streak and an automatic backoff pause never survive it.
    let result = conn
        .execute(statement(
            "UPDATE conversation_timer \
             SET prompt_text = ?, idle_secs = ?, enabled = ?, \
                 strike_count = 0, auto_paused_at = NULL, auto_pause_reason = NULL, \
                 updated_at = ? \
             WHERE id = ? AND updated_at = ?",
            vec![
                prompt_text.into(),
                idle_grace_secs.into(),
                (enabled as i32).into(),
                now.into(),
                id.into(),
                input.expected_updated_at.into(),
            ],
        ))
        .await?;
    if result.rows_affected() != 1 {
        return Err(DbError::Conflict(
            "This timer changed while you were editing it".into(),
        ));
    }
    find(conn, id).await
}

pub async fn delete(conn: &impl ConnectionTrait, id: &str) -> Result<(), DbError> {
    let result = conn
        .execute(statement(
            "DELETE FROM conversation_timer WHERE id = ?",
            vec![id.into()],
        ))
        .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound(format!("Session timer {id}")));
    }
    Ok(())
}

/// Atomically reserve one continuation occurrence. The timer remains enabled;
/// the engine closes its current idle window and the next TurnComplete re-arms it.
/// `strike_count` is the streak of reminders since the last `reset_delay`.
pub async fn claim_fire(
    conn: &impl ConnectionTrait,
    id: &str,
    previous_updated_at: DateTime<Utc>,
    strike_count: i32,
) -> Result<SessionTimerInfo, DbError> {
    let now = Utc::now();
    let result = conn
        .execute(statement(
            "UPDATE conversation_timer \
             SET last_fired_at = ?, fire_count = fire_count + 1, strike_count = ?, \
                 auto_paused_at = NULL, auto_pause_reason = NULL, updated_at = ? \
             WHERE id = ? AND updated_at = ? AND enabled = 1",
            vec![
                now.into(),
                strike_count.into(),
                now.into(),
                id.into(),
                previous_updated_at.into(),
            ],
        ))
        .await?;
    if result.rows_affected() != 1 {
        return Err(DbError::Conflict(format!(
            "Session timer {id} changed before its fire was claimed"
        )));
    }
    find(conn, id).await
}

/// Reset the reminder delay to `idle_grace`. Call this after real progress
/// on the continuation goal, or when new information unblocks the Session.
/// It does not fire the timer and does not change `enabled`.
pub async fn reset_delay(
    conn: &impl ConnectionTrait,
    id: &str,
    previous_updated_at: DateTime<Utc>,
) -> Result<SessionTimerInfo, DbError> {
    let now = Utc::now();
    let result = conn
        .execute(statement(
            "UPDATE conversation_timer \
             SET strike_count = 0, auto_paused_at = NULL, auto_pause_reason = NULL, \
                 updated_at = ? \
             WHERE id = ? AND updated_at = ?",
            vec![now.into(), id.into(), previous_updated_at.into()],
        ))
        .await?;
    if result.rows_affected() != 1 {
        return Err(DbError::Conflict(format!(
            "Session timer {id} changed before its delay could be reset"
        )));
    }
    find(conn, id).await
}

/// Park a timer that kept poking without any new information while its
/// Session is waiting on unanswered letters. `enabled` is deliberately left
/// untouched: this is backoff stretched to "until something new happens",
/// not a user decision. Real new mailbox information or any user edit
/// revives the timer.
pub async fn auto_pause(
    conn: &impl ConnectionTrait,
    id: &str,
    previous_updated_at: DateTime<Utc>,
    reason: &str,
) -> Result<SessionTimerInfo, DbError> {
    let now = Utc::now();
    let result = conn
        .execute(statement(
            "UPDATE conversation_timer \
             SET auto_paused_at = ?, auto_pause_reason = ?, updated_at = ? \
             WHERE id = ? AND updated_at = ? AND enabled = 1",
            vec![
                now.into(),
                reason.into(),
                now.into(),
                id.into(),
                previous_updated_at.into(),
            ],
        ))
        .await?;
    if result.rows_affected() != 1 {
        return Err(DbError::Conflict(format!(
            "Session timer {id} changed before it could auto-pause"
        )));
    }
    find(conn, id).await
}

pub fn fire_dedupe_id(timer_id: &str, occurrence: i32) -> String {
    format!("timer-fire-{timer_id}-{occurrence}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::AgentType;

    async fn setup() -> (crate::db::AppDatabase, i32) {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/timer-test").await;
        let conversation_id = seed_conversation(&db, folder_id, AgentType::Codex).await;
        (db, conversation_id)
    }

    fn input(conversation_id: i32, id: &str) -> CreateSessionTimerInput {
        CreateSessionTimerInput {
            conversation_id,
            prompt_text: "Continue the current objective".into(),
            idle_grace_secs: 2,
            client_dedupe_id: Some(id.into()),
        }
    }

    #[tokio::test]
    async fn create_update_pause_and_resume() {
        let (db, conversation_id) = setup().await;
        let timer = create(&db.conn, input(conversation_id, "timer-1"))
            .await
            .unwrap();
        assert!(timer.enabled);
        assert_eq!(timer.idle_grace_secs, 2);

        let timer = update(
            &db.conn,
            &timer.id,
            UpdateSessionTimerInput {
                prompt_text: Some("Read docs/current-task.md and continue".into()),
                enabled: Some(false),
                idle_grace_secs: None,
                expected_updated_at: timer.updated_at,
            },
        )
        .await
        .unwrap();
        assert!(!timer.enabled);
        assert!(timer.prompt_text.contains("current-task.md"));

        let timer = update(
            &db.conn,
            &timer.id,
            UpdateSessionTimerInput {
                prompt_text: None,
                enabled: Some(true),
                idle_grace_secs: None,
                expected_updated_at: timer.updated_at,
            },
        )
        .await
        .unwrap();
        assert!(timer.enabled);
    }

    #[tokio::test]
    async fn create_is_deduped_and_rejects_invalid_input() {
        let (db, conversation_id) = setup().await;
        let first = create(&db.conn, input(conversation_id, "same-id"))
            .await
            .unwrap();
        let second = create(&db.conn, input(conversation_id, "same-id"))
            .await
            .unwrap();
        assert_eq!(first.id, second.id);

        let mut invalid = input(conversation_id, "bad");
        invalid.prompt_text = " ".into();
        assert!(create(&db.conn, invalid).await.is_err());
    }

    #[tokio::test]
    async fn claim_is_single_writer_and_timer_stays_enabled() {
        let (db, conversation_id) = setup().await;
        let timer = create(&db.conn, input(conversation_id, "claim"))
            .await
            .unwrap();
        let claimed = claim_fire(&db.conn, &timer.id, timer.updated_at, 0)
            .await
            .unwrap();
        assert!(claimed.enabled);
        assert_eq!(claimed.fire_count, 1);
        assert!(claim_fire(&db.conn, &timer.id, timer.updated_at, 0)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn auto_pause_keeps_enabled_and_any_edit_clears_it() {
        let (db, conversation_id) = setup().await;
        let timer = create(&db.conn, input(conversation_id, "brake"))
            .await
            .unwrap();
        let parked = auto_pause(&db.conn, &timer.id, timer.updated_at, "waiting_no_progress")
            .await
            .unwrap();
        assert!(parked.enabled, "the brake never flips the user's switch");
        assert!(parked.auto_paused_at.is_some());

        let revived = update(
            &db.conn,
            &timer.id,
            UpdateSessionTimerInput {
                prompt_text: None,
                enabled: Some(true),
                idle_grace_secs: None,
                expected_updated_at: parked.updated_at,
            },
        )
        .await
        .unwrap();
        assert!(revived.auto_paused_at.is_none());
        assert_eq!(revived.strike_count, 0);
    }

    #[tokio::test]
    async fn enabled_skips_timers_whose_conversation_was_deleted() {
        let (db, conversation_id) = setup().await;
        create(&db.conn, input(conversation_id, "doomed"))
            .await
            .unwrap();
        assert_eq!(enabled(&db.conn).await.unwrap().len(), 1);

        db.conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE conversation SET deleted_at = CURRENT_TIMESTAMP WHERE id = ?",
                vec![conversation_id.into()],
            ))
            .await
            .unwrap();
        assert!(
            enabled(&db.conn).await.unwrap().is_empty(),
            "a soft-deleted Session's timer must stop earning queue entries"
        );
    }

    #[tokio::test]
    async fn reset_delay_clears_strike_without_toggling_enabled() {
        let (db, conversation_id) = setup().await;
        let timer = create(&db.conn, input(conversation_id, "reset"))
            .await
            .unwrap();
        let claimed = claim_fire(&db.conn, &timer.id, timer.updated_at, 3)
            .await
            .unwrap();
        assert_eq!(claimed.strike_count, 3);
        let reset = reset_delay(&db.conn, &claimed.id, claimed.updated_at)
            .await
            .unwrap();
        assert!(reset.enabled);
        assert_eq!(reset.strike_count, 0);
    }
}
