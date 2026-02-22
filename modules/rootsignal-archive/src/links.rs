use std::sync::LazyLock;
use regex::Regex;

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
/// Resolves relative URLs against `base_url` and deduplicates.
pub fn extract_all_links(html: &str, base_url: &str) -> Vec<String> {
    let base = url::Url::parse(base_url).ok();
    let mut seen = std::collections::HashSet::new();
    let mut links = Vec::new();

    for cap in HREF_RE.captures_iter(html) {
        if let Some(resolved) = resolve_href(&cap[1], base.as_ref()) {
            if seen.insert(resolved.clone()) {
                links.push(resolved);
            }
        }
    }

    links
}

/// Extract links from raw HTML that match a given URL pattern.
/// Resolves relative URLs against `base_url`, deduplicates, and caps at 20 results.
pub fn extract_links_by_pattern(html: &str, base_url: &str, pattern: &str) -> Vec<String> {
    let base = url::Url::parse(base_url).ok();
    let mut seen = std::collections::HashSet::new();
    let mut links = Vec::new();

    for cap in HREF_RE.captures_iter(html) {
        if let Some(resolved) = resolve_href(&cap[1], base.as_ref()) {
            if resolved.contains(pattern) && seen.insert(resolved.clone()) {
                links.push(resolved);
                if links.len() >= 20 {
                    break;
                }
            }
        }
    }

    links
}
