use dioxus::prelude::*;

use super::layout::Layout;
use crate::templates::render_to_html;

#[derive(Clone, PartialEq)]
pub struct CityView {
    pub name: String,
    pub slug: String,
    pub center_lat: f64,
    pub center_lng: f64,
    pub radius_km: f64,
    pub geo_terms: String,
    pub active: bool,
    pub scout_running: bool,
    pub source_count: u32,
    pub signal_count: u32,
    pub last_scout_completed: Option<String>,
    pub sources_due: u32,
}

#[allow(non_snake_case)]
#[component]
fn CitiesList(cities: Vec<CityView>) -> Element {
    rsx! {
        Layout { title: "Cities".to_string(), active_page: "cities".to_string(),
            div { class: "max-w-4xl mx-auto p-6",
                h2 { class: "text-xl font-semibold mb-4", "Cities" }

                div { class: "bg-white border border-gray-200 rounded-lg p-4 mb-6",
                    h3 { class: "font-semibold mb-3", "Add City" }
                    form { method: "POST", action: "/admin/cities",
                        div { class: "flex gap-3 items-end",
                            div { class: "flex-1",
                                label { r#for: "location", class: "block text-sm text-gray-500 mb-1", "City" }
                                input {
                                    r#type: "text", name: "location", id: "location", required: true,
                                    class: "w-full px-3 py-2 border border-gray-300 rounded text-sm",
                                    placeholder: "Minneapolis, Minnesota"
                                }
                            }
                            button {
                                r#type: "submit",
                                class: "px-6 py-2 bg-blue-600 text-white rounded text-sm cursor-pointer hover:bg-blue-800",
                                "Add City"
                            }
                        }
                    }
                }

                if cities.is_empty() {
                    p { class: "text-gray-400 text-center py-10",
                        "No cities yet. Add one above to get started."
                    }
                }
                for city in cities.iter() {
                    div { class: "bg-white border border-gray-200 rounded-lg p-4 mb-3 hover:border-gray-400",
                        div { class: "flex items-center gap-2 mb-1",
                            h3 { class: "text-base font-semibold",
                                a { href: "/admin/cities/{city.slug}", class: "text-gray-900 hover:text-blue-600 no-underline",
                                    "{city.name}"
                                }
                            }
                            span { class: "text-xs text-gray-400", "({city.slug})" }
                            if city.active {
                                span { class: "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-green-50 text-green-800",
                                    "active"
                                }
                            } else {
                                span { class: "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-gray-100 text-gray-500",
                                    "inactive"
                                }
                            }
                        }
                        div { class: "flex gap-3 items-center text-xs text-gray-400 mt-1",
                            span { class: "inline-flex items-center gap-1 px-2 py-0.5 rounded bg-blue-50 text-blue-700 font-semibold",
                                "{city.signal_count} signals"
                            }
                            span { class: "inline-flex items-center gap-1 px-2 py-0.5 rounded bg-indigo-50 text-indigo-700 font-semibold",
                                "{city.source_count} sources"
                            }
                            if let Some(last) = &city.last_scout_completed {
                                span { class: "inline-flex items-center gap-1 px-2 py-0.5 rounded bg-gray-100 text-gray-600 font-semibold",
                                    "Last scout: {last}"
                                }
                            } else {
                                span { class: "inline-flex items-center gap-1 px-2 py-0.5 rounded bg-gray-100 text-gray-400 font-semibold",
                                    "Never scouted"
                                }
                            }
                            if city.sources_due > 0 {
                                {
                                    let due = city.sources_due;
                                    let s = if due != 1 { "s" } else { "" };
                                    rsx! {
                                        span { class: "inline-flex items-center gap-1 px-2 py-0.5 rounded bg-amber-50 text-amber-700 font-semibold",
                                            "{due} source{s} due"
                                        }
                                    }
                                }
                            }
                        }
                        div { class: "flex gap-3 items-center text-xs text-gray-400 mt-1",
                            span { "Center: {city.center_lat:.4}, {city.center_lng:.4}" }
                            span { "Radius: {city.radius_km:.0} km" }
                            if !city.geo_terms.is_empty() {
                                span { "Terms: {city.geo_terms}" }
                            }
                        }
                        div { class: "mt-2 flex gap-2 items-center",
                            if city.scout_running {
                                span { class: "inline-flex items-center gap-1 text-xs text-amber-700 bg-amber-50 px-3 py-1 rounded",
                                    "Scout running\u{2026}"
                                }
                                form { method: "POST", action: "/admin/cities/{city.slug}/scout/stop", class: "inline",
                                    button {
                                        r#type: "submit",
                                        class: "px-3 py-1 bg-red-600 text-white rounded text-xs cursor-pointer hover:bg-red-800",
                                        "Stop"
                                    }
                                }
                                form { method: "POST", action: "/admin/cities/{city.slug}/scout/reset", class: "inline",
                                    button {
                                        r#type: "submit",
                                        class: "px-3 py-1 border border-gray-300 text-gray-600 rounded text-xs cursor-pointer hover:bg-gray-100",
                                        "Reset Lock"
                                    }
                                }
                            } else {
                                form { method: "POST", action: "/admin/cities/{city.slug}/scout", class: "inline",
                                    button {
                                        r#type: "submit",
                                        class: "px-3 py-1 bg-indigo-600 text-white rounded text-xs cursor-pointer hover:bg-indigo-800",
                                        "Run Scout"
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

pub fn render_cities(cities: Vec<CityView>) -> String {
    let mut dom = VirtualDom::new_with_props(CitiesList, CitiesListProps { cities });
    dom.rebuild_in_place();
    render_to_html(&dom)
}
