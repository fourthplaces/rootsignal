use dioxus::prelude::*;

use super::layout::Layout;
use crate::templates::render_to_html;

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
        Layout { title: "Quality Dashboard".to_string(), active_page: "quality".to_string(),
            div { class: "max-w-4xl mx-auto p-6",
                h2 { class: "text-xl font-semibold mb-4", "Quality Dashboard (Internal)" }
                div { class: "grid grid-cols-2 gap-4 mb-6",
                    div { class: "bg-white border border-gray-200 rounded-lg p-4 text-center",
                        div { class: "text-4xl font-bold text-blue-700", "{total_count}" }
                        div { class: "text-sm text-gray-400", "Total Signals" }
                    }
                    div { class: "bg-white border border-gray-200 rounded-lg p-4 text-center",
                        div { class: "text-4xl font-bold text-green-700", "{type_count}" }
                        div { class: "text-sm text-gray-400", "Signal Types" }
                    }
                }

                div { class: "bg-white border border-gray-200 rounded-lg p-4 mb-3",
                    h3 { class: "font-semibold mb-3", "Signal Type Breakdown" }
                    table { class: "w-full text-sm",
                        thead {
                            tr {
                                th { class: "text-left pb-2 text-gray-500", "Type" }
                                th { class: "text-right pb-2 text-gray-500", "Count" }
                            }
                        }
                        tbody {
                            for (label, count) in by_type.iter() {
                                tr {
                                    td { class: "py-1", "{label}" }
                                    td { class: "text-right py-1", "{count}" }
                                }
                            }
                        }
                    }
                }

                div { class: "bg-white border border-gray-200 rounded-lg p-4 mb-3",
                    h3 { class: "font-semibold mb-3", "Freshness Distribution" }
                    table { class: "w-full text-sm",
                        thead {
                            tr {
                                th { class: "text-left pb-2 text-gray-500", "Period" }
                                th { class: "text-right pb-2 text-gray-500", "Count" }
                            }
                        }
                        tbody {
                            for (label, count) in freshness.iter() {
                                tr {
                                    td { class: "py-1", "{label}" }
                                    td { class: "text-right py-1", "{count}" }
                                }
                            }
                        }
                    }
                }

                div { class: "bg-white border border-gray-200 rounded-lg p-4 mb-3",
                    h3 { class: "font-semibold mb-3", "Confidence Distribution" }
                    table { class: "w-full text-sm",
                        thead {
                            tr {
                                th { class: "text-left pb-2 text-gray-500", "Tier" }
                                th { class: "text-right pb-2 text-gray-500", "Count" }
                            }
                        }
                        tbody {
                            for (label, count) in confidence.iter() {
                                tr {
                                    td { class: "py-1", "{label}" }
                                    td { class: "text-right py-1", "{count}" }
                                }
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
    render_to_html(&dom)
}
