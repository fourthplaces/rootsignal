//! Shared helpers for scrape activities.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use rootsignal_common::types::ArchiveFile;
use rootsignal_common::{Node, NodeType, Post, ShortVideo, Story};

/// Format a post for LLM extraction with optional permalink.
pub(crate) fn format_post(index: usize, post: &Post) -> String {
    let text = post.text.as_deref().unwrap_or("");
    match &post.permalink {
        Some(url) => format!("--- Post {} ({}) ---\n{}", index + 1, url, text),
        None => format!("--- Post {} ---\n{}", index + 1, text),
    }
}

/// Format a story for LLM extraction.
/// Includes transcribed/OCR'd text from media attachments alongside any caption.
pub(crate) fn format_story(index: usize, story: &Story) -> String {
    let content = content_with_attachments(story.text.as_deref(), &story.attachments);
    let loc_suffix = location_suffix(story.location.as_deref());
    match &story.permalink {
        Some(url) => format!("--- Story {} ({}) ---\n{}{}", index + 1, url, content, loc_suffix),
        None => format!("--- Story {} ---\n{}{}", index + 1, content, loc_suffix),
    }
}

/// Format a short video (reel/TikTok) for LLM extraction.
/// Includes transcribed/OCR'd text from media attachments alongside any caption.
pub(crate) fn format_short_video(index: usize, video: &ShortVideo) -> String {
    let content = content_with_attachments(video.text.as_deref(), &video.attachments);
    let loc_suffix = location_suffix(video.location.as_deref());
    match &video.permalink {
        Some(url) => format!("--- Video {} ({}) ---\n{}{}", index + 1, url, content, loc_suffix),
        None => format!("--- Video {} ---\n{}{}", index + 1, content, loc_suffix),
    }
}

fn content_with_attachments(caption: Option<&str>, attachments: &[ArchiveFile]) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if let Some(text) = caption.filter(|t| !t.is_empty()) {
        parts.push(text);
    }
    for file in attachments {
        if let Some(ref text) = file.text {
            if !text.is_empty() {
                parts.push(text);
            }
        }
    }
    parts.join("\n\n")
}

fn location_suffix(location: Option<&str>) -> String {
    match location.filter(|l| !l.is_empty()) {
        Some(loc) => format!(" [location: {}]", loc),
        None => String::new(),
    }
}

/// Collect implied queries from Concern and HelpRequest nodes.
pub(crate) fn collect_implied_queries(nodes: &[Node]) -> Vec<String> {
    nodes
        .iter()
        .filter(|n| matches!(n.node_type(), NodeType::Concern | NodeType::HelpRequest))
        .filter_map(|n| n.meta())
        .flat_map(|meta| meta.implied_queries.iter().cloned())
        .collect()
}

/// Overwrite source_url on nodes whose source_id maps to a permalink.
pub(crate) fn resolve_source_ids(
    nodes: &mut [Node],
    source_ids: &[(Uuid, String)],
    permalink_map: &HashMap<String, String>,
) {
    let id_lookup: HashMap<Uuid, &str> = source_ids
        .iter()
        .map(|(id, sid)| (*id, sid.as_str()))
        .collect();
    for node in nodes {
        if let Some(meta) = node.meta_mut() {
            if let Some(sid) = id_lookup.get(&meta.id) {
                if let Some(url) = permalink_map.get(*sid) {
                    meta.url = url.clone();
                }
            }
        }
    }
}

/// Apply a fallback published_at to nodes that don't have one.
pub(crate) fn apply_published_at_fallback(nodes: &mut [Node], fallback: DateTime<Utc>) {
    for node in nodes {
        if let Some(meta) = node.meta_mut() {
            if meta.published_at.is_none() {
                meta.published_at = Some(fallback);
            }
        }
    }
}
