use dioxus::prelude::*;

use super::layout::Layout;
use crate::templates::render_to_html;

#[derive(Clone, PartialEq)]
pub struct ScoutStatusRow {
    pub city_name: String,
    pub city_slug: String,
    pub last_scouted: Option<String>,
    pub sources_due: u32,
    pub running: bool,
}

#[derive(Clone, PartialEq)]
pub struct DashboardData {
    // Stat cards
    pub total_signals: u64,
    pub total_stories: u64,
    pub total_actors: u64,
    pub active_sources: usize,
    pub total_sources: usize,
    pub unmet_tension_count: usize,
    // Chart data (pre-serialized JSON for Chart.js)
    pub signal_volume_json: String,
    pub signal_type_json: String,
    pub story_arc_json: String,
    pub story_category_json: String,
    pub freshness_json: String,
    pub confidence_json: String,
    pub source_weight_json: String,
    // Table data
    pub unmet_tensions: Vec<TensionRow>,
    pub top_sources: Vec<SourceRow>,
    pub bottom_sources: Vec<SourceRow>,
    pub extraction_yield: Vec<YieldRow>,
    pub gap_stats: Vec<GapRow>,
    pub scout_status: Vec<ScoutStatusRow>,
}

#[derive(Clone, PartialEq)]
pub struct TensionRow {
    pub title: String,
    pub severity: String,
    pub category: String,
    pub what_would_help: String,
}

#[derive(Clone, PartialEq)]
pub struct SourceRow {
    pub name: String,
    pub signals: u32,
    pub weight: f64,
    pub empty_runs: u32,
}

#[derive(Clone, PartialEq)]
pub struct YieldRow {
    pub source_type: String,
    pub extracted: u32,
    pub survived: u32,
    pub corroborated: u32,
    pub contradicted: u32,
}

#[derive(Clone, PartialEq)]
pub struct GapRow {
    pub gap_type: String,
    pub total: u32,
    pub successful: u32,
    pub avg_weight: f64,
}

fn stat_card(value: String, label: &str, color: &str) -> Element {
    let text_class = format!("text-3xl font-bold text-{color}-700");
    rsx! {
        div { class: "bg-white border border-gray-200 rounded-lg p-4 text-center",
            div { class: "{text_class}", "{value}" }
            div { class: "text-xs text-gray-400 mt-1", "{label}" }
        }
    }
}

