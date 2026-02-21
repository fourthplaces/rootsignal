// Apify social media fetcher (Instagram, Facebook, Reddit, Twitter, TikTok).
// Moved from scout::pipeline::scraper in Phase 3.

use anyhow::Result;
use apify_client::ApifyClient;
use rootsignal_common::{SocialPlatform, SocialPost};
use tracing::info;

pub(crate) struct SocialFetcher {
    client: ApifyClient,
}

impl SocialFetcher {
    pub(crate) fn new(client: ApifyClient) -> Self {
        Self { client }
    }

    /// Fetch posts from a social platform profile/account.
    pub(crate) async fn fetch_posts(
        &self,
        platform: &SocialPlatform,
        identifier: &str,
        limit: u32,
    ) -> Result<Vec<SocialPost>> {
        info!(?platform, identifier, limit, "Fetching social posts");

        match platform {
            SocialPlatform::Instagram => {
                let posts = self.client.scrape_instagram_posts(identifier, limit).await?;
                Ok(posts
                    .into_iter()
                    .filter_map(|p| {
                        let content = p.caption?;
                        if content.is_empty() {
                            return None;
                        }
                        Some(SocialPost {
                            content,
                            author: p.owner_username,
                            url: Some(p.url),
                        })
                    })
                    .collect())
            }
            SocialPlatform::Facebook => {
                let posts = self.client.scrape_facebook_posts(identifier, limit).await?;
                Ok(posts
                    .into_iter()
                    .filter_map(|p| {
                        let content = p.text?;
                        if content.is_empty() {
                            return None;
                        }
                        Some(SocialPost {
                            content,
                            author: p.page_name,
                            url: p.url,
                        })
                    })
                    .collect())
            }
            SocialPlatform::Reddit => {
                let posts = self.client.scrape_reddit_posts(identifier, limit).await?;
                Ok(posts
                    .into_iter()
                    .filter_map(|p| {
                        if p.data_type.as_deref() != Some("post") {
                            return None;
                        }
                        let title = p.title.unwrap_or_default();
                        let body = p.body.unwrap_or_default();
                        let content = format!("{}\n\n{}", title, body).trim().to_string();
                        if content.is_empty() {
                            return None;
                        }
                        Some(SocialPost {
                            content,
                            author: None,
                            url: p.url,
                        })
                    })
                    .collect())
            }
            SocialPlatform::Twitter => {
                let tweets = self.client.scrape_x_posts(identifier, limit).await?;
                Ok(tweets
                    .into_iter()
                    .filter_map(|t| {
                        let content = t.content()?.to_string();
                        if content.is_empty() {
                            return None;
                        }
                        Some(SocialPost {
                            content,
                            author: t.author.as_ref().and_then(|a| a.user_name.clone()),
                            url: t.url,
                        })
                    })
                    .collect())
            }
            SocialPlatform::TikTok => {
                let posts = self.client.scrape_tiktok_posts(identifier, limit).await?;
                Ok(posts
                    .into_iter()
                    .filter_map(|p| {
                        let content = p.text?;
                        if content.len() < 20 {
                            return None;
                        }
                        Some(SocialPost {
                            content,
                            author: p.author_meta.as_ref().and_then(|a| a.name.clone()),
                            url: p.web_video_url,
                        })
                    })
                    .collect())
            }
            SocialPlatform::Bluesky => {
                anyhow::bail!("Bluesky is not yet supported by the social fetcher")
            }
        }
    }

