use crate::{EvidenceView, NodeView};

/// Render the map page.
pub fn render_map() -> String {
    let content = r#"
<div style="padding:0;max-width:none;">
    <div id="map" style="height:calc(100vh - 56px);border-radius:0;border:none;"></div>
</div>
<script src="https://unpkg.com/leaflet@1.9.4/dist/leaflet.js"></script>
<script>
const map = L.map('map').setView([44.9778, -93.2650], 12);
L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
    attribution: '&copy; OpenStreetMap contributors',
    maxZoom: 18,
}).addTo(map);

const colors = { Event: '#1565c0', Give: '#2e7d32', Ask: '#e65100', Notice: '#7b1fa2', Tension: '#c62828' };
let markers = L.layerGroup().addTo(map);

function loadMarkers() {
    const bounds = map.getBounds();
    const center = bounds.getCenter();
    const ne = bounds.getNorthEast();
    const radius = Math.min(center.distanceTo(ne) / 1000, 50);

    fetch(`/api/nodes/near?lat=${center.lat}&lng=${center.lng}&radius=${radius}`)
        .then(r => r.json())
        .then(data => {
            markers.clearLayers();
            (data.features || []).forEach(f => {
                const p = f.properties;
                const [lng, lat] = f.geometry.coordinates;
                const color = colors[p.node_type] || '#999';
                const m = L.circleMarker([lat, lng], {
                    radius: 7, fillColor: color, color: '#fff', weight: 2, fillOpacity: 0.85
                });
                m.bindPopup(`<strong>${p.title}</strong><br><span style="color:${color};font-weight:600;font-size:11px">${p.node_type}</span><br><span style="font-size:12px;color:#555">${(p.summary||'').substring(0, 120)}</span><br><a href="/nodes/${p.id}" style="font-size:12px">View details</a>`);
                markers.addLayer(m);
            });
        });
}

map.on('moveend', loadMarkers);
loadMarkers();
</script>
"#;

    build_page("Map", content)
}

