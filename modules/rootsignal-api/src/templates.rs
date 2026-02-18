/// Wrap page content in the admin HTML shell (DOCTYPE, head, styles, nav).
pub fn build_page(title: &str, content: &str) -> String {
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

</style>
</head>
<body>
<div class="header">
    <h1>Root Signal</h1>
    <nav><a href="/admin">Map</a><a href="/admin/nodes">Signals</a></nav>
</div>
{content}
</body>
</html>"#,
        title = html_escape(title),
    )
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
