// Target detection: URL pattern matching and content-type routing.

use rootsignal_common::SocialPlatform;

/// Detected platform for a URL. Used by SourceHandle to route to the right service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Instagram,
    Twitter,
    Reddit,
    Facebook,
    TikTok,
    Bluesky,
    Web,
}

/// Detect which platform a URL belongs to.
pub fn detect_platform(url: &str) -> Platform {
    let lower = url.to_lowercase();
    if lower.contains("instagram.com") {
        Platform::Instagram
    } else if lower.contains("twitter.com") || lower.contains("x.com/") {
        Platform::Twitter
    } else if lower.contains("reddit.com") {
        Platform::Reddit
    } else if lower.contains("facebook.com") {
        Platform::Facebook
    } else if lower.contains("tiktok.com") {
        Platform::TikTok
    } else if lower.contains("bsky.app") {
        Platform::Bluesky
    } else {
        Platform::Web
    }
}

/// Normalize a URL for use as a source identity.
/// Strips protocol, www., trailing slashes. Lowercases host.
/// twitter.com and x.com are aliased to x.com.
pub fn normalize_url(url: &str) -> String {
    let mut s = url.trim().to_string();

    // Strip protocol
    if let Some(rest) = s.strip_prefix("https://") {
        s = rest.to_string();
    } else if let Some(rest) = s.strip_prefix("http://") {
        s = rest.to_string();
    }

    // Strip www.
    if let Some(rest) = s.strip_prefix("www.") {
        s = rest.to_string();
    }

    // Strip trailing slash
    while s.ends_with('/') {
        s.pop();
    }

    // Alias twitter.com → x.com
    if s.starts_with("twitter.com") {
        s = s.replacen("twitter.com", "x.com", 1);
    }

    s
}

/// Extract the identifier (username, subreddit, etc.) from a normalized URL.
pub fn extract_identifier(url: &str, platform: Platform) -> String {
    match platform {
        Platform::Reddit => {
            // Handle /r/Name or /user/Name
            if let Some(idx) = url.find("/r/") {
                let rest = &url[idx + 3..];
                return rest.split('/').next().unwrap_or(rest).to_string();
            }
            if let Some(idx) = url.find("/user/") {
                let rest = &url[idx + 6..];
                return rest.split('/').next().unwrap_or(rest).to_string();
            }
            extract_last_path_segment(url)
        }
        _ => extract_last_path_segment(url),
    }
}

fn extract_last_path_segment(url: &str) -> String {
    // Take everything after the host part
    if let Some(slash_idx) = url.find('/') {
        let path = &url[slash_idx + 1..];
        // Get first meaningful path segment
        let segment = path.split('/').find(|s| !s.is_empty()).unwrap_or(path);
        // Strip query params
        let segment = segment.split('?').next().unwrap_or(segment);
        segment.to_string()
    } else {
        url.to_string()
    }
}

/// Default post limit for social search when not specified in URL.
const DEFAULT_SOCIAL_SEARCH_LIMIT: u32 = 20;

/// What kind of target is this? Determined from the target string alone (no HTTP).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetKind {
    /// Plain text query (not a URL) → web search via Serper
    WebQuery(String),
    /// Social platform profile or feed URL → Apify
    Social {
        platform: SocialPlatform,
        identifier: String,
    },
    /// Social platform topic/hashtag search → Apify
    SocialSearch {
        platform: SocialPlatform,
        topics: Vec<String>,
        limit: u32,
    },
    /// HTTP(S) URL — needs content-type detection after fetch
    Url(String),
}

/// Content kind determined from HTTP response headers and/or body sniffing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ContentKind {
    Html,
    Feed,
    Pdf,
    Raw,
}

/// Detect what kind of target this is from the string alone.
pub fn detect_target(target: &str) -> TargetKind {
    let trimmed = target.trim();

    // Bare subreddit reference: "r/Minneapolis"
    if let Some(sub) = trimmed.strip_prefix("r/") {
        let sub = sub.trim_end_matches('/');
        if !sub.is_empty() && !sub.contains(' ') {
            return TargetKind::Social {
                platform: SocialPlatform::Reddit,
                identifier: sub.to_string(),
            };
        }
    }

    // Not a URL → web search query
    if rootsignal_common::is_web_query(trimmed) {
        return TargetKind::WebQuery(trimmed.to_string());
    }

    // Try to match social platform URL patterns
    let lower = trimmed.to_lowercase();
    if let Some(social) = detect_social_url(&lower, trimmed) {
        return social;
    }

    // Generic URL — content-type determined later via HTTP
    TargetKind::Url(trimmed.to_string())
}

