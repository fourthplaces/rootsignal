use dioxus::prelude::VirtualDom;

/// Render a VirtualDom into a complete HTML document string.
pub fn render_to_html(dom: &VirtualDom) -> String {
    format!(
        "<!DOCTYPE html><html lang=\"en\">{}</html>",
        dioxus::ssr::render(dom)
    )
}
