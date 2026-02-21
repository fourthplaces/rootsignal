// Target detection: URL pattern matching and content-type routing.

use rootsignal_common::SocialPlatform;

/// What kind of target is this? Determined from the target string alone (no HTTP).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TargetKind {
    /// Plain text query (not a URL) → web search via Serper
    WebQuery(String),
    /// Social platform profile or feed URL → Apify
    Social {
        platform: SocialPlatform,
        identifier: String,
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
pub(crate) fn detect_target(target: &str) -> TargetKind {
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
fn detect_social_url(lower: &str, original: &str) -> Option<TargetKind> {
    if lower.contains("instagram.com") {
        let identifier = extract_path_identifier(original);
        return Some(TargetKind::Social {
            platform: SocialPlatform::Instagram,
            identifier,
        });
    }
    if lower.contains("facebook.com") {
        let identifier = extract_path_identifier(original);
        return Some(TargetKind::Social {
            platform: SocialPlatform::Facebook,
            identifier,
        });
    }
    if lower.contains("reddit.com") {
        let identifier = extract_reddit_identifier(original);
        return Some(TargetKind::Social {
            platform: SocialPlatform::Reddit,
            identifier,
        });
    }
    if lower.contains("tiktok.com") {
        let identifier = extract_path_identifier(original);
        return Some(TargetKind::Social {
            platform: SocialPlatform::TikTok,
            identifier,
        });
    }
    if lower.contains("twitter.com") || lower.contains("x.com/") {
        let identifier = extract_path_identifier(original);
        return Some(TargetKind::Social {
            platform: SocialPlatform::Twitter,
            identifier,
        });
    }
    if lower.contains("bsky.app") {
        let identifier = extract_path_identifier(original);
        return Some(TargetKind::Social {
            platform: SocialPlatform::Bluesky,
            identifier,
        });
    }
    None
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
