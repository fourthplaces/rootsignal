use dioxus::prelude::*;

use crate::templates::build_page;

#[allow(non_snake_case)]
#[component]
fn QualityDashboard(
    total_count: u64,
    type_count: usize,
    by_type: Vec<(String, u64)>,
    freshness: Vec<(String, u64)>,
    confidence: Vec<(String, u64)>,
) -> Element {
    rsx! {
        div { class: "container",
            h2 { style: "margin-bottom:16px;", "Quality Dashboard (Internal)" }
            div { style: "display:grid;grid-template-columns:repeat(2,1fr);gap:16px;margin-bottom:24px;",
                div { class: "node-card", style: "text-align:center;",
                    div { style: "font-size:36px;font-weight:700;color:#1565c0;", "{total_count}" }
                    div { style: "font-size:13px;color:#888;", "Total Signals" }
                }
                div { class: "node-card", style: "text-align:center;",
                    div { style: "font-size:36px;font-weight:700;color:#2e7d32;", "{type_count}" }
                    div { style: "font-size:13px;color:#888;", "Signal Types" }
                }
            }

            // Signal Type Breakdown
            div { class: "node-card",
                h3 { style: "margin-bottom:12px;", "Signal Type Breakdown" }
                table { style: "width:100%;font-size:14px;",
                    thead {
                        tr {
                            th { style: "text-align:left;", "Type" }
                            th { style: "text-align:right;", "Count" }
                        }
                    }
                    tbody {
                        for (label, count) in by_type.iter() {
                            tr {
                                td { "{label}" }
                                td { style: "text-align:right;", "{count}" }
                            }
                        }
                    }
                }
            }

            // Freshness Distribution
            div { class: "node-card",
                h3 { style: "margin-bottom:12px;", "Freshness Distribution" }
                table { style: "width:100%;font-size:14px;",
                    thead {
                        tr {
                            th { style: "text-align:left;", "Period" }
                            th { style: "text-align:right;", "Count" }
                        }
                    }
                    tbody {
                        for (label, count) in freshness.iter() {
                            tr {
                                td { "{label}" }
                                td { style: "text-align:right;", "{count}" }
                            }
                        }
                    }
                }
            }

            // Confidence Distribution
            div { class: "node-card",
                h3 { style: "margin-bottom:12px;", "Confidence Distribution" }
                table { style: "width:100%;font-size:14px;",
                    thead {
                        tr {
                            th { style: "text-align:left;", "Tier" }
                            th { style: "text-align:right;", "Count" }
                        }
                    }
                    tbody {
                        for (label, count) in confidence.iter() {
                            tr {
                                td { "{label}" }
                                td { style: "text-align:right;", "{count}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn render_quality(
    total_count: u64,
    type_count: usize,
    by_type: Vec<(String, u64)>,
    freshness: Vec<(String, u64)>,
    confidence: Vec<(String, u64)>,
) -> String {
    let mut dom = VirtualDom::new_with_props(
        QualityDashboard,
        QualityDashboardProps { total_count, type_count, by_type, freshness, confidence },
    );
    dom.rebuild_in_place();
    build_page("Quality Dashboard", &dioxus::ssr::render(&dom))
}
