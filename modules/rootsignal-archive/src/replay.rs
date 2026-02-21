use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use rootsignal_common::ContentSemantics;

use crate::archive::{Content, FetchBackend, FetchedContent, FetchRequest};
use crate::error::{ArchiveError, Result};
use crate::semantics;
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

    /// Same signature as Archive::fetch. Returns a handle — no work until a terminal method is called.
    pub fn fetch(&self, target: &str) -> FetchRequest<'_> {
        FetchRequest::new(self, target)
    }

    /// Look up a stored interaction by target.
    async fn lookup(&self, target: &str) -> Result<StoredInteraction> {
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

        Ok(interaction)
    }
}

#[async_trait]
impl FetchBackend for Replay {
    async fn fetch_content(&self, target: &str) -> Result<FetchedContent> {
        let target = target.trim();
        let interaction = self.lookup(target).await?;
        let target_raw = interaction.target_raw.clone();
        let content_hash = interaction.content_hash.clone();
        let fetched_at = interaction.fetched_at;
        let duration_ms = interaction.duration_ms as u32;

        let content = interaction_to_content(interaction);
        let text = semantics::extractable_text(&content, &target_raw);

        Ok(FetchedContent {
            target: target_raw,
            content,
            content_hash,
            fetched_at,
            duration_ms,
            text,
        })
    }

    async fn resolve_semantics(&self, content: &FetchedContent) -> Result<ContentSemantics> {
        // Look up cached semantics from the store by content_hash
        let cached = self
            .store
            .semantics_by_content_hash(&content.content_hash)
            .await?;

        match cached {
            Some(json) => serde_json::from_value::<ContentSemantics>(json)
                .map_err(|e| ArchiveError::FetchFailed(format!("Invalid cached semantics: {e}"))),
            None => Err(ArchiveError::NotFound(format!(
                "No cached semantics for content_hash={}",
                content.content_hash
            ))),
        }
    }
}

/// Reconstruct Content from a stored interaction.
fn interaction_to_content(i: StoredInteraction) -> Content {
    match i.kind.as_str() {
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
            region_slug: "minneapolis".to_string(),
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
            semantics: None,
        }
    }

    #[test]
    fn reconstruct_page() {
        let mut i = make_interaction("page");
        i.raw_html = Some("<h1>Hello</h1>".to_string());
        i.markdown = Some("# Hello".to_string());

        let content = interaction_to_content(i);
        match content {
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

        let content = interaction_to_content(i);
        match content {
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

        let content = interaction_to_content(i);
        match content {
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

        let content = interaction_to_content(i);
        match content {
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

        let content = interaction_to_content(i);
        match content {
            Content::Raw(text) => assert_eq!(text, "raw body text"),
            other => panic!("Expected Content::Raw, got {:?}", other),
        }
    }

    #[test]
    fn missing_json_returns_empty_collections() {
        // Search with no response_json should return empty vec, not error
        let i = make_interaction("search");
        let content = interaction_to_content(i);
        match content {
            Content::SearchResults(r) => assert!(r.is_empty()),
            other => panic!("Expected empty Content::SearchResults, got {:?}", other),
        }
    }
}
