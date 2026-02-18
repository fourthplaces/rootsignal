use dioxus::prelude::*;

use super::layout::Layout;
use crate::templates::render_to_html;

#[allow(non_snake_case)]
fn MapPage() -> Element {
    let map_script = r#"
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
                m.bindPopup(`<strong>${p.title}</strong><br><span style="color:${color};font-weight:600;font-size:11px">${p.node_type}</span><br><span style="font-size:12px;color:#555">${(p.summary||'').substring(0, 120)}</span><br><a href="/admin/nodes/${p.id}" style="font-size:12px">View details</a>`);
                markers.addLayer(m);
            });
        });
}

map.on('moveend', loadMarkers);
loadMarkers();
"#;

    rsx! {
        Layout { title: "Map".to_string(), active_page: "map".to_string(),
            div {
                div { id: "map", class: "h-screen w-full" }
            }
            script { src: "https://unpkg.com/leaflet@1.9.4/dist/leaflet.js" }
            script { dangerous_inner_html: map_script }
        }
    }
}

pub fn render_map() -> String {
    let mut dom = VirtualDom::new(MapPage);
    dom.rebuild_in_place();
    render_to_html(&dom)
}
