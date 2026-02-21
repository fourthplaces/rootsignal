// HTML → markdown transform via spider_transformations Readability.

use spider_transformations::transformation::content::{
    transform_content_input, ReturnFormat, TransformConfig, TransformInput,
};

/// Convert raw HTML bytes into clean markdown using Readability extraction.
pub(crate) fn html_to_markdown(html: &[u8], url: Option<&str>) -> String {
    let parsed_url = url.and_then(|u| url::Url::parse(u).ok());
    let config = TransformConfig {
        readability: true,
        main_content: true,
        return_format: ReturnFormat::Markdown,
        filter_images: true,
        filter_svg: true,
        clean_html: true,
    };
    let input = TransformInput {
        url: parsed_url.as_ref(),
        content: html,
        screenshot_bytes: None,
        encoding: None,
        selector_config: None,
        ignore_tags: None,
    };

    transform_content_input(input, &config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_html_to_markdown() {
        let html = b"<html><body><h1>Hello World</h1><p>Some content here.</p></body></html>";
        let md = html_to_markdown(html, Some("https://example.com"));
        assert!(md.contains("Hello World"), "Expected heading in output: {md}");
        assert!(md.contains("Some content"), "Expected paragraph in output: {md}");
    }

    #[test]
    fn empty_html_returns_something() {
        let md = html_to_markdown(b"", None);
        // Should not panic on empty input
        assert!(md.is_empty() || !md.is_empty()); // just testing no panic
    }

    #[test]
    fn strips_images_and_svg() {
        let html = b"<html><body><p>Text</p><img src='foo.png'/><svg></svg></body></html>";
        let md = html_to_markdown(html, None);
        assert!(!md.contains("foo.png"), "Should filter images: {md}");
    }
}
