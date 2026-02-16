use scraper::{ElementRef, Html, Selector};
use std::collections::HashMap;
use url::Url;

use crate::scraping::url_alias::normalize_url;

const MAX_LINKS_PER_PAGE: usize = 500;

#[derive(Debug, Clone)]
pub struct ExtractedLink {
    pub target_url: String,
    pub anchor_text: Option<String>,
    pub surrounding_text: Option<String>,
    pub section: Option<String>,
}

/// Extract outbound links from HTML with rich context for LLM crawl decisions.
///
/// - Resolves relative URLs against `base_url`
/// - Normalizes target URLs via `normalize_url()`
/// - Filters: no self-links, no non-http schemes, no javascript/mailto/tel
/// - Caps at 500 links per page after dedup
/// - Deduplicates by target_url, preferring body context over nav/footer
pub fn extract_links_with_context(html: &str, base_url: &str) -> Vec<ExtractedLink> {
    let base = match Url::parse(base_url) {
        Ok(u) => u,
        Err(_) => return Vec::new(),
    };
    let normalized_base = normalize_url(base_url).unwrap_or_else(|_| base_url.to_string());

    let document = Html::parse_document(html);
    let anchor_selector = Selector::parse("a[href]").unwrap();

    // Collect all links, dedup by target_url preferring body context
    let mut best: HashMap<String, ExtractedLink> = HashMap::new();

    for element in document.select(&anchor_selector) {
        let href = match element.value().attr("href") {
            Some(h) => h.trim(),
            None => continue,
        };

        // Skip non-navigable hrefs
        if href.is_empty()
            || href.starts_with('#')
            || href.starts_with("javascript:")
            || href.starts_with("mailto:")
            || href.starts_with("tel:")
            || href.starts_with("data:")
            || href.starts_with("blob:")
        {
            continue;
        }

        // Resolve relative URLs
        let resolved = match base.join(href) {
            Ok(u) => u,
            Err(_) => continue,
        };

        // Only http/https
        if resolved.scheme() != "http" && resolved.scheme() != "https" {
            continue;
        }

        // Normalize
        let target_url = match normalize_url(resolved.as_str()) {
            Ok(n) => n,
            Err(_) => resolved.to_string(),
        };

        // Skip self-links
        if target_url == normalized_base {
            continue;
        }

        let section = detect_section(&element);
        let anchor_text = get_anchor_text(&element);
        let surrounding_text = get_surrounding_text(&element);

        let link = ExtractedLink {
            target_url: target_url.clone(),
            anchor_text,
            surrounding_text,
            section: Some(section.clone()),
        };

        // Dedup: prefer body context over nav/header/footer/sidebar
        match best.get(&target_url) {
            Some(existing) => {
                let existing_section = existing.section.as_deref().unwrap_or("body");
                if existing_section != "body" && section == "body" {
                    best.insert(target_url, link);
                } else if existing_section == "body" && section == "body" {
                    // Both body: keep the one with longer surrounding_text
                    let existing_len = existing.surrounding_text.as_ref().map_or(0, |s| s.len());
                    let new_len = link.surrounding_text.as_ref().map_or(0, |s| s.len());
                    if new_len > existing_len {
                        best.insert(target_url, link);
                    }
                }
            }
            None => {
                best.insert(target_url, link);
            }
        }
    }

    let mut links: Vec<ExtractedLink> = best.into_values().collect();
    links.truncate(MAX_LINKS_PER_PAGE);
    links
}

/// Detect the semantic section of an anchor element by walking up its ancestors.
fn detect_section(element: &ElementRef) -> String {
    let mut current = element.parent();
    while let Some(node) = current {
        if let Some(el) = ElementRef::wrap(node) {
            match el.value().name() {
                "nav" => return "nav".to_string(),
                "header" => return "header".to_string(),
                "footer" => return "footer".to_string(),
                "aside" => return "sidebar".to_string(),
                _ => {}
            }
        }
        current = node.parent();
    }
    "body".to_string()
}

/// Get the visible text content of an anchor element.
fn get_anchor_text(element: &ElementRef) -> Option<String> {
    let text: String = element.text().collect::<Vec<_>>().join(" ");
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        // Fall back to alt text from child images
        let img_selector = Selector::parse("img[alt]").unwrap();
        element
            .select(&img_selector)
            .next()
            .and_then(|img| img.value().attr("alt"))
            .map(|alt| alt.trim().to_string())
            .filter(|s| !s.is_empty())
    } else {
        Some(trimmed)
    }
}

