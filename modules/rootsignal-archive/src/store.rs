/// Postgres persistence for web interactions. Internal to the archive crate.
#[allow(dead_code)]
pub(crate) struct ArchiveStore {
    pool: sqlx::PgPool,
}

#[allow(dead_code)]
impl ArchiveStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

// Store methods will be implemented in Phase 4.
