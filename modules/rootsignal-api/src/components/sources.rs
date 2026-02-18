use dioxus::prelude::*;

/// View model for a single source.
#[derive(Clone, PartialEq)]
pub struct SourceView {
    pub canonical_key: String,
    pub canonical_value: String,
    pub url: Option<String>,
    pub source_type: String,
    pub is_query: bool,
    pub discovery_method: String,
    pub weight: f64,
    pub quality_penalty: f64,
    pub effective_weight: f64,
    pub cadence_hours: u32,
    pub signals_produced: u32,
    pub signals_corroborated: u32,
    pub consecutive_empty_runs: u32,
    pub last_scraped: Option<String>,
    pub last_produced_signal: Option<String>,
    pub gap_context: Option<String>,
}

/// A source in the schedule preview with its scheduling status.
#[derive(Clone, PartialEq)]
pub struct ScheduledSourceView {
    pub canonical_value: String,
    pub source_type: String,
    pub is_query: bool,
    pub reason: String,
    pub weight: f64,
    pub cadence_hours: u32,
    pub last_scraped: Option<String>,
    pub hours_until_due: Option<i64>,
}

/// Summary of the next run schedule.
#[derive(Clone, PartialEq)]
pub struct SchedulePreview {
    pub scheduled: Vec<ScheduledSourceView>,
    pub exploration: Vec<ScheduledSourceView>,
    pub skipped_count: usize,
    pub total_sources: usize,
}

fn source_type_badge(source_type: &str, is_query: bool) -> &'static str {
    if is_query {
        "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-amber-50 text-amber-800"
    } else {
        match source_type {
            "instagram" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-pink-50 text-pink-800",
            "reddit" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-orange-50 text-orange-800",
            "facebook" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-blue-50 text-blue-800",
            "bluesky" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-sky-50 text-sky-800",
            "twitter" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-gray-100 text-gray-800",
            "tiktok" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-rose-50 text-rose-800",
            _ => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold uppercase bg-green-50 text-green-800",
        }
    }
}

fn discovery_badge(method: &str) -> &'static str {
    match method {
        "curated" => "inline-block px-2 py-0.5 rounded-full text-xs bg-indigo-50 text-indigo-700",
        "cold_start" => "inline-block px-2 py-0.5 rounded-full text-xs bg-gray-100 text-gray-600",
        "gap_analysis" => "inline-block px-2 py-0.5 rounded-full text-xs bg-purple-50 text-purple-700",
        "tension_seed" => "inline-block px-2 py-0.5 rounded-full text-xs bg-red-50 text-red-700",
        "signal_reference" => "inline-block px-2 py-0.5 rounded-full text-xs bg-cyan-50 text-cyan-700",
        "hashtag_discovery" => "inline-block px-2 py-0.5 rounded-full text-xs bg-teal-50 text-teal-700",
        "human_submission" => "inline-block px-2 py-0.5 rounded-full text-xs bg-yellow-50 text-yellow-700",
        _ => "inline-block px-2 py-0.5 rounded-full text-xs bg-gray-100 text-gray-600",
    }
}

fn reason_badge(reason: &str) -> &'static str {
    match reason {
        "Cadence" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold bg-green-50 text-green-800",
        "Never scraped" | "New" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold bg-blue-50 text-blue-800",
        "Exploration" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold bg-amber-50 text-amber-800",
        _ => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold bg-gray-100 text-gray-600",
    }
}

fn weight_bar_color(weight: f64) -> &'static str {
    if weight > 0.8 {
        "bg-green-500"
    } else if weight > 0.5 {
        "bg-blue-500"
    } else if weight > 0.2 {
        "bg-amber-500"
    } else {
        "bg-red-400"
    }
}

fn format_source_type(s: &str) -> &str {
    match s {
        "tavily_query" => "Search",
        "eventbrite_query" => "Eventbrite",
        "gofundme_query" => "GoFundMe",
        "volunteermatch_query" => "VolunteerMatch",
        "instagram" => "Instagram",
        "facebook" => "Facebook",
        "reddit" => "Reddit",
        "tiktok" => "TikTok",
        "twitter" => "Twitter",
        "bluesky" => "Bluesky",
        "web" => "Web",
        _ => s,
    }
}

