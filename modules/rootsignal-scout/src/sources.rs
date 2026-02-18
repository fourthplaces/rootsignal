use rootsignal_common::SourceType;

/// Build a canonical key: `city_slug:source_type:canonical_value`.
pub fn make_canonical_key(city_slug: &str, source_type: SourceType, canonical_value: &str) -> String {
    format!("{}:{}:{}", city_slug, source_type, canonical_value)
}

/// Extract the canonical value for a source given its type and URL/identifier.
/// For Instagram: extracts username from URL. For Reddit: extracts subreddit. Others: returns as-is.
pub fn canonical_value_from_url(source_type: SourceType, url_or_value: &str) -> String {
    match source_type {
        SourceType::Instagram => {
            // https://www.instagram.com/{username}/ → username
            url_or_value
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or(url_or_value)
                .to_string()
        }
        SourceType::Reddit => {
            // https://www.reddit.com/r/{subreddit} → subreddit
            if let Some(idx) = url_or_value.find("/r/") {
                url_or_value[idx + 3..]
                    .trim_end_matches('/')
                    .split('/')
                    .next()
                    .unwrap_or(url_or_value)
                    .to_string()
            } else {
                url_or_value.to_string()
            }
        }
        SourceType::TikTok => {
            // https://www.tiktok.com/@{username} → username
            url_or_value
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or(url_or_value)
                .trim_start_matches('@')
                .to_string()
        }
        SourceType::Twitter => {
            // https://twitter.com/{handle} or https://x.com/{handle} → handle
            url_or_value
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or(url_or_value)
                .trim_start_matches('@')
                .to_string()
        }
        // Web, Facebook, TavilyQuery, GoFundMeQuery, EventbriteQuery, VolunteerMatchQuery, Bluesky: use as-is
        _ => url_or_value.to_string(),
    }
}
