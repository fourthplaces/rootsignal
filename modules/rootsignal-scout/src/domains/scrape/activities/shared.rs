//! Shared helpers for scrape activities.

use chrono::{DateTime, Utc};

use rootsignal_common::{Node, NodeType, Post};

/// Format a post for LLM extraction with optional permalink.
pub(crate) fn format_post(index: usize, post: &Post) -> String {
    let text = post.text.as_deref().unwrap_or("");
    match &post.permalink {
        Some(url) => format!("--- Post {} ({}) ---\n{}", index + 1, url, text),
        None => format!("--- Post {} ---\n{}", index + 1, text),
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
