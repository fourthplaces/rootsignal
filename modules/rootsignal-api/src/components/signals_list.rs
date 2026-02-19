use dioxus::prelude::*;

use super::NodeView;
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

#[allow(non_snake_case)]
#[component]
fn SignalsList(nodes: Vec<NodeView>) -> Element {
    rsx! {
        Layout { title: "Signals".to_string(), active_page: "signals".to_string(),
            div { class: "max-w-4xl mx-auto p-6",
                h2 { class: "text-xl font-semibold mb-4", "Recent Signals" }
                if nodes.is_empty() {
                    p { class: "text-gray-400 text-center py-10",
                        "No signals found yet. Run the scout to populate the graph."
                    }
                }
                for node in nodes.iter() {
                    div { class: "bg-white border border-gray-200 rounded-lg p-4 mb-3 hover:border-gray-400",
                        div {
                            span { class: badge_classes(&node.type_class), "{node.type_label}" }
                            if node.confidence < 0.6 {
                                span { class: "text-xs text-amber-700 ml-2", "Limited verification" }
                            }
                        }
                        h3 { class: "text-base mt-1",
                            a { href: "/admin/nodes/{node.id}", class: "text-gray-900 hover:text-blue-600 no-underline",
                                "{node.title}"
                            }
                        }
                        p { class: "text-sm text-gray-500 mt-1", "{node.summary}" }
                        div { class: "flex gap-3 items-center text-xs text-gray-400 mt-2",
                            span { "Verified {node.last_confirmed}" }
                            if node.source_diversity > 1 {
                                {
                                    let s = if node.source_diversity != 2 { "s" } else { "" };
                                    let div = node.source_diversity;
                                    rsx! { span { "{div} independent source{s}" } }
                                }
                            } else if node.corroboration_count > 0 {
                                span { "1 source" }
                            }
                            if node.cause_heat > 0.1 {
                                {
                                    let pct = (node.cause_heat * 100.0).round() as u32;
                                    rsx! {
                                        span { class: "text-orange-600 font-semibold",
                                            title: "Cause heat: community attention in this signal's neighborhood",
                                            "cause heat {pct}%"
                                        }
                                    }
                                }
                            }
                            if !node.action_url.is_empty() {
                                a {
                                    href: "{node.action_url}",
                                    class: "inline-block px-4 py-1.5 bg-blue-600 text-white rounded text-sm font-medium hover:bg-blue-800 no-underline",
                                    target: "_blank",
                                    rel: "noopener",
                                    "Take Action"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn render_signals_list(nodes: Vec<NodeView>) -> String {
    let mut dom = VirtualDom::new_with_props(SignalsList, SignalsListProps { nodes });
    dom.rebuild_in_place();
    render_to_html(&dom)
}
