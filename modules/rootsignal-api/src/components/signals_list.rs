use dioxus::prelude::*;

use super::NodeView;
use crate::templates::build_page;

#[allow(non_snake_case)]
#[component]
fn SignalsList(nodes: Vec<NodeView>) -> Element {
    rsx! {
        div { class: "container",
            h2 { style: "margin-bottom:16px;", "Recent Civic Signals" }
            if nodes.is_empty() {
                p { style: "color:#888;text-align:center;padding:40px;",
                    "No signals found yet. Run the scout to populate the graph."
                }
            }
            for node in nodes.iter() {
                div { class: "node-card",
                    div {
                        span { class: "badge badge-{node.type_class}", "{node.type_label}" }
                        if node.confidence < 0.6 {
                            span { style: "font-size:11px;color:#795548;margin-left:8px;",
                                "Limited verification"
                            }
                        }
                    }
                    h3 {
                        a { href: "/admin/nodes/{node.id}", "{node.title}" }
                    }
                    p { class: "summary", "{node.summary}" }
                    div { class: "meta-row",
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
                                    span {
                                        style: "color:#e65100;font-size:11px;font-weight:600;",
                                        title: "Cause heat: community attention in this signal's neighborhood",
                                        "cause heat {pct}%"
                                    }
                                }
                            }
                        }
                        if !node.action_url.is_empty() {
                            a {
                                href: "{node.action_url}",
                                class: "action-btn",
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

pub fn render_signals_list(nodes: Vec<NodeView>) -> String {
    let mut dom = VirtualDom::new_with_props(SignalsList, SignalsListProps { nodes });
    dom.rebuild_in_place();
    build_page("Signals", &dioxus::ssr::render(&dom))
}