#[allow(non_snake_case)]
#[component]
fn SourcesTab(
    city_slug: String,
    sources: Vec<SourceView>,
    schedule: SchedulePreview,
) -> Element {
    // Split sources into query sources and page sources
    let query_sources: Vec<&SourceView> = sources.iter().filter(|s| s.is_query).collect();
    let page_sources: Vec<&SourceView> = sources.iter().filter(|s| !s.is_query).collect();

    rsx! {
        // Next Run Preview
        div { class: "bg-white border border-gray-200 rounded-lg p-4 mb-6",
            h3 { class: "text-base font-semibold mb-3", "Next Scout Run Preview" }
            div { class: "flex gap-4 text-sm mb-3",
                span { class: "text-green-700 font-medium",
                    "{schedule.scheduled.len()} scheduled"
                }
                span { class: "text-amber-700 font-medium",
                    "{schedule.exploration.len()} exploration"
                }
                span { class: "text-gray-500",
                    "{schedule.skipped_count} skipped"
                }
                span { class: "text-gray-400",
                    "{schedule.total_sources} total"
                }
            }

            if !schedule.scheduled.is_empty() || !schedule.exploration.is_empty() {
                div { class: "overflow-x-auto",
                    table { class: "w-full text-sm",
                        thead {
                            tr { class: "text-left text-xs text-gray-500 border-b",
                                th { class: "pb-2 pr-3", "Source" }
                                th { class: "pb-2 pr-3", "Type" }
                                th { class: "pb-2 pr-3", "Reason" }
                                th { class: "pb-2 pr-3", "Weight" }
                                th { class: "pb-2 pr-3", "Cadence" }
                                th { class: "pb-2", "Last run" }
                            }
                        }
                        tbody {
                            for src in schedule.scheduled.iter().chain(schedule.exploration.iter()) {
                                tr { class: "border-b border-gray-100",
                                    td { class: "py-2 pr-3 font-mono text-xs max-w-[200px] truncate",
                                        title: "{src.canonical_value}",
                                        "{src.canonical_value}"
                                    }
                                    td { class: "py-2 pr-3",
                                        span { class: source_type_badge(&src.source_type, src.is_query),
                                            "{format_source_type(&src.source_type)}"
                                        }
                                    }
                                    td { class: "py-2 pr-3",
                                        span { class: reason_badge(&src.reason), "{src.reason}" }
                                    }
                                    td { class: "py-2 pr-3 text-xs",
                                        "{src.weight:.2}"
                                    }
                                    td { class: "py-2 pr-3 text-xs text-gray-500",
                                        "{src.cadence_hours}h"
                                    }
                                    td { class: "py-2 text-xs text-gray-500",
                                        if let Some(ref last) = src.last_scraped {
                                            "{last}"
                                        } else {
                                            "never"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                p { class: "text-gray-400 text-sm", "No sources are due for scraping." }
            }
        }

        // Query Sources
        div { class: "mb-6",
            h3 { class: "text-base font-semibold mb-3",
                "Query Sources "
                span { class: "text-gray-400 font-normal text-sm", "({query_sources.len()})" }
            }
            if query_sources.is_empty() {
                p { class: "text-gray-400 text-sm", "No query sources." }
            }
            for src in query_sources.iter() {
                { render_source_card(src, &city_slug) }
            }
        }

        // Page Sources
        div { class: "mb-6",
            h3 { class: "text-base font-semibold mb-3",
                "Page Sources "
                span { class: "text-gray-400 font-normal text-sm", "({page_sources.len()})" }
            }
            if page_sources.is_empty() {
                p { class: "text-gray-400 text-sm", "No page sources." }
            }
            for src in page_sources.iter() {
                { render_source_card(src, &city_slug) }
            }
        }
    }
}

fn render_source_card(src: &SourceView, _city_slug: &str) -> Element {
    let weight_pct = (src.effective_weight * 100.0).round() as u32;
    let bar_color = weight_bar_color(src.effective_weight);

    rsx! {
        div { class: "bg-white border border-gray-200 rounded-lg p-3 mb-2 hover:border-gray-400",
            div { class: "flex items-start justify-between",
                div { class: "flex-1 min-w-0",
                    div { class: "flex gap-2 items-center flex-wrap",
                        span { class: source_type_badge(&src.source_type, src.is_query),
                            "{format_source_type(&src.source_type)}"
                        }
                        span { class: discovery_badge(&src.discovery_method),
                            "{src.discovery_method}"
                        }
                        if src.consecutive_empty_runs > 3 {
                            span { class: "inline-block px-2 py-0.5 rounded-full text-xs bg-red-50 text-red-700",
                                "{src.consecutive_empty_runs} empty runs"
                            }
                        }
                    }
                    div { class: "mt-1 font-mono text-sm truncate",
                        if let Some(ref url) = src.url {
                            a { href: "{url}", target: "_blank", rel: "noopener",
                                class: "text-blue-600 hover:text-blue-800 no-underline",
                                "{src.canonical_value}"
                            }
                        } else {
                            span { class: "text-gray-900", "{src.canonical_value}" }
                        }
                    }
                    if let Some(ref ctx) = src.gap_context {
                        p { class: "text-xs text-gray-500 mt-1 italic", "{ctx}" }
                    }
                }
                // Weight bar
                div { class: "ml-4 text-right shrink-0",
                    div { class: "text-xs text-gray-500 mb-1", "weight {weight_pct}%" }
                    div { class: "w-20 h-2 bg-gray-200 rounded-full overflow-hidden",
                        div {
                            class: "h-full rounded-full {bar_color}",
                            style: "width: {weight_pct}%",
                        }
                    }
                    if src.quality_penalty < 1.0 {
                        div { class: "text-xs text-red-500 mt-0.5",
                            "penalty {src.quality_penalty:.1}"
                        }
                    }
                }
            }
            div { class: "flex gap-4 text-xs text-gray-400 mt-2",
                if src.is_query {
                    span { "{src.signals_produced} URLs produced" }
                    span { "cadence {src.cadence_hours}h" }
                    if let Some(ref last) = src.last_scraped {
                        span { "queried {last}" }
                    } else {
                        span { "never queried" }
                    }
                } else {
                    span { "{src.signals_produced} signals" }
                    if src.signals_corroborated > 0 {
                        span { "{src.signals_corroborated} corroborated" }
                    }
                    span { "cadence {src.cadence_hours}h" }
                    if let Some(ref last) = src.last_scraped {
                        span { "scraped {last}" }
                    } else {
                        span { "never scraped" }
                    }
                    if let Some(ref last) = src.last_produced_signal {
                        span { "last signal {last}" }
                    }
                }
            }
        }
    }
}

pub fn render_sources_tab(
    city_slug: String,
    sources: Vec<SourceView>,
    schedule: SchedulePreview,
) -> Element {
    rsx! {
        SourcesTab { city_slug, sources, schedule }
    }
}
