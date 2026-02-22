use std::sync::LazyLock;
use regex::Regex;

static MENTION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"@([\w.]+)").expect("valid regex"));
static HASHTAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#([\w]+)").expect("valid regex"));

/// Extract @mentions from text. Returns deduplicated, lowercased usernames without the @ prefix.
pub fn extract_mentions(text: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    MENTION_RE
        .captures_iter(text)
        .filter_map(|c| {
            let name = c[1].to_lowercase();
            seen.insert(name.clone()).then_some(name)
        })
        .collect()
}

/// Extract #hashtags from text. Returns deduplicated, lowercased tags without the # prefix.
pub fn extract_hashtags(text: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    HASHTAG_RE
        .captures_iter(text)
        .filter_map(|c| {
            let tag = c[1].to_lowercase();
            seen.insert(tag.clone()).then_some(tag)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_mentions() {
        let text = "Hey @alice and @Bob.Smith, check this out @alice";
        let mentions = extract_mentions(text);
        assert_eq!(mentions, vec!["alice", "bob.smith"]);
    }

    #[test]
    fn extracts_hashtags() {
        let text = "Love #Minneapolis and #community vibes #Minneapolis";
        let tags = extract_hashtags(text);
        assert_eq!(tags, vec!["minneapolis", "community"]);
    }

    #[test]
    fn empty_text() {
        assert!(extract_mentions("").is_empty());
        assert!(extract_hashtags("").is_empty());
    }

    #[test]
    fn no_matches() {
        assert!(extract_mentions("no mentions here").is_empty());
        assert!(extract_hashtags("no hashtags here").is_empty());
    }
}
