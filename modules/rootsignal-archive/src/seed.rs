// Seeder: write-only interface for inserting Content into archive Postgres.
// Read path: use Replay::for_run(pool, run_id).

use sqlx::PgPool;
use uuid::Uuid;

use crate::archive::Content;
use crate::error::Result;
use crate::store::{ArchiveStore, InsertInteraction};

/// Write-only interface for seeding archive data into Postgres.
/// Paired with `Replay::for_run(pool, run_id)` for the read path.
pub struct Seeder {
    store: ArchiveStore,
    run_id: Uuid,
    region_slug: String,
}

impl Seeder {
    /// Create a new Seeder. Runs migrations to ensure the table exists.
    pub async fn new(pool: PgPool, run_id: Uuid, region_slug: &str) -> Result<Self> {
        let store = ArchiveStore::new(pool);
        store.migrate().await?;
        Ok(Self {
            store,
            run_id,
            region_slug: region_slug.to_string(),
        })
    }

    /// Insert a Content value into the archive, keyed by target.
    pub async fn insert(&self, target: &str, content: Content) -> Result<()> {
        let interaction = match content {
            Content::Page(page) => InsertInteraction {
                run_id: self.run_id,
                region_slug: self.region_slug.clone(),
                kind: "page".to_string(),
                target: target.to_string(),
                target_raw: target.to_string(),
                fetcher: "seeder".to_string(),
                raw_html: Some(page.raw_html),
                markdown: Some(page.markdown),
                response_json: None,
                raw_bytes: None,
                content_hash: page.content_hash,
                duration_ms: 0,
                error: None,
                metadata: None,
                semantics: None,
            },
            Content::SearchResults(results) => {
                let json = serde_json::to_value(&results).unwrap_or_default();
                let hash = rootsignal_common::content_hash(&json.to_string()).to_string();
                InsertInteraction {
                    run_id: self.run_id,
                    region_slug: self.region_slug.clone(),
                    kind: "search".to_string(),
                    target: target.to_string(),
                    target_raw: target.to_string(),
                    fetcher: "seeder".to_string(),
                    raw_html: None,
                    markdown: None,
                    response_json: Some(json),
                    raw_bytes: None,
                    content_hash: hash,
                    duration_ms: 0,
                    error: None,
                    metadata: None,
                    semantics: None,
                }
            }
            Content::SocialPosts(posts) => {
                let json = serde_json::to_value(&posts).unwrap_or_default();
                let hash = rootsignal_common::content_hash(&json.to_string()).to_string();
                InsertInteraction {
                    run_id: self.run_id,
                    region_slug: self.region_slug.clone(),
                    kind: "social".to_string(),
                    target: target.to_string(),
                    target_raw: target.to_string(),
                    fetcher: "seeder".to_string(),
                    raw_html: None,
                    markdown: None,
                    response_json: Some(json),
                    raw_bytes: None,
                    content_hash: hash,
                    duration_ms: 0,
                    error: None,
                    metadata: None,
                    semantics: None,
                }
            }
            Content::Feed(items) => {
                let json = serde_json::to_value(&items).unwrap_or_default();
                let hash = rootsignal_common::content_hash(&json.to_string()).to_string();
                InsertInteraction {
                    run_id: self.run_id,
                    region_slug: self.region_slug.clone(),
                    kind: "feed".to_string(),
                    target: target.to_string(),
                    target_raw: target.to_string(),
                    fetcher: "seeder".to_string(),
                    raw_html: None,
                    markdown: None,
                    response_json: Some(json),
                    raw_bytes: None,
                    content_hash: hash,
                    duration_ms: 0,
                    error: None,
                    metadata: None,
                    semantics: None,
                }
            }
            Content::Raw(body) => {
                let hash = rootsignal_common::content_hash(&body).to_string();
                InsertInteraction {
                    run_id: self.run_id,
                    region_slug: self.region_slug.clone(),
                    kind: "raw".to_string(),
                    target: target.to_string(),
                    target_raw: target.to_string(),
                    fetcher: "seeder".to_string(),
                    raw_html: Some(body),
                    markdown: None,
                    response_json: None,
                    raw_bytes: None,
                    content_hash: hash,
                    duration_ms: 0,
                    error: None,
                    metadata: None,
                    semantics: None,
                }
            }
            Content::Pdf(_) => {
                // PDF seeding not yet needed; store as empty raw for now
                InsertInteraction {
                    run_id: self.run_id,
                    region_slug: self.region_slug.clone(),
                    kind: "pdf".to_string(),
                    target: target.to_string(),
                    target_raw: target.to_string(),
                    fetcher: "seeder".to_string(),
                    raw_html: None,
                    markdown: None,
                    response_json: None,
                    raw_bytes: None,
                    content_hash: String::new(),
                    duration_ms: 0,
                    error: None,
                    metadata: None,
                    semantics: None,
                }
            }
        };

        self.store.insert(interaction).await;
        Ok(())
    }

    /// The run_id this seeder writes to.
    pub fn run_id(&self) -> Uuid {
        self.run_id
    }
}
