use dioxus::prelude::*;

use super::{NodeView, StoryView};
use super::layout::Layout;
use crate::templates::render_to_html;

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

#[allow(non_snake_case)]
#[component]
fn StoryDetail(story: StoryView, signals: Vec<NodeView>) -> Element {
    rsx! {
        Layout { title: story.headline.clone(), active_page: "stories".to_string(),
            div { class: "max-w-4xl mx-auto p-6",
                a { href: "/admin/stories", class: "text-sm text-blue-600 no-underline",
                    "\u{2190} Back to stories"
                }
                div { class: "bg-white border border-gray-200 rounded-lg p-4 mt-3",
                    div { class: "flex gap-2 items-center",
                        span { class: status_badge_classes(&story.status), "{story.status}" }
                        if let Some(arc) = &story.arc {
                            span { class: arc_badge_classes(arc), "{arc}" }
                        }
                    }

                    h2 { class: "text-xl font-semibold my-2", "{story.headline}" }

                    if let Some(lede) = &story.lede {
                        p { class: "text-gray-600 text-[15px] italic mb-2", "{lede}" }
                    }

                    p { class: "text-gray-500 text-[15px]", "{story.summary}" }

                    dl { class: "grid grid-cols-2 gap-2 my-4 text-sm",
                        dt { class: "text-gray-400", "Signals" }
                        dd { class: "text-gray-700", "{story.signal_count}" }
                        dt { class: "text-gray-400", "Sources" }
                        dd { class: "text-gray-700", "{story.source_count}" }
                        dt { class: "text-gray-400", "Evidence" }
                        dd { class: "text-gray-700", "{story.evidence_count}" }
                        dt { class: "text-gray-400", "Energy" }
                        dd { class: "text-gray-700",
                            {
                                let e = (story.energy * 100.0).round() as u32;
                                format!("{e}%")
                            }
                        }
                        dt { class: "text-gray-400", "Velocity" }
                        dd { class: "text-gray-700",
                            {format!("{:.2}", story.velocity)}
                        }
                        if let Some(cat) = &story.category {
                            dt { class: "text-gray-400", "Category" }
                            dd { class: "text-gray-700", "{cat}" }
                        }
                        dt { class: "text-gray-400", "Last updated" }
                        dd { class: "text-gray-700", "{story.last_updated}" }
                    }

                    if let Some(narrative) = &story.narrative {
                        div { class: "mt-3 pt-3 border-t border-gray-100",
                            h4 { class: "text-sm text-gray-500 mb-1.5", "Narrative" }
                            p { class: "text-sm text-gray-700 leading-relaxed whitespace-pre-line", "{narrative}" }
                        }
                    }

                    if !signals.is_empty() {
                        div { class: "mt-4 pt-3 border-t border-gray-100",
                            h4 { class: "text-sm text-gray-500 mb-2", "Constituent Signals" }
                            for sig in signals.iter() {
                                a {
                                    href: "/admin/nodes/{sig.id}",
                                    class: "block p-3 mb-1.5 bg-gray-50 border border-gray-200 rounded-md no-underline text-gray-900",
                                    span { class: signal_badge_classes(&sig.type_class), "{sig.type_label}" }
                                    span { class: "text-sm ml-2", "{sig.title}" }
                                    if !sig.summary.is_empty() {
                                        div { class: "text-xs text-gray-500 mt-1 pl-1", "{sig.summary}" }
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

pub fn render_story_detail(story: StoryView, signals: Vec<NodeView>) -> String {
    let mut dom = VirtualDom::new_with_props(
        StoryDetail,
        StoryDetailProps { story, signals },
    );
    dom.rebuild_in_place();
    render_to_html(&dom)
}
