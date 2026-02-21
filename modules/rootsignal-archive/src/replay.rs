use sqlx::PgPool;
use uuid::Uuid;

use rootsignal_common::SocialPlatform;

use crate::archive::{Content, FetchResponse};
use crate::error::{ArchiveError, Result};
use crate::store::{ArchiveStore, StoredInteraction};

/// Replays archived content from Postgres. No network access.
/// Drop-in replacement for Archive during testing and extraction iteration.
pub struct Replay {
    store: ArchiveStore,
    run_id: Option<Uuid>,
}

impl Replay {
    /// Replay content from a specific run.
    pub fn for_run(pool: PgPool, run_id: Uuid) -> Self {
        Self {
            store: ArchiveStore::new(pool),
            run_id: Some(run_id),
        }
    }

    /// Replay the most recent content for each target.
    pub fn latest(pool: PgPool) -> Self {
        Self {
            store: ArchiveStore::new(pool),
            run_id: None,
        }
    }

    /// Same signature as Archive::fetch. Reads from Postgres only.
    pub async fn fetch(&self, target: &str) -> Result<FetchResponse> {
        let target = target.trim();

        let interaction = if let Some(run_id) = self.run_id {
            self.store.by_run_and_target(run_id, target).await?
        } else {
            self.store.latest_by_target(target).await?
        };

        let interaction = interaction.ok_or_else(|| ArchiveError::NotFound(target.to_string()))?;

        // If the original fetch failed, reproduce the error
        if let Some(ref err) = interaction.error {
            return Err(ArchiveError::FetchFailed(err.clone()));
        }

        interaction_to_response(interaction)
    }

    /// Same signature as Archive::search_social. Reads from Postgres only.
    pub async fn search_social(
        &self,
        platform: &SocialPlatform,
        _topics: &[&str],
        _limit: u32,
    ) -> Result<FetchResponse> {
        let platform_name = platform_str(platform);

        let interaction = if let Some(run_id) = self.run_id {
            self.store.social_topics_by_run(run_id, platform_name).await?
        } else {
            // For latest mode, construct the target key and look up by target
            let target = format!("social_topics:{}", platform_name);
            self.store.latest_by_target(&target).await?
        };

        let interaction = interaction.ok_or_else(|| {
            ArchiveError::NotFound(format!("social_topics:{}", platform_name))
        })?;

        if let Some(ref err) = interaction.error {
            return Err(ArchiveError::FetchFailed(err.clone()));
        }

        interaction_to_response(interaction)
    }
}

/// Reconstruct a FetchResponse from a stored interaction.
fn interaction_to_response(i: StoredInteraction) -> Result<FetchResponse> {
    let content = match i.kind.as_str() {
        "page" => {
            Content::Page(rootsignal_common::ScrapedPage {
                url: i.target.clone(),
                raw_html: i.raw_html.unwrap_or_default(),
                markdown: i.markdown.unwrap_or_default(),
                content_hash: i.content_hash.clone(),
            })
        }
        "feed" => {
            let items: Vec<rootsignal_common::FeedItem> = i
                .response_json
                .map(|j| serde_json::from_value(j).unwrap_or_default())
                .unwrap_or_default();
            Content::Feed(items)
        }
        "search" => {
            let results: Vec<rootsignal_common::SearchResult> = i
                .response_json
                .map(|j| serde_json::from_value(j).unwrap_or_default())
                .unwrap_or_default();
            Content::SearchResults(results)
        }
        "social" => {
            let posts: Vec<rootsignal_common::SocialPost> = i
                .response_json
                .map(|j| serde_json::from_value(j).unwrap_or_default())
                .unwrap_or_default();
            Content::SocialPosts(posts)
        }
        "pdf" => {
            Content::Pdf(rootsignal_common::PdfContent {
                extracted_text: String::new(), // PDF extraction not yet implemented
            })
        }
        _ => {
            let body = i.raw_html.unwrap_or_default();
            Content::Raw(body)
        }
    };

    Ok(FetchResponse {
        target: i.target_raw,
        content,
        content_hash: i.content_hash,
        fetched_at: i.fetched_at,
        duration_ms: i.duration_ms as u32,
    })
}

fn platform_str(platform: &SocialPlatform) -> &'static str {
    match platform {
        SocialPlatform::Instagram => "instagram",
        SocialPlatform::Facebook => "facebook",
        SocialPlatform::Reddit => "reddit",
        SocialPlatform::Twitter => "twitter",
        SocialPlatform::TikTok => "tiktok",
        SocialPlatform::Bluesky => "bluesky",
    }
}