/// Get surrounding text context (~200 chars centered on the link).
/// Walks up to the nearest block-level parent and extracts its text content.
fn get_surrounding_text(element: &ElementRef) -> Option<String> {
    let block_elements = [
        "p",
        "div",
        "li",
        "td",
        "th",
        "dd",
        "dt",
        "blockquote",
        "section",
        "article",
    ];

    // Walk up to find nearest block-level parent
    let mut current = element.parent();
    let mut block_parent: Option<ElementRef> = None;

    while let Some(node) = current {
        if let Some(el) = ElementRef::wrap(node) {
            if block_elements.contains(&el.value().name()) {
                block_parent = Some(el);
                break;
            }
        }
        current = node.parent();
    }

    let parent = block_parent?;
    let full_text: String = parent.text().collect::<Vec<_>>().join(" ");
    let full_text = full_text.trim();

    if full_text.is_empty() {
        return None;
    }

    // If short enough, return the whole thing
    if full_text.len() <= 200 {
        return Some(full_text.to_string());
    }

    // Find the anchor text position and extract ~200 chars centered on it
    let anchor_text: String = element.text().collect::<Vec<_>>().join(" ");
    let anchor_text = anchor_text.trim();

    if let Some(pos) = full_text.find(anchor_text) {
        let start = pos.saturating_sub(100);
        let end = (pos + anchor_text.len() + 100).min(full_text.len());

        // Align to char boundaries
        let start = full_text.floor_char_boundary(start);
        let end = full_text.ceil_char_boundary(end);

        Some(full_text[start..end].trim().to_string())
    } else {
        // Fallback: first 200 chars
        let end = full_text.ceil_char_boundary(200.min(full_text.len()));
        Some(full_text[..end].trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_link_extraction() {
        let html = r#"
            <html><body>
                <p>Check out <a href="https://example.com/about">our about page</a> for more info.</p>
            </body></html>
        "#;
        let links = extract_links_with_context(html, "https://example.com");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target_url, "https://example.com/about");
        assert_eq!(links[0].anchor_text.as_deref(), Some("our about page"));
        assert_eq!(links[0].section.as_deref(), Some("body"));
    }

    #[test]
    fn test_filters_self_links() {
        let html = r#"
            <html><body>
                <a href="https://example.com">Home</a>
                <a href="https://example.com/other">Other</a>
            </body></html>
        "#;
        let links = extract_links_with_context(html, "https://example.com");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target_url, "https://example.com/other");
    }

    #[test]
    fn test_filters_non_http() {
        let html = r#"
            <html><body>
                <a href="mailto:test@example.com">Email</a>
                <a href="tel:555-1234">Call</a>
                <a href="javascript:void(0)">Click</a>
                <a href="ftp://files.example.com">FTP</a>
                <a href="https://example.com/real">Real link</a>
            </body></html>
        "#;
        let links = extract_links_with_context(html, "https://base.com");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target_url, "https://example.com/real");
    }

    #[test]
    fn test_resolves_relative_urls() {
        let html = r#"
            <html><body>
                <a href="/about">About</a>
                <a href="contact">Contact</a>
            </body></html>
        "#;
        let links = extract_links_with_context(html, "https://example.com/page");
        assert_eq!(links.len(), 2);
        let urls: Vec<&str> = links.iter().map(|l| l.target_url.as_str()).collect();
        assert!(urls.contains(&"https://example.com/about"));
        assert!(urls.contains(&"https://example.com/contact"));
    }

    #[test]
    fn test_section_detection() {
        let html = r#"
            <html>
            <body>
                <nav><a href="https://example.com/nav">Nav Link</a></nav>
                <header><a href="https://example.com/header">Header Link</a></header>
                <main><a href="https://example.com/main">Main Link</a></main>
                <footer><a href="https://example.com/footer">Footer Link</a></footer>
                <aside><a href="https://example.com/aside">Sidebar Link</a></aside>
            </body>
            </html>
        "#;
        let links = extract_links_with_context(html, "https://base.com");
        let by_url: HashMap<&str, &ExtractedLink> =
            links.iter().map(|l| (l.target_url.as_str(), l)).collect();

        assert_eq!(
            by_url["https://example.com/nav"].section.as_deref(),
            Some("nav")
        );
        assert_eq!(
            by_url["https://example.com/header"].section.as_deref(),
            Some("header")
        );
        assert_eq!(
            by_url["https://example.com/main"].section.as_deref(),
            Some("body")
        );
        assert_eq!(
            by_url["https://example.com/footer"].section.as_deref(),
            Some("footer")
        );
        assert_eq!(
            by_url["https://example.com/aside"].section.as_deref(),
            Some("sidebar")
        );
    }

    #[test]
    fn test_dedup_prefers_body() {
        let html = r#"
            <html>
            <body>
                <nav><a href="https://example.com/page">Nav version</a></nav>
                <p><a href="https://example.com/page">Body version with more context around it</a></p>
            </body>
            </html>
        "#;
        let links = extract_links_with_context(html, "https://base.com");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].section.as_deref(), Some("body"));
    }

    #[test]
    fn test_surrounding_text() {
        let html = r#"
            <html><body>
                <p>The Springfield Community Center is hosting a food drive. Visit <a href="https://scc.org">their website</a> for details on how to contribute.</p>
            </body></html>
        "#;
        let links = extract_links_with_context(html, "https://base.com");
        assert_eq!(links.len(), 1);
        let surrounding = links[0].surrounding_text.as_ref().unwrap();
        assert!(surrounding.contains("food drive"));
        assert!(surrounding.contains("their website"));
    }

    #[test]
    fn test_image_alt_as_anchor_text() {
        let html = r#"
            <html><body>
                <p><a href="https://example.com/logo"><img src="logo.png" alt="Company Logo"></a></p>
            </body></html>
        "#;
        let links = extract_links_with_context(html, "https://base.com");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].anchor_text.as_deref(), Some("Company Logo"));
    }

    #[test]
    fn test_caps_at_500_links() {
        let mut html = String::from("<html><body>");
        for i in 0..600 {
            html.push_str(&format!(
                r#"<a href="https://example.com/page/{i}">Link {i}</a>"#
            ));
        }
        html.push_str("</body></html>");

        let links = extract_links_with_context(&html, "https://base.com");
        assert!(links.len() <= 500);
    }

    #[test]
    fn test_empty_html() {
        let links = extract_links_with_context("", "https://base.com");
        assert!(links.is_empty());
    }

    #[test]
    fn test_invalid_base_url() {
        let html = r#"<html><body><a href="https://example.com">Link</a></body></html>"#;
        let links = extract_links_with_context(html, "not-a-url");
        assert!(links.is_empty());
    }
}