/// Detect social platform from a URL. Returns None if not a social URL.
/// Distinguishes profile URLs from search/explore URLs.
fn detect_social_url(lower: &str, original: &str) -> Option<TargetKind> {
    // Parse URL for query params
    let parsed = url::Url::parse(original).ok();
    let limit = parsed
        .as_ref()
        .and_then(|u| {
            u.query_pairs()
                .find(|(k, _)| k == "limit")
                .and_then(|(_, v)| v.parse::<u32>().ok())
        })
        .unwrap_or(DEFAULT_SOCIAL_SEARCH_LIMIT);

    // Instagram
    if lower.contains("instagram.com") {
        // /explore/tags/X → social search
        if lower.contains("/explore/tags/") {
            let topics = extract_after_segment(original, "/explore/tags/");
            if !topics.is_empty() {
                return Some(TargetKind::SocialSearch {
                    platform: SocialPlatform::Instagram,
                    topics,
                    limit,
                });
            }
        }
        let identifier = extract_path_identifier(original);
        return Some(TargetKind::Social {
            platform: SocialPlatform::Instagram,
            identifier,
        });
    }

    // Reddit
    if lower.contains("reddit.com") {
        // /search?q=X → social search
        if lower.contains("/search") {
            if let Some(ref u) = parsed {
                let topics = extract_query_topics(u);
                if !topics.is_empty() {
                    return Some(TargetKind::SocialSearch {
                        platform: SocialPlatform::Reddit,
                        topics,
                        limit,
                    });
                }
            }
        }
        let identifier = extract_reddit_identifier(original);
        return Some(TargetKind::Social {
            platform: SocialPlatform::Reddit,
            identifier,
        });
    }

    // Twitter / X
    if lower.contains("twitter.com") || lower.contains("x.com/") {
        // /search?q=X → social search
        if lower.contains("/search") {
            if let Some(ref u) = parsed {
                let topics = extract_query_topics(u);
                if !topics.is_empty() {
                    return Some(TargetKind::SocialSearch {
                        platform: SocialPlatform::Twitter,
                        topics,
                        limit,
                    });
                }
            }
        }
        let identifier = extract_path_identifier(original);
        return Some(TargetKind::Social {
            platform: SocialPlatform::Twitter,
            identifier,
        });
    }

    // TikTok
    if lower.contains("tiktok.com") {
        // /search?q=X → social search
        if lower.contains("/search") {
            if let Some(ref u) = parsed {
                let topics = extract_query_topics(u);
                if !topics.is_empty() {
                    return Some(TargetKind::SocialSearch {
                        platform: SocialPlatform::TikTok,
                        topics,
                        limit,
                    });
                }
            }
        }
        let identifier = extract_path_identifier(original);
        return Some(TargetKind::Social {
            platform: SocialPlatform::TikTok,
            identifier,
        });
    }

    // Facebook
    if lower.contains("facebook.com") {
        // /search/posts/?q=X → social search
        if lower.contains("/search") {
            if let Some(ref u) = parsed {
                let topics = extract_query_topics(u);
                if !topics.is_empty() {
                    return Some(TargetKind::SocialSearch {
                        platform: SocialPlatform::Facebook,
                        topics,
                        limit,
                    });
                }
            }
        }
        let identifier = extract_path_identifier(original);
        return Some(TargetKind::Social {
            platform: SocialPlatform::Facebook,
            identifier,
        });
    }

    // Bluesky
    if lower.contains("bsky.app") {
        // /search?q=X → social search
        if lower.contains("/search") {
            if let Some(ref u) = parsed {
                let topics = extract_query_topics(u);
                if !topics.is_empty() {
                    return Some(TargetKind::SocialSearch {
                        platform: SocialPlatform::Bluesky,
                        topics,
                        limit,
                    });
                }
            }
        }
        let identifier = extract_path_identifier(original);
        return Some(TargetKind::Social {
            platform: SocialPlatform::Bluesky,
            identifier,
        });
    }

    None
}