/// Render the nodes list page.
pub fn render_nodes(nodes: &[NodeView]) -> String {
    let mut cards = String::new();

    if nodes.is_empty() {
        cards.push_str(r#"<p style="color:#888;text-align:center;padding:40px;">No signals found yet. Run the scout to populate the graph.</p>"#);
    }

    for node in nodes {
        let limited = if node.confidence < 0.6 {
            r#"<span style="font-size:11px;color:#795548;margin-left:8px;">Limited verification</span>"#
        } else {
            ""
        };

        let action = if !node.action_url.is_empty() {
            format!(
                r#"<a href="{}" class="action-btn" target="_blank" rel="noopener">Take Action</a>"#,
                html_escape(&node.action_url)
            )
        } else {
            String::new()
        };

        let corr = if node.corroboration_count > 0 {
            let s = if node.corroboration_count != 1 { "s" } else { "" };
            format!("<span>{} source{s}</span>", node.corroboration_count)
        } else {
            String::new()
        };

        cards.push_str(&format!(
            r#"<div class="node-card">
    <div><span class="badge badge-{tc}">{tl}</span>{limited}</div>
    <h3><a href="/nodes/{id}">{title}</a></h3>
    <p class="summary">{summary}</p>
    <div class="meta-row"><span>Verified {last}</span>{corr}{action}</div>
</div>"#,
            tc = node.type_class,
            tl = html_escape(&node.type_label),
            id = node.id,
            title = html_escape(&node.title),
            summary = html_escape(&node.summary),
            last = html_escape(&node.last_confirmed),
        ));
    }

    let content = format!(
        r#"<div class="container"><h2 style="margin-bottom:16px;">Recent Civic Signals</h2>{cards}</div>"#
    );

    build_page("Signals", &content)
}

/// Render a node detail page.
pub fn render_node_detail(node: &NodeView, evidence: &[EvidenceView]) -> String {
    let limited_banner = if node.confidence < 0.6 {
        r#"<div class="limited-banner">This signal has limited verification. It may be incomplete or not yet corroborated by multiple sources.</div>"#.to_string()
    } else {
        String::new()
    };

    let action_section = if !node.action_url.is_empty() {
        format!(
            r#"<a href="{}" class="action-btn" target="_blank" rel="noopener" style="margin:12px 0;display:inline-block;">Take Action</a>"#,
            html_escape(&node.action_url)
        )
    } else {
        r#"<p style="font-size:13px;color:#888;margin:12px 0;padding:8px;background:#f5f5f5;border-radius:4px;">This is context — here's what's happening in your community.</p>"#.to_string()
    };

    let s = if node.corroboration_count != 1 { "s" } else { "" };
    let roles_html = if !node.audience_roles.is_empty() {
        let tags: String = node
            .audience_roles
            .iter()
            .map(|r| format!(r#"<span class="role-tag">{}</span>"#, html_escape(r)))
            .collect::<Vec<_>>()
            .join("");
        format!(r#"<div class="roles">{tags}</div>"#)
    } else {
        String::new()
    };

    let evidence_html = if !evidence.is_empty() {
        let items: String = evidence
            .iter()
            .map(|ev| {
                let relevance_html = match &ev.relevance {
                    Some(r) => {
                        let color = match r.as_str() {
                            "contradicting" => "#c62828",
                            "direct" => "#2e7d32",
                            _ => "#795548",
                        };
                        format!(
                            r#"<span style="font-size:11px;font-weight:600;color:{color};margin-left:6px;">{}</span>"#,
                            html_escape(r)
                        )
                    }
                    None => String::new(),
                };
                let snippet_html = match &ev.snippet {
                    Some(s) => format!(
                        r#"<p style="font-size:12px;color:#666;margin:2px 0 8px 0;line-height:1.4;">{}</p>"#,
                        html_escape(s)
                    ),
                    None => String::new(),
                };
                format!(
                    r#"<a href="{url}" target="_blank" rel="noopener">{url}</a>{relevance_html}
                    {snippet_html}"#,
                    url = html_escape(&ev.source_url),
                )
            })
            .collect::<Vec<_>>()
            .join("");
        format!(r#"<div class="evidence-list"><h4>Sources</h4>{items}</div>"#)
    } else {
        String::new()
    };

    let content = format!(
        r#"<div class="container">
    <a href="/nodes" style="font-size:13px;color:#0066cc;text-decoration:none;">&larr; Back to signals</a>
    <div class="node-card" style="margin-top:12px;">
        <span class="badge badge-{tc}">{tl}</span>
        <h2 style="margin:8px 0;">{title}</h2>
        <p class="summary" style="font-size:15px;">{summary}</p>
        {limited_banner}
        {action_section}
        <dl class="detail-meta">
            <dt>Last verified</dt><dd>{last}</dd>
            <dt>Corroboration</dt><dd>Confirmed by {corr} source{s}</dd>
            <dt>Completeness</dt><dd>{comp}</dd>
        </dl>
        {roles_html}
        {evidence_html}
    </div>
</div>"#,
        tc = node.type_class,
        tl = html_escape(&node.type_label),
        title = html_escape(&node.title),
        summary = html_escape(&node.summary),
        last = html_escape(&node.last_confirmed),
        corr = node.corroboration_count,
        comp = html_escape(&node.completeness_label),
    );

    build_page(&node.title, &content)
}

/// Render the quality dashboard.
pub fn render_quality(
    total_count: u64,
    type_count: usize,
    role_count: usize,
    by_type: &[(String, u64)],
    freshness: &[(String, u64)],
    confidence: &[(String, u64)],
    roles: &[(String, u64)],
) -> String {
    let type_rows: String = by_type
        .iter()
        .map(|(t, c)| format!("<tr><td>{t}</td><td style=\"text-align:right;\">{c}</td></tr>"))
        .collect::<Vec<_>>()
        .join("");
    let freshness_rows: String = freshness
        .iter()
        .map(|(t, c)| format!("<tr><td>{t}</td><td style=\"text-align:right;\">{c}</td></tr>"))
        .collect::<Vec<_>>()
        .join("");
    let confidence_rows: String = confidence
        .iter()
        .map(|(t, c)| format!("<tr><td>{t}</td><td style=\"text-align:right;\">{c}</td></tr>"))
        .collect::<Vec<_>>()
        .join("");
    let role_rows: String = roles
        .iter()
        .map(|(t, c)| format!("<tr><td>{t}</td><td style=\"text-align:right;\">{c}</td></tr>"))
        .collect::<Vec<_>>()
        .join("");

    let content = format!(
        r#"<div class="container">
    <h2 style="margin-bottom:16px;">Quality Dashboard (Internal)</h2>
    <div style="display:grid;grid-template-columns:repeat(3,1fr);gap:16px;margin-bottom:24px;">
        <div class="node-card" style="text-align:center;"><div style="font-size:36px;font-weight:700;color:#1565c0;">{total_count}</div><div style="font-size:13px;color:#888;">Total Signals</div></div>
        <div class="node-card" style="text-align:center;"><div style="font-size:36px;font-weight:700;color:#2e7d32;">{type_count}</div><div style="font-size:13px;color:#888;">Signal Types</div></div>
        <div class="node-card" style="text-align:center;"><div style="font-size:36px;font-weight:700;color:#e65100;">{role_count}</div><div style="font-size:13px;color:#888;">Audience Roles</div></div>
    </div>
    <div class="node-card"><h3 style="margin-bottom:12px;">Signal Type Breakdown</h3><table style="width:100%;font-size:14px;"><thead><tr><th style="text-align:left;">Type</th><th style="text-align:right;">Count</th></tr></thead><tbody>{type_rows}</tbody></table></div>
    <div class="node-card"><h3 style="margin-bottom:12px;">Freshness Distribution</h3><table style="width:100%;font-size:14px;"><thead><tr><th style="text-align:left;">Period</th><th style="text-align:right;">Count</th></tr></thead><tbody>{freshness_rows}</tbody></table></div>
    <div class="node-card"><h3 style="margin-bottom:12px;">Confidence Distribution</h3><table style="width:100%;font-size:14px;"><thead><tr><th style="text-align:left;">Tier</th><th style="text-align:right;">Count</th></tr></thead><tbody>{confidence_rows}</tbody></table></div>
    <div class="node-card"><h3 style="margin-bottom:12px;">Audience Roles</h3><table style="width:100%;font-size:14px;"><thead><tr><th style="text-align:left;">Role</th><th style="text-align:right;">Count</th></tr></thead><tbody>{role_rows}</tbody></table></div>
</div>"#
    );

    build_page("Quality Dashboard", &content)
}

// --- Helpers ---

fn build_page(title: &str, content: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} — Root Signal</title>
<link rel="stylesheet" href="https://unpkg.com/leaflet@1.9.4/dist/leaflet.css" />
<style>
*{{margin:0;padding:0;box-sizing:border-box;}}
body{{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;color:#1a1a1a;background:#fafafa;}}
.header{{background:#1a1a1a;color:#fff;padding:12px 24px;display:flex;align-items:center;justify-content:space-between;}}
.header h1{{font-size:18px;font-weight:600;}}
.header nav a{{color:#ccc;text-decoration:none;margin-left:20px;font-size:14px;}}
.header nav a:hover{{color:#fff;}}
.container{{max-width:960px;margin:0 auto;padding:24px;}}
#map{{height:500px;border-radius:8px;margin-bottom:24px;border:1px solid #ddd;}}
.node-card{{background:#fff;border:1px solid #e0e0e0;border-radius:8px;padding:16px;margin-bottom:12px;}}
.node-card:hover{{border-color:#999;}}
.node-card h3{{font-size:16px;margin-bottom:4px;}}
.node-card h3 a{{color:#1a1a1a;text-decoration:none;}}
.node-card h3 a:hover{{color:#0066cc;}}
.node-card .summary{{color:#555;font-size:14px;margin-bottom:8px;}}
.badge{{display:inline-block;padding:2px 8px;border-radius:12px;font-size:11px;font-weight:600;text-transform:uppercase;}}
.badge-event{{background:#e3f2fd;color:#1565c0;}}
.badge-give{{background:#e8f5e9;color:#2e7d32;}}
.badge-ask{{background:#fff3e0;color:#e65100;}}
.badge-notice{{background:#f3e5f5;color:#7b1fa2;}}
.badge-tension{{background:#fce4ec;color:#c62828;}}
.meta-row{{display:flex;gap:12px;align-items:center;font-size:12px;color:#888;margin-top:8px;}}
.action-btn{{display:inline-block;padding:6px 16px;background:#0066cc;color:#fff;border-radius:4px;text-decoration:none;font-size:13px;font-weight:500;}}
.action-btn:hover{{background:#004499;}}
.limited-banner{{background:#fff8e1;border:1px solid #ffecb3;padding:8px 12px;border-radius:4px;font-size:13px;color:#795548;margin-bottom:12px;}}
.evidence-list{{margin-top:12px;padding-top:12px;border-top:1px solid #eee;}}
.evidence-list h4{{font-size:13px;color:#666;margin-bottom:6px;}}
.evidence-list a{{font-size:13px;color:#0066cc;display:block;margin-bottom:4px;}}
.detail-meta{{display:grid;grid-template-columns:1fr 1fr;gap:8px;margin:16px 0;font-size:13px;}}
.detail-meta dt{{color:#888;}}
.detail-meta dd{{color:#333;}}
.roles{{display:flex;gap:6px;flex-wrap:wrap;}}
.role-tag{{background:#f0f0f0;padding:2px 8px;border-radius:10px;font-size:11px;color:#555;}}
</style>
</head>
<body>
<div class="header">
    <h1>Root Signal</h1>
    <nav><a href="/">Map</a><a href="/nodes">Signals</a></nav>
</div>
{content}
</body>
</html>"#,
        title = html_escape(title),
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
