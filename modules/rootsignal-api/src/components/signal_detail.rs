use dioxus::prelude::*;

use super::{EvidenceView, NodeView, ResponseView};
use super::layout::Layout;
use crate::templates::render_to_html;

fn badge_classes(type_class: &str) -> &'static str {
    match type_class {
        "event" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-blue-50 text-blue-800",
        "give" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-green-50 text-green-800",
        "ask" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-orange-50 text-orange-800",
        "notice" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-purple-50 text-purple-800",
        "tension" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-red-50 text-red-800",
        _ => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-gray-100 text-gray-600",
    }
}

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
        Layout { title: node.title.clone(), active_page: "signals".to_string(),
            div { class: "max-w-4xl mx-auto p-6",
                a { href: "/admin/nodes", class: "text-sm text-blue-600 no-underline",
                    "\u{2190} Back to signals"
                }
                div { class: "bg-white border border-gray-200 rounded-lg p-4 mt-3",
                    span { class: badge_classes(&node.type_class), "{node.type_label}" }
                    h2 { class: "text-xl font-semibold my-2", "{node.title}" }
                    p { class: "text-gray-500 text-[15px]", "{node.summary}" }

                    if node.confidence < 0.6 {
                        div { class: "bg-amber-50 border border-amber-200 px-3 py-2 rounded text-sm text-amber-800 mt-3",
                            "This signal has limited verification. It may be incomplete or not yet corroborated by multiple sources."
                        }
                    }

                    if let Some(cat) = &node.tension_category {
                        span { class: "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-red-50 text-red-800 mr-2 mt-2",
                            "{cat}"
                        }
                    }
                    if let Some(help) = &node.tension_what_would_help {
                        div { class: "bg-orange-50 border-l-[3px] border-orange-600 px-3.5 py-2.5 my-3 rounded text-sm text-gray-700",
                            strong { "What would help: " }
                            "{help}"
                        }
                    }

                    if !node.action_url.is_empty() {
                        a {
                            href: "{node.action_url}",
                            class: "inline-block px-4 py-1.5 bg-blue-600 text-white rounded text-sm font-medium hover:bg-blue-800 no-underline my-3",
                            target: "_blank",
                            rel: "noopener",
                            "Take Action"
                        }
                    } else {
                        p { class: "text-sm text-gray-400 my-3 p-2 bg-gray-50 rounded",
                            "This is context \u{2014} here\u{2019}s what\u{2019}s happening in your community."
                        }
                    }

                    dl { class: "grid grid-cols-2 gap-2 my-4 text-sm",
                        dt { class: "text-gray-400", "Last verified" }
                        dd { class: "text-gray-700", "{node.last_confirmed}" }
                        dt { class: "text-gray-400", "Corroboration" }
                        dd { class: "text-gray-700", "{corr_label}" }
                        dt { class: "text-gray-400", "Completeness" }
                        dd { class: "text-gray-700", "{node.completeness_label}" }
                    }

                    if !evidence.is_empty() {
                        div { class: "mt-3 pt-3 border-t border-gray-100",
                            h4 { class: "text-sm text-gray-500 mb-1.5", "Sources" }
                            for ev in evidence.iter() {
                                {
                                    let label = source_label(&ev.source_url);
                                    rsx! {
                                        a { href: "{ev.source_url}", target: "_blank", rel: "noopener",
                                            class: "text-sm text-blue-600 block mb-1",
                                            "{label}"
                                        }
                                    }
                                }
                                if let Some(rel) = &ev.relevance {
                                    {
                                        let color = match rel.as_str() {
                                            "contradicting" => "text-red-800",
                                            "direct" => "text-green-800",
                                            _ => "text-amber-800",
                                        };
                                        rsx! {
                                            span { class: "text-xs font-semibold {color} ml-1.5",
                                                "{rel}"
                                            }
                                        }
                                    }
                                }
                                if let Some(c) = ev.evidence_confidence {
                                    if c > 0.0 {
                                        {
                                            let pct = (c * 100.0).round() as u32;
                                            rsx! {
                                                span { class: "text-xs text-gray-400 ml-1",
                                                    "({pct}%)"
                                                }
                                            }
                                        }
                                    }
                                }
                                if let Some(snippet) = &ev.snippet {
                                    p { class: "text-xs text-gray-500 mt-0.5 mb-2 leading-relaxed",
                                        "{snippet}"
                                    }
                                }
                            }
                        }
                    }

                    if !responses.is_empty() {
                        div { class: "mt-4 pt-3 border-t border-gray-100",
                            h4 { class: "text-sm text-gray-500 mb-2", "Responses" }
                            for r in responses.iter() {
                                {
                                    let strength_pct = (r.match_strength * 100.0).round() as u32;
                                    rsx! {
                                        a {
                                            href: "/admin/nodes/{r.id}",
                                            class: "block p-3 mb-1.5 bg-gray-50 border border-gray-200 rounded-md no-underline text-gray-900",
                                            span { class: "{badge_classes(&r.type_class)} mr-2",
                                                "{r.type_label}"
                                            }
                                            span { class: "text-sm", "{r.title}" }
                                            span { class: "text-xs text-gray-400 ml-1.5", "{strength_pct}% match" }
                                            if !r.explanation.is_empty() {
                                                div { class: "text-xs text-gray-500 mt-1 pl-1",
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
}

pub fn render_signal_detail(
    node: NodeView,
    evidence: Vec<EvidenceView>,
    responses: Vec<ResponseView>,
) -> String {
    let mut dom = VirtualDom::new_with_props(
        SignalDetail,
        SignalDetailProps { node, evidence, responses },
    );
    dom.rebuild_in_place();
    render_to_html(&dom)
}
