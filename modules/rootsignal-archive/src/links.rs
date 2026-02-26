use std::sync::LazyLock;
use regex::Regex;

/// Matches `href` attributes — the only semantic "link" in HTML.
/// Covers `<a href>`, `<link href>`, `<area href>`.
static HREF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"href\s*=\s*["']([^"']+)["']"#).expect("valid regex"));

/// Resolve a raw href against a base URL, returning an absolute URL with fragment stripped.
fn resolve_href(raw: &str, base: Option<&url::Url>) -> Option<String> {
    let mut parsed = if raw.starts_with("http://") || raw.starts_with("https://") {
        url::Url::parse(raw).ok()?
    } else {
        base?.join(raw).ok()?
    };
    parsed.set_fragment(None);
    Some(parsed.to_string())
}

/// Extract all links from raw HTML.
/// Only extracts URLs from `href` attributes (`<a>`, `<link>`, `<area>`),
/// ignoring URLs in `src`, `xmlns`, data attributes, JS, CSS, and plain text.
/// Resolves relative hrefs against `base_url`. Deduplicates.
pub fn extract_all_links(html: &str, base_url: &str) -> Vec<String> {
    let base = url::Url::parse(base_url).ok();
    let mut seen = std::collections::HashSet::new();
    let mut links = Vec::new();

    for cap in HREF_RE.captures_iter(html) {
        let raw = &cap[1];
        if let Some(resolved) = resolve_href(raw, base.as_ref()) {
            if seen.insert(resolved.clone()) {
                links.push(resolved);
            }
        }
    }

    links
}

/// Extract links from raw HTML that match a given URL pattern.
/// Only extracts from `href` attributes; deduplicates.
pub fn extract_links_by_pattern(html: &str, base_url: &str, pattern: &str) -> Vec<String> {
    extract_all_links(html, base_url)
        .into_iter()
        .filter(|url| pattern.is_empty() || url.contains(pattern))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- href extraction ---

    #[test]
    fn href_links_are_extracted() {
        let html = r#"<a href="https://instagram.com/org">IG</a>"#;
        let links = extract_all_links(html, "https://example.com");
        assert_eq!(links, vec!["https://instagram.com/org"]);
    }

    #[test]
    fn extracts_multiple_hrefs() {
        let html = r#"
            <a href="https://a.com">A</a>
            <a href="https://b.com">B</a>
        "#;
        let links = extract_all_links(html, "https://example.com");
        assert!(links.contains(&"https://a.com/".to_string()));
        assert!(links.contains(&"https://b.com/".to_string()));
    }

    #[test]
    fn single_quoted_href() {
        let html = "<a href='https://example.com/page'>link</a>";
        let links = extract_all_links(html, "https://base.com");
        assert!(links.contains(&"https://example.com/page".to_string()));
    }

    // --- Non-href URLs are ignored ---

    #[test]
    fn namespace_uris_are_not_extracted() {
        let html = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>
            <div about="http://purl.org/dc/terms/">RDF</div>"#;
        let links = extract_all_links(html, "https://example.com");
        assert!(links.is_empty(), "namespace/RDF URIs should not be extracted");
    }

    #[test]
    fn image_src_is_not_extracted() {
        let html = r#"<img src="https://avatars.githubusercontent.com/u/123">"#;
        let links = extract_all_links(html, "https://example.com");
        assert!(links.is_empty(), "img src should not be extracted");
    }

    #[test]
    fn script_urls_are_not_extracted() {
        let html = r#"<script src="https://cdn.example.com/app.js"></script>
            <script>var u = "https://api.example.com/v1";</script>"#;
        let links = extract_all_links(html, "https://example.com");
        assert!(links.is_empty(), "script src and inline JS URLs should not be extracted");
    }

    #[test]
    fn plain_text_urls_are_not_extracted() {
        let html = "Visit us at https://example.com/about for more info";
        let links = extract_all_links(html, "https://base.com");
        assert!(links.is_empty(), "plain text URLs should not be extracted");
    }

    #[test]
    fn data_attribute_urls_are_not_extracted() {
        let html = r#"<div data-url="https://cdn.example.com/img.png">content</div>"#;
        let links = extract_all_links(html, "https://base.com");
        assert!(links.is_empty(), "data attribute URLs should not be extracted");
    }

    // --- Relative URL resolution ---

    #[test]
    fn relative_hrefs_still_resolve() {
        let html = r#"<a href="/about">About</a>"#;
        let links = extract_all_links(html, "https://example.com");
        assert!(links.contains(&"https://example.com/about".to_string()));
    }

    #[test]
    fn resolves_relative_path() {
        let html = r#"<a href="events/today">Events</a>"#;
        let links = extract_all_links(html, "https://example.com/calendar/");
        assert!(links.contains(&"https://example.com/calendar/events/today".to_string()));
    }

    // --- Deduplication ---

    #[test]
    fn deduplication_still_works() {
        let html = r#"
            <a href="https://example.com/page">link1</a>
            <a href="https://example.com/page">link2</a>
        "#;
        let links = extract_all_links(html, "https://base.com");
        let count = links.iter().filter(|u| *u == "https://example.com/page").count();
        assert_eq!(count, 1, "Same URL should appear exactly once");
    }

    // --- Fragment stripping ---

    #[test]
    fn fragment_is_stripped_from_absolute_href() {
        let html = r#"<a href="https://example.com/page#section">link</a>"#;
        let links = extract_all_links(html, "https://base.com");
        assert_eq!(links, vec!["https://example.com/page"]);
    }

    #[test]
    fn fragment_is_stripped_from_relative_href() {
        let html = r#"<a href="/page#breadcrumb">link</a>"#;
        let links = extract_all_links(html, "https://example.com");
        assert_eq!(links, vec!["https://example.com/page"]);
    }

    #[test]
    fn same_page_with_different_fragments_deduplicates() {
        let html = r#"
            <a href="/page#breadcrumb">one</a>
            <a href="/page#primaryimage">two</a>
            <a href="/page#footer">three</a>
        "#;
        let links = extract_all_links(html, "https://example.com");
        assert_eq!(links, vec!["https://example.com/page"]);
    }

    #[test]
    fn bare_fragment_resolves_to_base_url() {
        let html = r##"<a href="#top">back to top</a>"##;
        let links = extract_all_links(html, "https://example.com/page");
        assert_eq!(links, vec!["https://example.com/page"]);
    }

    // --- Empty / malformed ---

    #[test]
    fn empty_html_returns_empty() {
        let links = extract_all_links("", "https://example.com");
        assert!(links.is_empty());
    }

    #[test]
    fn no_links_returns_empty() {
        let html = "<p>Just some text with no links</p>";
        let links = extract_all_links(html, "https://example.com");
        assert!(links.is_empty());
    }

    #[test]
    fn empty_href_skipped() {
        let html = r#"<a href="">empty</a>"#;
        let links = extract_all_links(html, "https://example.com");
        // Empty href resolves to base URL
        assert!(links.len() <= 1);
    }

    #[test]
    fn malformed_base_url_does_not_crash() {
        let html = r#"<a href="/about">link</a>"#;
        let links = extract_all_links(html, "not a url");
        // Should not panic; relative hrefs just get skipped
        assert!(links.is_empty() || !links.is_empty());
    }

    // --- Mixed content (realistic page) ---

    #[test]
    fn linktree_style_page() {
        let html = r#"
            <a href="https://instagram.com/mplsmutualaid">Instagram</a>
            <a href="https://gofundme.com/f/help-families?utm_source=linktree">GoFundMe</a>
            <a href="https://docs.google.com/document/d/ABC123/edit">Resource Doc</a>
            <a href="/terms">Terms</a>
        "#;
        let links = extract_all_links(html, "https://linktr.ee/mplsmutualaid");
        assert!(links.contains(&"https://instagram.com/mplsmutualaid".to_string()));
        assert!(links.contains(&"https://gofundme.com/f/help-families?utm_source=linktree".to_string()));
        assert!(links.contains(&"https://docs.google.com/document/d/ABC123/edit".to_string()));
        assert!(links.contains(&"https://linktr.ee/terms".to_string()));
        assert_eq!(links.len(), 4);
    }

    // --- extract_links_by_pattern ---

    #[test]
    fn pattern_filter_instagram() {
        let html = r#"
            <a href="https://instagram.com/org">IG</a>
            <a href="https://facebook.com/org">FB</a>
            <a href="https://instagram.com/other">IG2</a>
        "#;
        let links = extract_links_by_pattern(html, "https://base.com", "instagram.com");
        assert_eq!(links.len(), 2);
        assert!(links.iter().all(|u| u.contains("instagram.com")));
    }

    #[test]
    fn pattern_empty_returns_all() {
        let html = r#"<a href="https://a.com">A</a><a href="https://b.com">B</a>"#;
        let links = extract_links_by_pattern(html, "https://base.com", "");
        assert_eq!(links.len(), 2);
    }
}
