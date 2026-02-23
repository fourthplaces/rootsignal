use std::sync::LazyLock;
use regex::Regex;

/// Matches any absolute URL anywhere in the text — href, data attributes, JS, plain text, etc.
static URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"https?://[^\s"'<>\]\)}{]+"#).expect("valid regex"));

/// Matches href attributes specifically (for relative URL resolution).
static HREF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"href\s*=\s*["']([^"']+)["']"#).expect("valid regex"));

/// Resolve a raw href against a base URL, returning an absolute URL.
fn resolve_href(raw: &str, base: Option<&url::Url>) -> Option<String> {
    if raw.starts_with("http://") || raw.starts_with("https://") {
        Some(raw.to_string())
    } else {
        base?.join(raw).ok().map(|u| u.to_string())
    }
}

/// Extract all links from raw HTML.
/// Finds every URL in the document — href attributes, data attributes, inline JS, plain text.
/// Also resolves relative hrefs against `base_url`. Deduplicates.
pub fn extract_all_links(html: &str, base_url: &str) -> Vec<String> {
    let base = url::Url::parse(base_url).ok();
    let mut seen = std::collections::HashSet::new();
    let mut links = Vec::new();

    // Absolute URLs anywhere in the HTML
    for m in URL_RE.find_iter(html) {
        let url = m.as_str().to_string();
        if seen.insert(url.clone()) {
            links.push(url);
        }
    }

    // Relative hrefs resolved against the base
    for cap in HREF_RE.captures_iter(html) {
        let raw = &cap[1];
        if raw.starts_with("http://") || raw.starts_with("https://") {
            continue; // already caught by URL_RE
        }
        if let Some(resolved) = resolve_href(raw, base.as_ref()) {
            if seen.insert(resolved.clone()) {
                links.push(resolved);
            }
        }
    }

    links
}

/// Extract links from raw HTML that match a given URL pattern.
/// Catches all URLs (not just href), deduplicates.
pub fn extract_links_by_pattern(html: &str, base_url: &str, pattern: &str) -> Vec<String> {
    extract_all_links(html, base_url)
        .into_iter()
        .filter(|url| pattern.is_empty() || url.contains(pattern))
        .collect()
}
