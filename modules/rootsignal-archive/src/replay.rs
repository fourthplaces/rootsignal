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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_interaction(kind: &str) -> StoredInteraction {
        StoredInteraction {
            id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            city_slug: "minneapolis".to_string(),
            kind: kind.to_string(),
            target: "https://example.com".to_string(),
            target_raw: "https://example.com".to_string(),
            fetcher: "test".to_string(),
            raw_html: None,
            markdown: None,
            response_json: None,
            raw_bytes: None,
            content_hash: "abc123".to_string(),
            fetched_at: Utc::now(),
            duration_ms: 100,
            error: None,
            metadata: None,
        }
    }

    #[test]
    fn reconstruct_page() {
        let mut i = make_interaction("page");
        i.raw_html = Some("<h1>Hello</h1>".to_string());
        i.markdown = Some("# Hello".to_string());

        let resp = interaction_to_response(i).unwrap();
        match resp.content {
            Content::Page(page) => {
                assert_eq!(page.raw_html, "<h1>Hello</h1>");
                assert_eq!(page.markdown, "# Hello");
            }
            other => panic!("Expected Content::Page, got {:?}", other),
        }
    }

    #[test]
    fn reconstruct_search_results() {
        let results = vec![rootsignal_common::SearchResult {
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            snippet: "A snippet".to_string(),
        }];
        let mut i = make_interaction("search");
        i.response_json = Some(serde_json::to_value(&results).unwrap());

        let resp = interaction_to_response(i).unwrap();
        match resp.content {
            Content::SearchResults(r) => {
                assert_eq!(r.len(), 1);
                assert_eq!(r[0].title, "Example");
            }
            other => panic!("Expected Content::SearchResults, got {:?}", other),
        }
    }

    #[test]
    fn reconstruct_social_posts() {
        let posts = vec![rootsignal_common::SocialPost {
            content: "Hello from insta".to_string(),
            author: Some("@user".to_string()),
            url: Some("https://instagram.com/p/123".to_string()),
        }];
        let mut i = make_interaction("social");
        i.response_json = Some(serde_json::to_value(&posts).unwrap());

        let resp = interaction_to_response(i).unwrap();
        match resp.content {
            Content::SocialPosts(p) => {
                assert_eq!(p.len(), 1);
                assert_eq!(p[0].content, "Hello from insta");
            }
            other => panic!("Expected Content::SocialPosts, got {:?}", other),
        }
    }

    #[test]
    fn reconstruct_feed() {
        let items = vec![rootsignal_common::FeedItem {
            url: "https://example.com/article".to_string(),
            title: Some("Article Title".to_string()),
            pub_date: None,
        }];
        let mut i = make_interaction("feed");
        i.response_json = Some(serde_json::to_value(&items).unwrap());

        let resp = interaction_to_response(i).unwrap();
        match resp.content {
            Content::Feed(f) => {
                assert_eq!(f.len(), 1);
                assert_eq!(f[0].url, "https://example.com/article");
            }
            other => panic!("Expected Content::Feed, got {:?}", other),
        }
    }

    #[test]
    fn reconstruct_raw_for_unknown_kind() {
        let mut i = make_interaction("something_new");
        i.raw_html = Some("raw body text".to_string());

        let resp = interaction_to_response(i).unwrap();
        match resp.content {
            Content::Raw(text) => assert_eq!(text, "raw body text"),
            other => panic!("Expected Content::Raw, got {:?}", other),
        }
    }

    #[test]
    fn error_interaction_preserved() {
        let mut i = make_interaction("page");
        i.error = Some("connection timed out".to_string());

        // interaction_to_response doesn't check error — that's Replay::fetch's job.
        // Just verify it doesn't panic.
        let resp = interaction_to_response(i).unwrap();
        assert!(matches!(resp.content, Content::Page(_)));
    }

    #[test]
    fn missing_json_returns_empty_collections() {
        // Search with no response_json should return empty vec, not error
        let i = make_interaction("search");
        let resp = interaction_to_response(i).unwrap();
        match resp.content {
            Content::SearchResults(r) => assert!(r.is_empty()),
            other => panic!("Expected empty Content::SearchResults, got {:?}", other),
        }
    }

    #[test]
    fn content_hash_preserved() {
        let i = make_interaction("page");
        let resp = interaction_to_response(i).unwrap();
        assert_eq!(resp.content_hash, "abc123");
    }
}
