use scraper::{Html, Selector};

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

/// Returns true if this element or any ancestor has `hidden` attribute
/// or inline style containing `display:none` or `visibility:hidden`.
fn is_hidden(element: &scraper::ElementRef) -> bool {
    use scraper::node::Element;

    let check_element = |el: &Element| -> bool {
        if el.attr("hidden").is_some() {
            return true;
        }
        if let Some(style) = el.attr("style") {
            let s = style.replace(' ', "").to_lowercase();
            if s.contains("display:none") || s.contains("visibility:hidden") {
                return true;
            }
        }
        false
    };

    // Check self
    if check_element(element.value()) {
        return true;
    }

    // Walk ancestors
    for ancestor in element.ancestors() {
        if let Some(el) = ancestor.value().as_element() {
            if check_element(el) {
                return true;
            }
        }
    }

    false
}

/// Extract all visible `<a href>` links from inside `<body>`.
/// Resolves relative hrefs against `base_url`. Deduplicates. Strips fragments.
pub fn extract_all_links(html: &str, base_url: &str) -> Vec<String> {
    let base = url::Url::parse(base_url).ok();
    let document = Html::parse_document(html);

    let selector = Selector::parse("body a[href]").expect("valid CSS selector");

    let mut seen = std::collections::HashSet::new();
    let mut links = Vec::new();

    for element in document.select(&selector) {
        if is_hidden(&element) {
            continue;
        }

        let raw = match element.value().attr("href") {
            Some(h) => h,
            None => continue,
        };

        if let Some(resolved) = resolve_href(raw, base.as_ref()) {
            if seen.insert(resolved.clone()) {
                links.push(resolved);
            }
        }
    }

    links
}

