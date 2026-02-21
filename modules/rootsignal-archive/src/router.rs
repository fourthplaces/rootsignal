/// Target detection: URL pattern matching and content-type routing.
/// Given a target string, determine what kind of content it is.

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TargetKind {
    /// Plain text query (not a URL) → web search via Serper
    WebQuery,
    /// Social platform profile or feed URL → Apify
    Social {
        platform: rootsignal_common::SocialPlatform,
        identifier: String,
    },
    /// HTTP(S) URL — needs content-type detection to determine Page/Feed/Pdf/Raw
    Url(String),
}

// Router implementation will be added in Phase 2.
