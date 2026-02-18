use dioxus::prelude::*;

use super::layout::Layout;
use super::StoryView;
use crate::templates::render_to_html;

#[derive(Clone, PartialEq)]
pub struct ActorView {
    pub id: String,
    pub name: String,
    pub actor_type: String,
    pub signal_count: u32,
    pub last_active: String,
    pub domains: String,
    pub city: String,
    pub description: String,
    pub typical_roles: String,
}

fn type_badge_class(actor_type: &str) -> &'static str {
    match actor_type {
        "Organization" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-blue-50 text-blue-800",
        "Person" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-green-50 text-green-800",
        "Place" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-purple-50 text-purple-800",
        "Group" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-amber-50 text-amber-800",
        _ => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-gray-100 text-gray-600",
    }
}

#[allow(non_snake_case)]
#[component]
fn ActorsList(actors: Vec<ActorView>) -> Element {
    rsx! {
        Layout { title: "Actors".to_string(), active_page: "actors".to_string(),
            div { class: "max-w-6xl mx-auto p-6",
                h2 { class: "text-xl font-semibold mb-4", "Actors" }

                if actors.is_empty() {
                    p { class: "text-gray-400 text-center py-10",
                        "No actors found. Actors are discovered when the scout processes signals."
                    }
                } else {
                    div { class: "bg-white border border-gray-200 rounded-lg overflow-hidden",
                        table { class: "w-full text-sm",
                            thead {
                                tr { class: "bg-gray-50",
                                    th { class: "text-left px-4 py-3 text-gray-500 font-medium", "Name" }
                                    th { class: "text-left px-4 py-3 text-gray-500 font-medium", "Type" }
                                    th { class: "text-right px-4 py-3 text-gray-500 font-medium", "Signals" }
                                    th { class: "text-left px-4 py-3 text-gray-500 font-medium", "Last Active" }
                                    th { class: "text-left px-4 py-3 text-gray-500 font-medium", "Domains" }
                                    th { class: "text-left px-4 py-3 text-gray-500 font-medium", "City" }
                                }
                            }
                            tbody {
                                for actor in actors.iter() {
                                    tr { class: "border-t border-gray-100 hover:bg-gray-50",
                                        td { class: "px-4 py-3",
                                            a { href: "/admin/actors/{actor.id}", class: "text-gray-900 hover:text-blue-600 no-underline font-medium",
                                                "{actor.name}"
                                            }
                                        }
                                        td { class: "px-4 py-3",
                                            span { class: type_badge_class(&actor.actor_type), "{actor.actor_type}" }
                                        }
                                        td { class: "text-right px-4 py-3", "{actor.signal_count}" }
                                        td { class: "px-4 py-3 text-gray-500", "{actor.last_active}" }
                                        td { class: "px-4 py-3 text-gray-500 text-xs max-w-48 truncate", "{actor.domains}" }
                                        td { class: "px-4 py-3 text-gray-500", "{actor.city}" }
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
fn ActorDetail(actor: ActorView, stories: Vec<StoryView>) -> Element {
    rsx! {
        Layout { title: actor.name.clone(), active_page: "actors".to_string(),
            div { class: "max-w-4xl mx-auto p-6",
                a { href: "/admin/actors", class: "text-sm text-blue-600 no-underline",
                    "\u{2190} Back to actors"
                }

                // Actor info card
                div { class: "bg-white border border-gray-200 rounded-lg p-4 mt-3 mb-4",
                    div { class: "flex items-center gap-2 mb-2",
                        h2 { class: "text-xl font-semibold", "{actor.name}" }
                        span { class: type_badge_class(&actor.actor_type), "{actor.actor_type}" }
                    }
                    if !actor.description.is_empty() {
                        p { class: "text-sm text-gray-600 mb-3", "{actor.description}" }
                    }
                    div { class: "grid grid-cols-2 md:grid-cols-4 gap-3 text-sm",
                        div {
                            div { class: "text-xs text-gray-400", "Signals" }
                            div { class: "font-semibold", "{actor.signal_count}" }
                        }
                        div {
                            div { class: "text-xs text-gray-400", "Last Active" }
                            div { class: "font-semibold", "{actor.last_active}" }
                        }
                        div {
                            div { class: "text-xs text-gray-400", "City" }
                            div { class: "font-semibold", "{actor.city}" }
                        }
                        if !actor.typical_roles.is_empty() {
                            div {
                                div { class: "text-xs text-gray-400", "Roles" }
                                div { class: "font-semibold text-xs", "{actor.typical_roles}" }
                            }
                        }
                    }
                    if !actor.domains.is_empty() {
                        div { class: "mt-2 text-xs text-gray-400",
                            "Domains: {actor.domains}"
                        }
                    }
                }

                // Stories
                h3 { class: "text-lg font-semibold mb-3", "Stories" }
                if stories.is_empty() {
                    p { class: "text-gray-400 text-sm", "No stories involving this actor." }
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

pub fn render_actors_list(actors: Vec<ActorView>) -> String {
    let mut dom = VirtualDom::new_with_props(ActorsList, ActorsListProps { actors });
    dom.rebuild_in_place();
    render_to_html(&dom)
}

pub fn render_actor_detail(actor: ActorView, stories: Vec<StoryView>) -> String {
    let mut dom = VirtualDom::new_with_props(ActorDetail, ActorDetailProps { actor, stories });
    dom.rebuild_in_place();
    render_to_html(&dom)
}
