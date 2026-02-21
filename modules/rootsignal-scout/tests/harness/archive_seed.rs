//! Bridge between SimulatedWeb's RunLog and the archive Seeder.
//! Converts each LogEntry into a Content value and inserts it.

use anyhow::Result;

use rootsignal_archive::{Content, Seeder};
use rootsignal_common::{ScrapedPage, SearchResult, SocialPost};
use simweb::{SimulatedWeb, snapshot::LogEntry};

/// Seed archive Postgres from a SimulatedWeb's logged interactions.
pub async fn seed_from_sim(seeder: &Seeder, sim: &SimulatedWeb) -> Result<()> {
    let log = sim.run_log().await;

    for entry in &log.entries {
        match entry {
            LogEntry::Search { query, results, .. } => {
                let common: Vec<SearchResult> = results
                    .iter()
                    .map(|r| SearchResult {
                        url: r.url.clone(),
                        title: r.title.clone(),
                        snippet: r.snippet.clone(),
                    })
                    .collect();
                seeder
                    .insert(query, Content::SearchResults(common))
                    .await?;
            }
            LogEntry::Scrape { url, page, .. } => {
                let markdown = page.content.clone();
                let raw_html = page
                    .raw_html
                    .clone()
                    .unwrap_or_else(|| format!("<html><body>{}</body></html>", page.content));
                let content_hash =
                    rootsignal_common::content_hash(&markdown).to_string();
                seeder
                    .insert(
                        url,
                        Content::Page(ScrapedPage {
                            url: url.clone(),
                            raw_html,
                            markdown,
                            content_hash,
                        }),
                    )
                    .await?;
            }
            LogEntry::Social {
                platform,
                identifier,
                posts,
                ..
            } => {
                let common: Vec<SocialPost> = posts
                    .iter()
                    .map(|p| SocialPost {
                        content: p.content.clone(),
                        author: p.author.clone(),
                        url: p.url.clone(),
                    })
                    .collect();
                let target = format!("{}:{}", platform.to_lowercase(), identifier);
                seeder
                    .insert(&target, Content::SocialPosts(common))
                    .await?;
            }
            LogEntry::Hashtags {
                hashtags, posts, ..
            } => {
                let common: Vec<SocialPost> = posts
                    .iter()
                    .map(|p| SocialPost {
                        content: p.content.clone(),
                        author: p.author.clone(),
                        url: p.url.clone(),
                    })
                    .collect();
                let target = format!("hashtags:{}", hashtags.join(","));
                seeder
                    .insert(&target, Content::SocialPosts(common))
                    .await?;
            }
        }
    }

    Ok(())
}
