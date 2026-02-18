use dioxus::prelude::*;

use super::StoryView;
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

#[allow(non_snake_case)]
#[component]
fn StoriesList(stories: Vec<StoryView>) -> Element {
    rsx! {
        Layout { title: "Stories".to_string(), active_page: "stories".to_string(),
            div { class: "max-w-4xl mx-auto p-6",
                h2 { class: "text-xl font-semibold mb-4", "Stories" }
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

pub fn render_stories_list(stories: Vec<StoryView>) -> String {
    let mut dom = VirtualDom::new_with_props(StoriesList, StoriesListProps { stories });
    dom.rebuild_in_place();
    render_to_html(&dom)
}