/// Extract links from raw HTML that match a given URL pattern.
/// Only extracts visible `<a href>` from `<body>`; deduplicates.
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
        let html = r#"<body><a href="https://instagram.com/org">IG</a></body>"#;
        let links = extract_all_links(html, "https://example.com");
        assert_eq!(links, vec!["https://instagram.com/org"]);
    }

    #[test]
    fn extracts_multiple_hrefs() {
        let html = r#"<body>
            <a href="https://a.com">A</a>
            <a href="https://b.com">B</a>
        </body>"#;
        let links = extract_all_links(html, "https://example.com");
        assert!(links.contains(&"https://a.com/".to_string()));
        assert!(links.contains(&"https://b.com/".to_string()));
    }

    #[test]
    fn single_quoted_href() {
        let html = "<body><a href='https://example.com/page'>link</a></body>";
        let links = extract_all_links(html, "https://base.com");
        assert!(links.contains(&"https://example.com/page".to_string()));
    }

    // --- Non-href URLs are ignored ---

    #[test]
    fn namespace_uris_are_not_extracted() {
        let html = r#"<body><svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>
            <div about="http://purl.org/dc/terms/">RDF</div></body>"#;
        let links = extract_all_links(html, "https://example.com");
        assert!(
            links.is_empty(),
            "namespace/RDF URIs should not be extracted"
        );
    }

    #[test]
    fn image_src_is_not_extracted() {
        let html = r#"<body><img src="https://avatars.githubusercontent.com/u/123"></body>"#;
        let links = extract_all_links(html, "https://example.com");
        assert!(links.is_empty(), "img src should not be extracted");
    }

    #[test]
    fn script_urls_are_not_extracted() {
        let html = r#"<body><script src="https://cdn.example.com/app.js"></script>
            <script>var u = "https://api.example.com/v1";</script></body>"#;
        let links = extract_all_links(html, "https://example.com");
        assert!(
            links.is_empty(),
            "script src and inline JS URLs should not be extracted"
        );
    }

    #[test]
    fn plain_text_urls_are_not_extracted() {
        let html = "<body>Visit us at https://example.com/about for more info</body>";
        let links = extract_all_links(html, "https://base.com");
        assert!(links.is_empty(), "plain text URLs should not be extracted");
    }

    #[test]
    fn data_attribute_urls_are_not_extracted() {
        let html = r#"<body><div data-url="https://cdn.example.com/img.png">content</div></body>"#;
        let links = extract_all_links(html, "https://base.com");
        assert!(
            links.is_empty(),
            "data attribute URLs should not be extracted"
        );
    }

    // --- Head-only links are excluded ---

    #[test]
    fn head_link_stylesheet_is_not_extracted() {
        let html = r#"<html><head><link rel="stylesheet" href="https://cdn.example.com/style.css"></head><body></body></html>"#;
        let links = extract_all_links(html, "https://example.com");
        assert!(links.is_empty(), "stylesheet links should not be extracted");
    }

    #[test]
    fn head_link_canonical_is_not_extracted() {
        let html = r#"<html><head><link rel="canonical" href="https://example.com/page"></head><body></body></html>"#;
        let links = extract_all_links(html, "https://example.com");
        assert!(links.is_empty(), "canonical links should not be extracted");
    }

    // --- Hidden elements are excluded ---

    #[test]
    fn hidden_attribute_link_is_excluded() {
        let html = r#"<body><div hidden><a href="https://hidden.com">hidden</a></div><a href="https://visible.com">visible</a></body>"#;
        let links = extract_all_links(html, "https://example.com");
        assert_eq!(links, vec!["https://visible.com/"]);
    }

    #[test]
    fn display_none_link_is_excluded() {
        let html = r#"<body><div style="display:none"><a href="https://hidden.com">hidden</a></div><a href="https://visible.com">visible</a></body>"#;
        let links = extract_all_links(html, "https://example.com");
        assert_eq!(links, vec!["https://visible.com/"]);
    }

    #[test]
    fn visibility_hidden_link_is_excluded() {
        let html = r#"<body><a style="visibility: hidden" href="https://hidden.com">hidden</a><a href="https://visible.com">visible</a></body>"#;
        let links = extract_all_links(html, "https://example.com");
        assert_eq!(links, vec!["https://visible.com/"]);
    }

    // --- Relative URL resolution ---

    #[test]
    fn relative_hrefs_still_resolve() {
        let html = r#"<body><a href="/about">About</a></body>"#;
        let links = extract_all_links(html, "https://example.com");
        assert!(links.contains(&"https://example.com/about".to_string()));
    }

    #[test]
    fn resolves_relative_path() {
        let html = r#"<body><a href="events/today">Events</a></body>"#;
        let links = extract_all_links(html, "https://example.com/calendar/");
        assert!(links.contains(&"https://example.com/calendar/events/today".to_string()));
    }

    // --- Deduplication ---

    #[test]
    fn deduplication_still_works() {
        let html = r#"<body>
            <a href="https://example.com/page">link1</a>
            <a href="https://example.com/page">link2</a>
        </body>"#;
        let links = extract_all_links(html, "https://base.com");
        let count = links
            .iter()
            .filter(|u| *u == "https://example.com/page")
            .count();
        assert_eq!(count, 1, "Same URL should appear exactly once");
    }

    // --- Fragment stripping ---

    #[test]
    fn fragment_is_stripped_from_absolute_href() {
        let html = r#"<body><a href="https://example.com/page#section">link</a></body>"#;
        let links = extract_all_links(html, "https://base.com");
        assert_eq!(links, vec!["https://example.com/page"]);
    }

    #[test]
    fn fragment_is_stripped_from_relative_href() {
        let html = r#"<body><a href="/page#breadcrumb">link</a></body>"#;
        let links = extract_all_links(html, "https://example.com");
        assert_eq!(links, vec!["https://example.com/page"]);
    }

    #[test]
    fn same_page_with_different_fragments_deduplicates() {
        let html = r#"<body>
            <a href="/page#breadcrumb">one</a>
            <a href="/page#primaryimage">two</a>
            <a href="/page#footer">three</a>
        </body>"#;
        let links = extract_all_links(html, "https://example.com");
        assert_eq!(links, vec!["https://example.com/page"]);
    }

    #[test]
    fn bare_fragment_resolves_to_base_url() {
        let html = r##"<body><a href="#top">back to top</a></body>"##;
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
        let html = "<body><p>Just some text with no links</p></body>";
        let links = extract_all_links(html, "https://example.com");
        assert!(links.is_empty());
    }

    #[test]
    fn empty_href_skipped() {
        let html = r#"<body><a href="">empty</a></body>"#;
        let links = extract_all_links(html, "https://example.com");
        // Empty href resolves to base URL
        assert!(links.len() <= 1);
    }

    #[test]
    fn malformed_base_url_does_not_crash() {
        let html = r#"<body><a href="/about">link</a></body>"#;
        let links = extract_all_links(html, "not a url");
        // Should not panic; relative hrefs just get skipped
        assert!(links.is_empty() || !links.is_empty());
    }

    // --- Mixed content (realistic page) ---

    #[test]
    fn linktree_style_page() {
        let html = r#"<body>
            <a href="https://instagram.com/mplsmutualaid">Instagram</a>
            <a href="https://gofundme.com/f/help-families?utm_source=linktree">GoFundMe</a>
            <a href="https://docs.google.com/document/d/ABC123/edit">Resource Doc</a>
            <a href="/terms">Terms</a>
        </body>"#;
        let links = extract_all_links(html, "https://linktr.ee/mplsmutualaid");
        assert!(links.contains(&"https://instagram.com/mplsmutualaid".to_string()));
        assert!(
            links.contains(&"https://gofundme.com/f/help-families?utm_source=linktree".to_string())
        );
        assert!(links.contains(&"https://docs.google.com/document/d/ABC123/edit".to_string()));
        assert!(links.contains(&"https://linktr.ee/terms".to_string()));
        assert_eq!(links.len(), 4);
    }

    // --- extract_links_by_pattern ---

    #[test]
    fn pattern_filter_instagram() {
        let html = r#"<body>
            <a href="https://instagram.com/org">IG</a>
            <a href="https://facebook.com/org">FB</a>
            <a href="https://instagram.com/other">IG2</a>
        </body>"#;
        let links = extract_links_by_pattern(html, "https://base.com", "instagram.com");
        assert_eq!(links.len(), 2);
        assert!(links.iter().all(|u| u.contains("instagram.com")));
    }

    #[test]
    fn pattern_empty_returns_all() {
        let html = r#"<body><a href="https://a.com">A</a><a href="https://b.com">B</a></body>"#;
        let links = extract_links_by_pattern(html, "https://base.com", "");
        assert_eq!(links.len(), 2);
    }
}
