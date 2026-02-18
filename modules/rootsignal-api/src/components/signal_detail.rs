use dioxus::prelude::*;

use super::{EvidenceView, NodeView, ResponseView};
use crate::templates::build_page;

fn source_label(url: &str) -> String {
    if url.contains("instagram.com/p/") || url.contains("instagram.com/reel/") {
        "Instagram post".to_string()
    } else if url.contains("instagram.com") {
        "Instagram".to_string()
    } else if url.contains("reddit.com/r/") && url.contains("/comments/") {
        "Reddit post".to_string()
    } else if url.contains("reddit.com") {
        "Reddit".to_string()
    } else if url.contains("facebook.com") {
        "Facebook post".to_string()
    } else if url.contains("tiktok.com") {
        "TikTok".to_string()
    } else if let Some(domain) = url.split('/').nth(2) {
        domain.trim_start_matches("www.").to_string()
    } else {
        "Source".to_string()
    }
}

#[allow(non_snake_case)]
#[component]
fn SignalDetail(
    node: NodeView,
    evidence: Vec<EvidenceView>,
    responses: Vec<ResponseView>,
) -> Element {
    let corr_label = if node.source_diversity > 1 {
        let s = if node.source_diversity != 2 { "s" } else { "" };
        format!("Confirmed by {} independent source{s}", node.source_diversity)
    } else if node.corroboration_count > 0 {
        format!("Seen {} time(s) from 1 source", node.corroboration_count)
    } else {
        "Not yet corroborated".to_string()
    };

    rsx! {
        div { class: "container",
            a { href: "/admin/nodes", style: "font-size:13px;color:#0066cc;text-decoration:none;",
                "\u{2190} Back to signals"
            }
            div { class: "node-card", style: "margin-top:12px;",
                span { class: "badge badge-{node.type_class}", "{node.type_label}" }
                h2 { style: "margin:8px 0;", "{node.title}" }
                p { class: "summary", style: "font-size:15px;", "{node.summary}" }

                // Limited verification banner
                if node.confidence < 0.6 {
                    div { class: "limited-banner",
                        "This signal has limited verification. It may be incomplete or not yet corroborated by multiple sources."
                    }
                }

                // Tension-specific sections
                if let Some(cat) = &node.tension_category {
                    span { class: "badge", style: "background:#fce4ec;color:#c62828;margin-right:8px;",
                        "{cat}"
                    }
                }
                if let Some(help) = &node.tension_what_would_help {
                    div { style: "background:#fff3e0;border-left:3px solid #e65100;padding:10px 14px;margin:12px 0;border-radius:4px;font-size:14px;color:#333;",
                        strong { "What would help: " }
                        "{help}"
                    }
                }

                // Action section
                if !node.action_url.is_empty() {
                    a {
                        href: "{node.action_url}",
                        class: "action-btn",
                        target: "_blank",
                        rel: "noopener",
                        style: "margin:12px 0;display:inline-block;",
                        "Take Action"
                    }
                } else {
                    p { style: "font-size:13px;color:#888;margin:12px 0;padding:8px;background:#f5f5f5;border-radius:4px;",
                        "This is context \u{2014} here\u{2019}s what\u{2019}s happening in your community."
                    }
                }

                // Detail metadata
                dl { class: "detail-meta",
                    dt { "Last verified" }
                    dd { "{node.last_confirmed}" }
                    dt { "Corroboration" }
                    dd { "{corr_label}" }
                    dt { "Completeness" }
                    dd { "{node.completeness_label}" }
                }

                // Evidence section
                if !evidence.is_empty() {
                    div { class: "evidence-list",
                        h4 { "Sources" }
                        for ev in evidence.iter() {
                            {
                                let label = source_label(&ev.source_url);
                                rsx! {
                                    a { href: "{ev.source_url}", target: "_blank", rel: "noopener",
                                        "{label}"
                                    }
                                }
                            }
                            // Relevance badge
                            if let Some(rel) = &ev.relevance {
                                {
                                    let color = match rel.as_str() {
                                        "contradicting" => "#c62828",
                                        "direct" => "#2e7d32",
                                        _ => "#795548",
                                    };
                                    rsx! {
                                        span { style: "font-size:11px;font-weight:600;color:{color};margin-left:6px;",
                                            "{rel}"
                                        }
                                    }
                                }
                            }
                            // Confidence percentage
                            if let Some(c) = ev.evidence_confidence {
                                if c > 0.0 {
                                    {
                                        let pct = (c * 100.0).round() as u32;
                                        rsx! {
                                            span { style: "font-size:11px;color:#999;margin-left:4px;",
                                                "({pct}%)"
                                            }
                                        }
                                    }
                                }
                            }
                            // Snippet
                            if let Some(snippet) = &ev.snippet {
                                p { style: "font-size:12px;color:#666;margin:2px 0 8px 0;line-height:1.4;",
                                    "{snippet}"
                                }
                            }
                        }
                    }
                }

                // Responses section
                if !responses.is_empty() {
                    div { style: "margin-top:16px;padding-top:12px;border-top:1px solid #eee;",
                        h4 { style: "font-size:13px;color:#666;margin-bottom:8px;", "Responses" }
                        for r in responses.iter() {
                            {
                                let strength_pct = (r.match_strength * 100.0).round() as u32;
                                rsx! {
                                    a {
                                        href: "/admin/nodes/{r.id}",
                                        style: "display:block;padding:10px 12px;margin-bottom:6px;background:#f9f9f9;border:1px solid #e0e0e0;border-radius:6px;text-decoration:none;color:#1a1a1a;",
                                        span { class: "badge badge-{r.type_class}", style: "margin-right:8px;",
                                            "{r.type_label}"
                                        }
                                        span { style: "font-size:14px;", "{r.title}" }
                                        span { style: "font-size:11px;color:#999;margin-left:6px;", "{strength_pct}% match" }
                                        if !r.explanation.is_empty() {
                                            div { style: "font-size:12px;color:#666;margin-top:4px;padding-left:4px;",
                                                "{r.explanation}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn render_signal_detail(
    node: NodeView,
    evidence: Vec<EvidenceView>,
    responses: Vec<ResponseView>,
) -> String {
    let title = node.title.clone();
    let mut dom = VirtualDom::new_with_props(
        SignalDetail,
        SignalDetailProps { node, evidence, responses },
    );
    dom.rebuild_in_place();
    build_page(&title, &dioxus::ssr::render(&dom))
}
