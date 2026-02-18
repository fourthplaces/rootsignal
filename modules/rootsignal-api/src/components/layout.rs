use dioxus::prelude::*;

struct NavItem {
    key: &'static str,
    label: &'static str,
    href: &'static str,
}

const NAV_ITEMS: &[NavItem] = &[
    NavItem { key: "map", label: "Map", href: "/admin" },
    NavItem { key: "stories", label: "Stories", href: "/admin/stories" },
    NavItem { key: "cities", label: "Cities", href: "/admin/cities" },
    NavItem { key: "quality", label: "Quality", href: "/admin/quality" },
];

/// Admin layout with sidebar navigation.
#[allow(non_snake_case)]
#[component]
pub fn Layout(title: String, active_page: String, children: Element) -> Element {
    let full_title = format!("{title} — Root Signal");
    rsx! {
        head {
            meta { charset: "utf-8" }
            meta { name: "viewport", content: "width=device-width, initial-scale=1" }
            title { "{full_title}" }
            script { src: "https://cdn.tailwindcss.com" }
            link { rel: "stylesheet", href: "https://unpkg.com/leaflet@1.9.4/dist/leaflet.css" }
        }
        body { class: "flex min-h-screen bg-gray-50 font-sans text-gray-900",
            div { class: "w-56 bg-gray-900 text-white flex flex-col shrink-0 fixed inset-y-0 left-0 z-50",
                div { class: "px-5 py-4 text-lg font-semibold border-b border-gray-700",
                    "Root Signal"
                }
                nav { class: "flex flex-col py-3",
                    for item in NAV_ITEMS.iter() {
                        {
                            let class = if item.key == active_page {
                                "block px-5 py-2.5 text-sm text-white bg-blue-600"
                            } else {
                                "block px-5 py-2.5 text-sm text-gray-400 hover:text-white hover:bg-gray-700 transition-colors"
                            };
                            let href = item.href;
                            let label = item.label;
                            rsx! { a { href: href, class: class, "{label}" } }
                        }
                    }
                }
            }
            div { class: "ml-56 flex-1 min-w-0",
                {children}
            }
        }
    }
}
