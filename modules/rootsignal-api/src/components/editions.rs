use dioxus::prelude::*;

use super::layout::Layout;
use super::StoryView;
use crate::templates::render_to_html;

#[derive(Clone, PartialEq)]
pub struct EditionView {
    pub id: String,
    pub period: String,
    pub story_count: u32,
    pub signal_count: u32,
    pub generated_at: String,
    pub editorial_summary: String,
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
fn EditionsList(editions: Vec<EditionView>) -> Element {
    rsx! {
        Layout { title: "Editions".to_string(), active_page: "editions".to_string(),
            div { class: "max-w-6xl mx-auto p-6",
                h2 { class: "text-xl font-semibold mb-4", "Editions" }

                if editions.is_empty() {
                    p { class: "text-gray-400 text-center py-10",
                        "No editions yet. Editions are generated periodically from clustered stories."
                    }
                } else {
                    div { class: "bg-white border border-gray-200 rounded-lg overflow-hidden",
                        table { class: "w-full text-sm",
                            thead {
                                tr { class: "bg-gray-50",
                                    th { class: "text-left px-4 py-3 text-gray-500 font-medium", "Period" }
                                    th { class: "text-right px-4 py-3 text-gray-500 font-medium", "Stories" }
                                    th { class: "text-right px-4 py-3 text-gray-500 font-medium", "Signals" }
                                    th { class: "text-left px-4 py-3 text-gray-500 font-medium", "Generated" }
                                }
                            }
                            tbody {
                                for edition in editions.iter() {
                                    tr { class: "border-t border-gray-100 hover:bg-gray-50",
                                        td { class: "px-4 py-3",
                                            a { href: "/admin/editions/{edition.id}", class: "text-gray-900 hover:text-blue-600 no-underline font-medium",
                                                "{edition.period}"
                                            }
                                        }
                                        td { class: "text-right px-4 py-3", "{edition.story_count}" }
                                        td { class: "text-right px-4 py-3", "{edition.signal_count}" }
                                        td { class: "px-4 py-3 text-gray-500", "{edition.generated_at}" }
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

#[allow(non_snake_case)]
#[component]
fn EditionDetail(edition: EditionView, stories: Vec<StoryView>) -> Element {
    rsx! {
        Layout { title: format!("Edition: {}", edition.period), active_page: "editions".to_string(),
            div { class: "max-w-4xl mx-auto p-6",
                a { href: "/admin/editions", class: "text-sm text-blue-600 no-underline",
                    "\u{2190} Back to editions"
                }

                // Edition info card
                div { class: "bg-white border border-gray-200 rounded-lg p-4 mt-3 mb-4",
                    h2 { class: "text-xl font-semibold mb-2", "{edition.period}" }
                    div { class: "grid grid-cols-3 gap-3 text-sm mb-3",
                        div {
                            div { class: "text-xs text-gray-400", "Stories" }
                            div { class: "font-semibold", "{edition.story_count}" }
                        }
                        div {
                            div { class: "text-xs text-gray-400", "Signals" }
                            div { class: "font-semibold", "{edition.signal_count}" }
                        }
                        div {
                            div { class: "text-xs text-gray-400", "Generated" }
                            div { class: "font-semibold", "{edition.generated_at}" }
                        }
                    }
                    if !edition.editorial_summary.is_empty() {
                        div { class: "border-t border-gray-100 pt-3 mt-3",
                            h3 { class: "text-sm font-semibold mb-1", "Editorial Summary" }
                            p { class: "text-sm text-gray-600 whitespace-pre-line", "{edition.editorial_summary}" }
                        }
                    }
                }

                // Featured stories
                h3 { class: "text-lg font-semibold mb-3", "Featured Stories" }
                if stories.is_empty() {
                    p { class: "text-gray-400 text-sm", "No featured stories in this edition." }
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

pub fn render_editions_list(editions: Vec<EditionView>) -> String {
    let mut dom = VirtualDom::new_with_props(EditionsList, EditionsListProps { editions });
    dom.rebuild_in_place();
    render_to_html(&dom)
}

pub fn render_edition_detail(edition: EditionView, stories: Vec<StoryView>) -> String {
    let mut dom = VirtualDom::new_with_props(EditionDetail, EditionDetailProps { edition, stories });
    dom.rebuild_in_place();
    render_to_html(&dom)
}