#[allow(non_snake_case)]
#[component]
fn Dashboard(data: DashboardData) -> Element {
    let source_label = format!("{} / {}", data.active_sources, data.total_sources);

    rsx! {
        Layout { title: "Dashboard".to_string(), active_page: "dashboard".to_string(),
            div { class: "max-w-7xl mx-auto p-6",
                h2 { class: "text-xl font-semibold mb-4", "Dashboard" }

                // --- Stat cards ---
                div { class: "grid grid-cols-2 md:grid-cols-3 lg:grid-cols-6 gap-3 mb-6",
                    { stat_card(data.total_signals.to_string(), "Signals", "blue") }
                    { stat_card(data.total_stories.to_string(), "Stories", "green") }
                    { stat_card(data.total_actors.to_string(), "Actors", "purple") }
                    { stat_card(source_label, "Active / Total Sources", "indigo") }
                    { stat_card(data.unmet_tension_count.to_string(), "Unmet Tensions", "red") }
                }

                // --- Scout Status ---
                if !data.scout_status.is_empty() {
                    div { class: "bg-white border border-gray-200 rounded-lg p-4 mb-6",
                        h3 { class: "font-semibold mb-3 text-sm", "Scout Status" }
                        table { class: "w-full text-xs",
                            thead {
                                tr {
                                    th { class: "text-left pb-2 text-gray-500", "City" }
                                    th { class: "text-left pb-2 text-gray-500", "Status" }
                                    th { class: "text-left pb-2 text-gray-500", "Last Scouted" }
                                    th { class: "text-right pb-2 text-gray-500", "Sources Due" }
                                }
                            }
                            tbody {
                                for row in data.scout_status.iter() {
                                    tr {
                                        td { class: "py-1 pr-2",
                                            a { href: "/admin/cities/{row.city_slug}", class: "text-blue-600 hover:text-blue-800 no-underline",
                                                "{row.city_name}"
                                            }
                                        }
                                        td { class: "py-1 pr-2",
                                            if row.running {
                                                span { class: "inline-block px-2 py-0.5 rounded-full text-xs font-semibold bg-amber-50 text-amber-800",
                                                    "Running"
                                                }
                                            } else {
                                                span { class: "inline-block px-2 py-0.5 rounded-full text-xs font-semibold bg-green-50 text-green-800",
                                                    "Idle"
                                                }
                                            }
                                        }
                                        td { class: "py-1 pr-2 text-gray-500",
                                            if let Some(last) = &row.last_scouted {
                                                "{last}"
                                            } else {
                                                "Never"
                                            }
                                        }
                                        td { class: "text-right py-1",
                                            if row.sources_due > 0 {
                                                span { class: "text-amber-700 font-semibold", "{row.sources_due}" }
                                            } else {
                                                "0"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // --- Charts (2-column grid) ---
                div { class: "grid grid-cols-1 lg:grid-cols-2 gap-4 mb-6",
                    // Signal volume over time
                    div { class: "bg-white border border-gray-200 rounded-lg p-4",
                        h3 { class: "font-semibold mb-3 text-sm", "Signal Volume (30 days)" }
                        canvas { id: "chart-signal-volume", height: "200" }
                        script { dangerous_inner_html: "{data.signal_volume_json}" }
                    }

                    // Signal type distribution
                    div { class: "bg-white border border-gray-200 rounded-lg p-4",
                        h3 { class: "font-semibold mb-3 text-sm", "Signal Type Distribution" }
                        canvas { id: "chart-signal-type", height: "200" }
                        script { dangerous_inner_html: "{data.signal_type_json}" }
                    }

                    // Story arc distribution
                    div { class: "bg-white border border-gray-200 rounded-lg p-4",
                        h3 { class: "font-semibold mb-3 text-sm", "Story Arc Distribution" }
                        canvas { id: "chart-story-arc", height: "200" }
                        script { dangerous_inner_html: "{data.story_arc_json}" }
                    }

                    // Story category distribution
                    div { class: "bg-white border border-gray-200 rounded-lg p-4",
                        h3 { class: "font-semibold mb-3 text-sm", "Story Category Distribution" }
                        canvas { id: "chart-story-category", height: "200" }
                        script { dangerous_inner_html: "{data.story_category_json}" }
                    }

                    // Freshness distribution
                    div { class: "bg-white border border-gray-200 rounded-lg p-4",
                        h3 { class: "font-semibold mb-3 text-sm", "Signal Freshness" }
                        canvas { id: "chart-freshness", height: "200" }
                        script { dangerous_inner_html: "{data.freshness_json}" }
                    }

                    // Confidence distribution
                    div { class: "bg-white border border-gray-200 rounded-lg p-4",
                        h3 { class: "font-semibold mb-3 text-sm", "Signal Confidence" }
                        canvas { id: "chart-confidence", height: "200" }
                        script { dangerous_inner_html: "{data.confidence_json}" }
                    }

                    // Source weight distribution
                    div { class: "bg-white border border-gray-200 rounded-lg p-4",
                        h3 { class: "font-semibold mb-3 text-sm", "Source Weight Distribution" }
                        canvas { id: "chart-source-weight", height: "200" }
                        script { dangerous_inner_html: "{data.source_weight_json}" }
                    }
                }

                // --- Tables (2-column grid) ---
                div { class: "grid grid-cols-1 lg:grid-cols-2 gap-4 mb-6",
                    // Unmet tensions
                    div { class: "bg-white border border-gray-200 rounded-lg p-4",
                        h3 { class: "font-semibold mb-3 text-sm", "Unmet Tensions" }
                        if data.unmet_tensions.is_empty() {
                            p { class: "text-gray-400 text-sm", "No unmet tensions." }
                        } else {
                            table { class: "w-full text-xs",
                                thead {
                                    tr {
                                        th { class: "text-left pb-2 text-gray-500", "Title" }
                                        th { class: "text-left pb-2 text-gray-500", "Severity" }
                                        th { class: "text-left pb-2 text-gray-500", "Category" }
                                        th { class: "text-left pb-2 text-gray-500", "What Would Help" }
                                    }
                                }
                                tbody {
                                    for t in data.unmet_tensions.iter() {
                                        tr {
                                            td { class: "py-1 pr-2 max-w-48 truncate", "{t.title}" }
                                            td { class: "py-1 pr-2",
                                                span { class: severity_class(&t.severity), "{t.severity}" }
                                            }
                                            td { class: "py-1 pr-2 text-gray-500", "{t.category}" }
                                            td { class: "py-1 text-gray-500 max-w-48 truncate", "{t.what_would_help}" }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Top & Bottom sources
                    div { class: "bg-white border border-gray-200 rounded-lg p-4",
                        h3 { class: "font-semibold mb-3 text-sm", "Top 5 Sources" }
                        { render_source_table(&data.top_sources) }
                        h3 { class: "font-semibold mb-3 mt-4 text-sm", "Bottom 5 Sources" }
                        { render_source_table(&data.bottom_sources) }
                    }

                    // Extraction yield
                    div { class: "bg-white border border-gray-200 rounded-lg p-4",
                        h3 { class: "font-semibold mb-3 text-sm", "Extraction Yield by Source Type" }
                        if data.extraction_yield.is_empty() {
                            p { class: "text-gray-400 text-sm", "No data." }
                        } else {
                            table { class: "w-full text-xs",
                                thead {
                                    tr {
                                        th { class: "text-left pb-2 text-gray-500", "Type" }
                                        th { class: "text-right pb-2 text-gray-500", "Extracted" }
                                        th { class: "text-right pb-2 text-gray-500", "Survived" }
                                        th { class: "text-right pb-2 text-gray-500", "Corroborated" }
                                        th { class: "text-right pb-2 text-gray-500", "Contradicted" }
                                    }
                                }
                                tbody {
                                    for y in data.extraction_yield.iter() {
                                        tr {
                                            td { class: "py-1", "{y.source_type}" }
                                            td { class: "text-right py-1", "{y.extracted}" }
                                            td { class: "text-right py-1", "{y.survived}" }
                                            td { class: "text-right py-1", "{y.corroborated}" }
                                            td { class: "text-right py-1", "{y.contradicted}" }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Gap strategy performance
                    div { class: "bg-white border border-gray-200 rounded-lg p-4",
                        h3 { class: "font-semibold mb-3 text-sm", "Gap Strategy Performance" }
                        if data.gap_stats.is_empty() {
                            p { class: "text-gray-400 text-sm", "No gap analysis data." }
                        } else {
                            table { class: "w-full text-xs",
                                thead {
                                    tr {
                                        th { class: "text-left pb-2 text-gray-500", "Gap Type" }
                                        th { class: "text-right pb-2 text-gray-500", "Sources" }
                                        th { class: "text-right pb-2 text-gray-500", "Successful" }
                                        th { class: "text-right pb-2 text-gray-500", "Avg Weight" }
                                    }
                                }
                                tbody {
                                    for g in data.gap_stats.iter() {
                                        {
                                            let w = format!("{:.2}", g.avg_weight);
                                            rsx! {
                                                tr {
                                                    td { class: "py-1", "{g.gap_type}" }
                                                    td { class: "text-right py-1", "{g.total}" }
                                                    td { class: "text-right py-1", "{g.successful}" }
                                                    td { class: "text-right py-1", "{w}" }
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

fn severity_class(severity: &str) -> &'static str {
    match severity {
        "high" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold bg-red-50 text-red-800",
        "medium" => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold bg-amber-50 text-amber-800",
        _ => "inline-block px-2 py-0.5 rounded-full text-xs font-semibold bg-gray-100 text-gray-600",
    }
}

fn render_source_table(sources: &[SourceRow]) -> Element {
    if sources.is_empty() {
        return rsx! { p { class: "text-gray-400 text-sm", "No data." } };
    }
    rsx! {
        table { class: "w-full text-xs",
            thead {
                tr {
                    th { class: "text-left pb-2 text-gray-500", "Source" }
                    th { class: "text-right pb-2 text-gray-500", "Signals" }
                    th { class: "text-right pb-2 text-gray-500", "Weight" }
                    th { class: "text-right pb-2 text-gray-500", "Empty Runs" }
                }
            }
            tbody {
                for s in sources.iter() {
                    {
                        let w = format!("{:.2}", s.weight);
                        rsx! {
                            tr {
                                td { class: "py-1 pr-2 max-w-48 truncate", "{s.name}" }
                                td { class: "text-right py-1", "{s.signals}" }
                                td { class: "text-right py-1", "{w}" }
                                td { class: "text-right py-1", "{s.empty_runs}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

// --- Chart.js JSON builders ---

pub fn build_signal_volume_chart(data: &[(String, u64, u64, u64, u64, u64)]) -> String {
    let labels: Vec<&str> = data.iter().map(|(d, _, _, _, _, _)| d.as_str()).collect();
    let events: Vec<u64> = data.iter().map(|(_, e, _, _, _, _)| *e).collect();
    let gives: Vec<u64> = data.iter().map(|(_, _, g, _, _, _)| *g).collect();
    let asks: Vec<u64> = data.iter().map(|(_, _, _, a, _, _)| *a).collect();
    let notices: Vec<u64> = data.iter().map(|(_, _, _, _, n, _)| *n).collect();
    let tensions: Vec<u64> = data.iter().map(|(_, _, _, _, _, t)| *t).collect();

    format!(
        r#"new Chart(document.getElementById('chart-signal-volume'),{{type:'line',data:{{labels:{labels},datasets:[{{label:'Event',data:{events},borderColor:'#1565c0',backgroundColor:'rgba(21,101,192,0.1)',tension:0.3,fill:false}},{{label:'Give',data:{gives},borderColor:'#2e7d32',backgroundColor:'rgba(46,125,50,0.1)',tension:0.3,fill:false}},{{label:'Ask',data:{asks},borderColor:'#e65100',backgroundColor:'rgba(230,81,0,0.1)',tension:0.3,fill:false}},{{label:'Notice',data:{notices},borderColor:'#7b1fa2',backgroundColor:'rgba(123,31,162,0.1)',tension:0.3,fill:false}},{{label:'Tension',data:{tensions},borderColor:'#c62828',backgroundColor:'rgba(198,40,40,0.1)',tension:0.3,fill:false}}]}},options:{{responsive:true,plugins:{{legend:{{position:'bottom',labels:{{boxWidth:12,padding:8}}}}}},scales:{{y:{{beginAtZero:true,ticks:{{precision:0}}}}}}}}}});"#,
        labels = serde_json::to_string(&labels).unwrap_or_default(),
        events = serde_json::to_string(&events).unwrap_or_default(),
        gives = serde_json::to_string(&gives).unwrap_or_default(),
        asks = serde_json::to_string(&asks).unwrap_or_default(),
        notices = serde_json::to_string(&notices).unwrap_or_default(),
        tensions = serde_json::to_string(&tensions).unwrap_or_default(),
    )
}

pub fn build_signal_type_chart(by_type: &[(String, u64)]) -> String {
    let labels: Vec<&str> = by_type.iter().map(|(l, _)| l.as_str()).collect();
    let values: Vec<u64> = by_type.iter().map(|(_, c)| *c).collect();
    let colors = vec!["#1565c0", "#2e7d32", "#e65100", "#7b1fa2", "#c62828"];

    format!(
        r#"new Chart(document.getElementById('chart-signal-type'),{{type:'doughnut',data:{{labels:{labels},datasets:[{{data:{values},backgroundColor:{colors}}}]}},options:{{responsive:true,plugins:{{legend:{{position:'bottom',labels:{{boxWidth:12,padding:8}}}}}}}}}});"#,
        labels = serde_json::to_string(&labels).unwrap_or_default(),
        values = serde_json::to_string(&values).unwrap_or_default(),
        colors = serde_json::to_string(&colors).unwrap_or_default(),
    )
}

pub fn build_horizontal_bar_chart(id: &str, data: &[(String, u64)], color: &str) -> String {
    let labels: Vec<&str> = data.iter().map(|(l, _)| l.as_str()).collect();
    let values: Vec<u64> = data.iter().map(|(_, c)| *c).collect();

    format!(
        r#"new Chart(document.getElementById('{id}'),{{type:'bar',data:{{labels:{labels},datasets:[{{data:{values},backgroundColor:'{color}'}}]}},options:{{responsive:true,indexAxis:'y',plugins:{{legend:{{display:false}}}},scales:{{x:{{beginAtZero:true,ticks:{{precision:0}}}}}}}}}});"#,
        id = id,
        labels = serde_json::to_string(&labels).unwrap_or_default(),
        values = serde_json::to_string(&values).unwrap_or_default(),
        color = color,
    )
}

pub fn build_bar_chart(id: &str, data: &[(String, u64)], color: &str) -> String {
    let labels: Vec<&str> = data.iter().map(|(l, _)| l.as_str()).collect();
    let values: Vec<u64> = data.iter().map(|(_, c)| *c).collect();

    format!(
        r#"new Chart(document.getElementById('{id}'),{{type:'bar',data:{{labels:{labels},datasets:[{{data:{values},backgroundColor:'{color}'}}]}},options:{{responsive:true,plugins:{{legend:{{display:false}}}},scales:{{y:{{beginAtZero:true,ticks:{{precision:0}}}}}}}}}});"#,
        id = id,
        labels = serde_json::to_string(&labels).unwrap_or_default(),
        values = serde_json::to_string(&values).unwrap_or_default(),
        color = color,
    )
}

pub fn build_source_weight_chart(buckets: &[(String, u64)]) -> String {
    build_bar_chart("chart-source-weight", buckets, "#6366f1")
}

pub fn source_weight_buckets(weights: &[f64]) -> Vec<(String, u64)> {
    let mut buckets = vec![
        ("0-0.2".to_string(), 0u64),
        ("0.2-0.4".to_string(), 0),
        ("0.4-0.6".to_string(), 0),
        ("0.6-0.8".to_string(), 0),
        ("0.8-1.0".to_string(), 0),
    ];
    for &w in weights {
        let idx = if w < 0.2 { 0 }
            else if w < 0.4 { 1 }
            else if w < 0.6 { 2 }
            else if w < 0.8 { 3 }
            else { 4 };
        buckets[idx].1 += 1;
    }
    buckets
}

pub fn render_dashboard(data: DashboardData) -> String {
    let mut dom = VirtualDom::new_with_props(Dashboard, DashboardProps { data });
    dom.rebuild_in_place();
    render_to_html(&dom)
}
