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
