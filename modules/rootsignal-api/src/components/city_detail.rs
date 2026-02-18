use dioxus::prelude::*;

use super::{CityView, NodeView, StoryView};
use super::layout::Layout;
use crate::templates::render_to_html;

fn signal_badge_classes(type_class: &str) -> &'static str {
    match type_class {
        "event" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-blue-50 text-blue-800",
        "give" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-green-50 text-green-800",
        "ask" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-orange-50 text-orange-800",
        "notice" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-purple-50 text-purple-800",
        "tension" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-red-50 text-red-800",
        _ => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-gray-100 text-gray-600",
    }
}

fn status_badge_classes(status: &str) -> &'static str {
    match status {
        "confirmed" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-green-50 text-green-800",
        "emerging" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-blue-50 text-blue-800",
        _ => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-gray-100 text-gray-600",
    }
}

fn arc_badge_classes(arc: &str) -> &'static str {
    match arc {
        "growing" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-emerald-50 text-emerald-800",
        "stable" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-gray-100 text-gray-600",
        "fading" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-amber-50 text-amber-800",
        "emerging" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-blue-50 text-blue-800",
        _ => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-gray-100 text-gray-600",
    }
}

#[allow(non_snake_case)]
#[component]
fn CityDetail(
    city: CityView,
    tab: String,
    signals: Vec<NodeView>,
    stories: Vec<StoryView>,
) -> Element {
    let signals_active = tab == "signals";
    let stories_active = tab == "stories";

    let tab_active = "px-4 py-2 text-sm font-medium text-blue-600 border-b-2 border-blue-600";
    let tab_inactive = "px-4 py-2 text-sm font-medium text-gray-500 hover:text-gray-700 border-b-2 border-transparent";

    rsx! {
        Layout { title: city.name.clone(), active_page: "cities".to_string(),
            div { class: "max-w-4xl mx-auto p-6",
                a { href: "/admin/cities", class: "text-sm text-blue-600 no-underline",
                    "\u{2190} Back to cities"
                }

                // City header
                div { class: "bg-white border border-gray-200 rounded-lg p-4 mt-3 mb-4",
                    div { class: "flex items-center justify-between",
                        div {
                            div { class: "flex items-center gap-2",
                                h2 { class: "text-xl font-semibold", "{city.name}" }
                                span { class: "text-xs text-gray-400", "({city.slug})" }
                                if city.active {
                                    span { class: "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-green-50 text-green-800",
                                        "active"
                                    }
                                }
                            }
                            div { class: "flex gap-3 text-xs text-gray-400 mt-1",
                                span { "Center: {city.center_lat:.4}, {city.center_lng:.4}" }
                                span { "Radius: {city.radius_km:.0} km" }
                                if !city.geo_terms.is_empty() {
                                    span { "Terms: {city.geo_terms}" }
                                }
                            }
                        }
                        div {
                            if city.scout_running {
                                span { class: "inline-flex items-center gap-1 text-xs text-amber-700 bg-amber-50 px-3 py-1.5 rounded",
                                    "Scout running\u{2026}"
                                }
                            } else {
                                form { method: "POST", action: "/admin/cities/{city.slug}/scout", class: "inline",
                                    button {
                                        r#type: "submit",
                                        class: "px-4 py-1.5 bg-indigo-600 text-white rounded text-sm cursor-pointer hover:bg-indigo-800",
                                        "Run Scout"
                                    }
                                }
                            }
                        }
                    }
                }

                // Tabs
                div { class: "flex gap-0 border-b border-gray-200 mb-4",
                    a {
                        href: "/admin/cities/{city.slug}?tab=signals",
                        class: if signals_active { tab_active } else { tab_inactive },
                        "Signals"
                    }
                    a {
                        href: "/admin/cities/{city.slug}?tab=stories",
                        class: if stories_active { tab_active } else { tab_inactive },
                        "Stories"
                    }
                }

                // Tab content
                if signals_active {
                    if signals.is_empty() {
                        p { class: "text-gray-400 text-center py-10",
                            "No signals found yet. Run the scout to populate the graph."
                        }
                    }
                    for node in signals.iter() {
                        div { class: "bg-white border border-gray-200 rounded-lg p-4 mb-3 hover:border-gray-400",
                            div {
                                span { class: signal_badge_classes(&node.type_class), "{node.type_label}" }
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

                if stories_active {
                    if stories.is_empty() {
                        p { class: "text-gray-400 text-center py-10",
                            "No stories yet. Stories emerge when scout clusters related signals."
                        }
                    }
                    for story in stories.iter() {
                        div { class: "bg-white border border-gray-200 rounded-lg p-4 mb-3 hover:border-gray-400",
                            div { class: "flex gap-2 items-center",
                                span { class: status_badge_classes(&story.status), "{story.status}" }
                                if let Some(arc) = &story.arc {
                                    span { class: arc_badge_classes(arc), "{arc}" }
                                }
                                if let Some(cat) = &story.category {
                                    span { class: "text-xs text-gray-400", "{cat}" }
                                }
                            }
                            h3 { class: "text-base mt-1",
                                a { href: "/admin/stories/{story.id}", class: "text-gray-900 hover:text-blue-600 no-underline",
                                    "{story.headline}"
                                }
                            }
                            p { class: "text-sm text-gray-500 mt-1", "{story.summary}" }
                            div { class: "flex gap-3 items-center text-xs text-gray-400 mt-2",
                                {
                                    let sig = story.signal_count;
                                    let s = if sig != 1 { "s" } else { "" };
                                    rsx! { span { "{sig} signal{s}" } }
                                }
                                {
                                    let src = story.source_count;
                                    let s = if src != 1 { "s" } else { "" };
                                    rsx! { span { "{src} source{s}" } }
                                }
                                {
                                    let e = (story.energy * 100.0).round() as u32;
                                    rsx! { span { "energy {e}%" } }
                                }
                                span { "Updated {story.last_updated}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn render_city_detail(
    city: CityView,
    tab: String,
    signals: Vec<NodeView>,
    stories: Vec<StoryView>,
) -> String {
    let mut dom = VirtualDom::new_with_props(
        CityDetail,
        CityDetailProps { city, tab, signals, stories },
    );
    dom.rebuild_in_place();
    render_to_html(&dom)
}
