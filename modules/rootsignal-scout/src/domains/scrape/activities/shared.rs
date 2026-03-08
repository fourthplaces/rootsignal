//! Shared helpers for scrape activities.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use uuid::Uuid;

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
pub(crate) fn format_story(index: usize, story: &Story) -> String {
    let text = story.text.as_deref().unwrap_or("");
    let location = story.location.as_deref().unwrap_or("");
    let loc_suffix = if location.is_empty() {
        String::new()
    } else {
        format!(" [location: {}]", location)
    };
    match &story.permalink {
        Some(url) => format!("--- Story {} ({}) ---\n{}{}", index + 1, url, text, loc_suffix),
        None => format!("--- Story {} ---\n{}{}", index + 1, text, loc_suffix),
    }
}

/// Format a short video (reel/TikTok) for LLM extraction.
pub(crate) fn format_short_video(index: usize, video: &ShortVideo) -> String {
    let text = video.text.as_deref().unwrap_or("");
    let location = video.location.as_deref().unwrap_or("");
    let loc_suffix = if location.is_empty() {
        String::new()
    } else {
        format!(" [location: {}]", location)
    };
    match &video.permalink {
        Some(url) => format!("--- Video {} ({}) ---\n{}{}", index + 1, url, text, loc_suffix),
        None => format!("--- Video {} ---\n{}{}", index + 1, text, loc_suffix),
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
                    meta.source_url = url.clone();
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