/// Extract topics from the path segment after a known prefix.
/// "https://instagram.com/explore/tags/coffee+minneapolis?limit=30" → ["coffee", "minneapolis"]
fn extract_after_segment(url: &str, segment: &str) -> Vec<String> {
    let idx = match url.find(segment) {
        Some(i) => i,
        None => return Vec::new(),
    };
    let rest = &url[idx + segment.len()..];
    // Take everything up to ? or end, trim slashes
    let path_part = rest.split('?').next().unwrap_or(rest).trim_matches('/');
    if path_part.is_empty() {
        return Vec::new();
    }
    path_part
        .split('+')
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Extract topics from the `q` query parameter, split on `+` or spaces.
fn extract_query_topics(url: &url::Url) -> Vec<String> {
    url.query_pairs()
        .find(|(k, _)| k == "q")
        .map(|(_, v)| {
            v.split(|c: char| c == '+' || c.is_whitespace())
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Extract the first meaningful path segment as an identifier.
/// "https://instagram.com/mnfoodshelf/" → "mnfoodshelf"
fn extract_path_identifier(url: &str) -> String {
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(url)
        .to_string()
}

/// Extract Reddit identifier: subreddit name from "/r/Name" or username from "/user/Name".
fn extract_reddit_identifier(url: &str) -> String {
    if let Some(idx) = url.find("/r/") {
        let rest = &url[idx + 3..];
        return rest
            .trim_end_matches('/')
            .split('/')
            .next()
            .unwrap_or(rest)
            .to_string();
    }
    if let Some(idx) = url.find("/user/") {
        let rest = &url[idx + 6..];
        return rest
            .trim_end_matches('/')
            .split('/')
            .next()
            .unwrap_or(rest)
            .to_string();
    }
    extract_path_identifier(url)
}

/// Determine content kind from HTTP Content-Type header and optional body bytes.
/// Used after fetching a URL to decide how to process the response.
pub(crate) fn detect_content_kind(content_type: &str, body: Option<&[u8]>) -> ContentKind {
    let ct = content_type.to_lowercase();

    if ct.contains("application/pdf") {
        return ContentKind::Pdf;
    }
    if ct.contains("application/rss+xml")
        || ct.contains("application/atom+xml")
        || ct.contains("application/feed+json")
    {
        return ContentKind::Feed;
    }
    if ct.contains("text/html") || ct.contains("application/xhtml") {
        return ContentKind::Html;
    }
    // Ambiguous XML — could be RSS/Atom or something else. Sniff the body.
    if ct.contains("text/xml") || ct.contains("application/xml") {
        if let Some(bytes) = body {
            if looks_like_feed(bytes) {
                return ContentKind::Feed;
            }
        }
        return ContentKind::Raw;
    }
    ContentKind::Raw
}

/// Quick heuristic: does this XML body look like an RSS or Atom feed?
fn looks_like_feed(body: &[u8]) -> bool {
    let preview = std::str::from_utf8(&body[..body.len().min(500)]).unwrap_or("");
    let lower = preview.to_lowercase();
    lower.contains("<rss") || lower.contains("<feed") || lower.contains("<atom")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_query() {
        assert_eq!(
            detect_target("affordable housing Minneapolis"),
            TargetKind::WebQuery("affordable housing Minneapolis".into())
        );
    }

    #[test]
    fn bare_subreddit() {
        assert_eq!(
            detect_target("r/Minneapolis"),
            TargetKind::Social {
                platform: SocialPlatform::Reddit,
                identifier: "Minneapolis".into(),
            }
        );
    }

    #[test]
    fn instagram_url() {
        match detect_target("https://instagram.com/mnfoodshelf/") {
            TargetKind::Social {
                platform,
                identifier,
            } => {
                assert_eq!(platform, SocialPlatform::Instagram);
                assert_eq!(identifier, "mnfoodshelf");
            }
            other => panic!("expected Social, got {:?}", other),
        }
    }

    #[test]
    fn instagram_explore_tags() {
        match detect_target("https://www.instagram.com/explore/tags/minneapolis+coffee?limit=30") {
            TargetKind::SocialSearch {
                platform,
                topics,
                limit,
            } => {
                assert_eq!(platform, SocialPlatform::Instagram);
                assert_eq!(topics, vec!["minneapolis", "coffee"]);
                assert_eq!(limit, 30);
            }
            other => panic!("expected SocialSearch, got {:?}", other),
        }
    }

    #[test]
    fn instagram_explore_tags_default_limit() {
        match detect_target("https://www.instagram.com/explore/tags/localfood") {
            TargetKind::SocialSearch {
                platform,
                topics,
                limit,
            } => {
                assert_eq!(platform, SocialPlatform::Instagram);
                assert_eq!(topics, vec!["localfood"]);
                assert_eq!(limit, DEFAULT_SOCIAL_SEARCH_LIMIT);
            }
            other => panic!("expected SocialSearch, got {:?}", other),
        }
    }

    #[test]
    fn reddit_search() {
        match detect_target("https://www.reddit.com/search/?q=minneapolis+housing&limit=25") {
            TargetKind::SocialSearch {
                platform,
                topics,
                limit,
            } => {
                assert_eq!(platform, SocialPlatform::Reddit);
                assert_eq!(topics, vec!["minneapolis", "housing"]);
                assert_eq!(limit, 25);
            }
            other => panic!("expected SocialSearch, got {:?}", other),
        }
    }

    #[test]
    fn twitter_search() {
        match detect_target("https://x.com/search?q=minneapolis") {
            TargetKind::SocialSearch {
                platform,
                topics,
                limit,
            } => {
                assert_eq!(platform, SocialPlatform::Twitter);
                assert_eq!(topics, vec!["minneapolis"]);
                assert_eq!(limit, DEFAULT_SOCIAL_SEARCH_LIMIT);
            }
            other => panic!("expected SocialSearch, got {:?}", other),
        }
    }

    #[test]
    fn tiktok_search() {
        match detect_target("https://www.tiktok.com/search?q=coffee+shops") {
            TargetKind::SocialSearch {
                platform,
                topics,
                limit,
            } => {
                assert_eq!(platform, SocialPlatform::TikTok);
                assert_eq!(topics, vec!["coffee", "shops"]);
                assert_eq!(limit, DEFAULT_SOCIAL_SEARCH_LIMIT);
            }
            other => panic!("expected SocialSearch, got {:?}", other),
        }
    }

    #[test]
    fn reddit_subreddit_url() {
        match detect_target("https://reddit.com/r/Minneapolis") {
            TargetKind::Social {
                platform,
                identifier,
            } => {
                assert_eq!(platform, SocialPlatform::Reddit);
                assert_eq!(identifier, "Minneapolis");
            }
            other => panic!("expected Social, got {:?}", other),
        }
    }

    #[test]
    fn twitter_and_x_urls() {
        for url in &["https://twitter.com/handle", "https://x.com/handle"] {
            match detect_target(url) {
                TargetKind::Social {
                    platform,
                    identifier,
                } => {
                    assert_eq!(platform, SocialPlatform::Twitter);
                    assert_eq!(identifier, "handle");
                }
                other => panic!("expected Social for {url}, got {:?}", other),
            }
        }
    }

    #[test]
    fn tiktok_url() {
        match detect_target("https://tiktok.com/@user") {
            TargetKind::Social {
                platform,
                identifier,
            } => {
                assert_eq!(platform, SocialPlatform::TikTok);
                assert_eq!(identifier, "@user");
            }
            other => panic!("expected Social, got {:?}", other),
        }
    }

    #[test]
    fn bluesky_url() {
        match detect_target("https://bsky.app/profile/someone.bsky.social") {
            TargetKind::Social {
                platform,
                identifier,
            } => {
                assert_eq!(platform, SocialPlatform::Bluesky);
                assert_eq!(identifier, "someone.bsky.social");
            }
            other => panic!("expected Social, got {:?}", other),
        }
    }

    #[test]
    fn generic_url() {
        assert_eq!(
            detect_target("https://city.gov/about"),
            TargetKind::Url("https://city.gov/about".into())
        );
    }

    #[test]
    fn content_type_html() {
        assert_eq!(
            detect_content_kind("text/html; charset=utf-8", None),
            ContentKind::Html
        );
    }

    #[test]
    fn content_type_pdf() {
        assert_eq!(
            detect_content_kind("application/pdf", None),
            ContentKind::Pdf
        );
    }

    #[test]
    fn content_type_rss() {
        assert_eq!(
            detect_content_kind("application/rss+xml", None),
            ContentKind::Feed
        );
    }

    #[test]
    fn content_type_ambiguous_xml_with_rss_body() {
        let body = b"<?xml version=\"1.0\"?><rss version=\"2.0\"><channel>...";
        assert_eq!(
            detect_content_kind("text/xml", Some(body)),
            ContentKind::Feed
        );
    }

    #[test]
    fn content_type_ambiguous_xml_without_feed() {
        let body = b"<?xml version=\"1.0\"?><data><item/></data>";
        assert_eq!(
            detect_content_kind("text/xml", Some(body)),
            ContentKind::Raw
        );
    }
}
