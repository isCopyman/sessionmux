use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Database(#[from] sea_orm::DbErr),
    #[error("migration error: {0}")]
    Migration(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("validation error: {0}")]
    Validation(String),
    /// A compare-and-swap guard lost the race (stale revision, or another
    /// process claimed the same transition first). Retry by re-reading.
    #[error("conflict: {0}")]
    Conflict(String),
    /// The `(external_id, agent_type)` a bind tried to write is already held
    /// by a different row (live or soft-deleted — the unique index does not
    /// exempt deleted rows). Unlike [`DbError::Conflict`], retrying never
    /// helps: the holder does not change on its own, so the caller must
    /// surface this as a permanent failure rather than back off and re-read.
    /// Deliberately named without "conflict" so it can never be mistaken for
    /// the CAS variant above and retried the same way.
    #[error("external id already bound to a different conversation: {0}")]
    ExternalIdTaken(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl Serialize for DbError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