    /// Search a platform for topics (hashtags/keywords).
    pub(crate) async fn search_topics(
        &self,
        platform: &SocialPlatform,
        topics: &[&str],
        limit: u32,
    ) -> Result<Vec<SocialPost>> {
        info!(?platform, ?topics, limit, "Searching social topics");

        match platform {
            SocialPlatform::Instagram => {
                let sanitized = sanitize_topics_to_hashtags(topics);
                let refs: Vec<&str> = sanitized.iter().map(|s| s.as_str()).collect();
                let posts = self.client.search_instagram_hashtags(&refs, limit).await?;
                Ok(posts
                    .into_iter()
                    .map(|p| SocialPost {
                        content: p.content,
                        author: Some(p.author_username),
                        url: Some(p.post_url),
                    })
                    .collect())
            }
            SocialPlatform::Twitter => {
                let posts = self.client.search_x_keywords(topics, limit).await?;
                Ok(posts
                    .into_iter()
                    .filter_map(|t| {
                        let content = t.content()?.to_string();
                        if content.is_empty() {
                            return None;
                        }
                        Some(SocialPost {
                            content,
                            author: t.author.as_ref().and_then(|a| a.user_name.clone()),
                            url: t.url,
                        })
                    })
                    .collect())
            }
            SocialPlatform::TikTok => {
                let posts = self.client.search_tiktok_keywords(topics, limit).await?;
                Ok(posts
                    .into_iter()
                    .filter_map(|p| {
                        let content = p.text?;
                        if content.len() < 20 {
                            return None;
                        }
                        Some(SocialPost {
                            content,
                            author: p.author_meta.as_ref().and_then(|a| a.name.clone()),
                            url: p.web_video_url,
                        })
                    })
                    .collect())
            }
            SocialPlatform::Reddit => {
                let posts = self.client.search_reddit_keywords(topics, limit).await?;
                Ok(posts
                    .into_iter()
                    .filter_map(|p| {
                        if p.data_type.as_deref() != Some("post") {
                            return None;
                        }
                        let title = p.title.unwrap_or_default();
                        let body = p.body.unwrap_or_default();
                        let content = format!("{}\n\n{}", title, body).trim().to_string();
                        if content.is_empty() {
                            return None;
                        }
                        Some(SocialPost {
                            content,
                            author: p.url.as_deref().and_then(extract_reddit_username),
                            url: p.url,
                        })
                    })
                    .collect())
            }
            // Facebook and Bluesky don't support keyword search
            _ => Ok(Vec::new()),
        }
    }
}

/// Extract a Reddit username from a URL like "https://www.reddit.com/user/NAME/..."
fn extract_reddit_username(url: &str) -> Option<String> {
    let parts: Vec<&str> = url.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        if (*part == "user" || *part == "u") && i + 1 < parts.len() {
            let name = parts[i + 1];
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Convert multi-word topic strings into valid Instagram hashtags (camelCase,
/// alphanumeric only). The Instagram hashtag API rejects values containing
/// spaces, punctuation, or other special characters.
pub(crate) fn sanitize_topics_to_hashtags(topics: &[&str]) -> Vec<String> {
    topics
        .iter()
        .map(|t| {
            t.split_whitespace()
                .enumerate()
                .map(|(i, w)| {
                    let w: String = w.chars().filter(|c| c.is_alphanumeric()).collect();
                    if i == 0 {
                        w.to_lowercase()
                    } else {
                        let mut chars = w.chars();
                        match chars.next() {
                            Some(first) => {
                                first.to_uppercase().to_string()
                                    + &chars.as_str().to_lowercase()
                            }
                            None => String::new(),
                        }
                    }
                })
                .collect::<String>()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_multi_word_topics() {
        let topics = &[
            "Minneapolis immigration legal aid volunteer Minnesota",
            "Minnesota teacher sanctuary movement",
        ];
        let result = sanitize_topics_to_hashtags(topics);
        assert_eq!(
            result,
            vec![
                "minneapolisImmigrationLegalAidVolunteerMinnesota",
                "minnesotaTeacherSanctuaryMovement",
            ]
        );
    }

    #[test]
    fn sanitize_single_word_topic() {
        let result = sanitize_topics_to_hashtags(&["MNimmigration"]);
        assert_eq!(result, vec!["mnimmigration"]);
    }

    #[test]
    fn sanitize_strips_special_chars() {
        let result = sanitize_topics_to_hashtags(&["Minneapolis: ICE raids — 2026!"]);
        assert_eq!(result, vec!["minneapolisIceRaids2026"]);
    }

    #[test]
    fn sanitize_filters_empty() {
        let result = sanitize_topics_to_hashtags(&["", "   ", "valid topic"]);
        assert_eq!(result, vec!["validTopic"]);
    }

    #[test]
    fn extract_reddit_user_from_url() {
        assert_eq!(
            extract_reddit_username("https://www.reddit.com/user/someuser/comments/abc"),
            Some("someuser".to_string())
        );
    }

    #[test]
    fn extract_reddit_user_short_form() {
        assert_eq!(
            extract_reddit_username("https://reddit.com/u/testuser"),
            Some("testuser".to_string())
        );
    }
}
