use sqlx::PgPool;
use uuid::Uuid;

use rootsignal_common::SocialPlatform;

use crate::archive::FetchResponse;
use crate::error::Result;

/// Replays archived content from Postgres. No network access.
/// Drop-in replacement for Archive during testing and extraction iteration.
pub struct Replay {
    #[allow(dead_code)]
    pool: PgPool,
    #[allow(dead_code)]
    run_id: Option<Uuid>,
}

impl Replay {
    /// Replay content from a specific run.
    pub fn for_run(pool: PgPool, run_id: Uuid) -> Self {
        Self {
            pool,
            run_id: Some(run_id),
        }
    }

    /// Replay the most recent content for each target.
    pub fn latest(pool: PgPool) -> Self {
        Self {
            pool,
            run_id: None,
        }
    }

    /// Same signature as Archive::fetch. Reads from Postgres only.
    pub async fn fetch(&self, _target: &str) -> Result<FetchResponse> {
        todo!("Phase 5: implement replay fetch")
    }

    /// Same signature as Archive::search_social. Reads from Postgres only.
    pub async fn search_social(
        &self,
        _platform: &SocialPlatform,
        _topics: &[&str],
        _limit: u32,
    ) -> Result<FetchResponse> {
        todo!("Phase 5: implement replay search_social")
    }
}
