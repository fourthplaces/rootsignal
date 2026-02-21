use ai_client::Claude;
use anyhow::Result;
use rootsignal_common::ContentSemantics;

use crate::archive::Content;

const MAX_EXTRACT_CHARS: usize = 30_000;
pub(crate) const MIN_EXTRACT_CHARS: usize = 50;

const SEMANTICS_SYSTEM_PROMPT: &str = r#"You are a factual content extractor. Given web content, extract ALL factual information into the structured schema provided.

Be thorough and precise. Extract:
- summary: A concise 2-3 sentence summary of the content
- entities: All organizations, people, government bodies, places, events, products mentioned
- locations: Physical locations with addresses and coordinates when available
- contacts: Phone numbers, emails, addresses, URLs for contacting entities
- schedules: Dates, times, recurring events, deadlines
- claims: Factual statements, statistics, quotes, policy positions, announcements
- temporal_markers: Time references ("last Tuesday", "Q3 2025", "next month")
- topics: Subject matter tags
- provenance: Author, publication date, source name
- language: ISO 639-1 code (e.g. "en")
- outbound_links: Important links with their relationship to the content

Be factual. Do not infer or speculate. Only extract what is explicitly stated.
Return empty arrays for categories with no matches."#;

/// Convert any Content variant to text suitable for LLM extraction.
pub(crate) fn extractable_text(content: &Content, source_url: &str) -> String {
    match content {
        Content::Page(page) => {
            format!("Source: {}\n\n{}", source_url, page.markdown)
        }
        Content::SocialPosts(posts) => {
            let mut text = format!("Source: {} (social media posts)\n\n", source_url);
            for post in posts {
                if let Some(ref author) = post.author {
                    text.push_str(&format!("Post by {}:\n", author));
                }
                text.push_str(&post.content);
                if let Some(ref url) = post.url {
                    text.push_str(&format!("\nLink: {}", url));
                }
                text.push_str("\n---\n");
            }
            text
        }
        Content::Feed(items) => {
            let mut text = format!("Source: {} (RSS/Atom feed)\n\n", source_url);
            for item in items {
                text.push_str(&format!("- {}", item.url));
                if let Some(ref title) = item.title {
                    text.push_str(&format!(" | {}", title));
                }
                if let Some(ref date) = item.pub_date {
                    text.push_str(&format!(" ({})", date));
                }
                text.push('\n');
            }
            text
        }
        Content::SearchResults(results) => {
            let mut text = format!("Source: {} (search results)\n\n", source_url);
            for r in results {
                text.push_str(&format!("- {}: {}", r.title, r.snippet));
                text.push_str(&format!(" ({})\n", r.url));
            }
            text
        }
        Content::Pdf(pdf) => {
            if pdf.extracted_text.is_empty() {
                String::new()
            } else {
                format!("Source: {} (PDF)\n\n{}", source_url, pdf.extracted_text)
            }
        }
        Content::Raw(body) => {
            format!("Source: {}\n\n{}", source_url, body)
        }
    }
}

/// Human-readable label for the content type, passed to the LLM for context.
pub(crate) fn content_kind_label(content: &Content) -> &'static str {
    match content {
        Content::Page(_) => "web page",
        Content::SocialPosts(_) => "social media posts",
        Content::Feed(_) => "RSS/Atom feed",
        Content::SearchResults(_) => "search results",
        Content::Pdf(_) => "PDF document",
        Content::Raw(_) => "raw web content",
    }
}

/// Run LLM extraction to produce ContentSemantics.
pub(crate) async fn extract_semantics(
    claude: &Claude,
    text: &str,
    source_url: &str,
    content_kind: &'static str,
) -> Result<ContentSemantics> {
    let truncated = truncate_to_char_boundary(text, MAX_EXTRACT_CHARS);

    let user_prompt = format!(
        "Content type: {}\nSource URL: {}\n\n{}",
        content_kind, source_url, truncated,
    );

    claude
        .extract::<ContentSemantics>("claude-haiku-4-5-20251001", SEMANTICS_SYSTEM_PROMPT, user_prompt)
        .await
}

fn truncate_to_char_boundary(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}
