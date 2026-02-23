import { useEffect, useRef, useState, useMemo } from "react";
import mapboxgl from "mapbox-gl";
import "mapbox-gl/dist/mapbox-gl.css";
import type { FeatureCollection } from "geojson";

mapboxgl.accessToken = import.meta.env.VITE_MAPBOX_TOKEN ?? "";

const TYPE_COLORS: Record<string, string> = {
  Gathering: "#3b82f6",
  Aid: "#22c55e",
  Need: "#f97316",
  Notice: "#6b7280",
  Tension: "#ef4444",
};

export type MapSignal = {
  __typename: string;
  id: string;
  title: string;
  summary: string;
  confidence: number;
  causeHeat: number | null;
  location: { lat: number; lng: number } | null;
};

function signalsToGeoJson(signals: MapSignal[]): FeatureCollection {
  return {
    type: "FeatureCollection",
    features: signals
      .filter((s) => s.location != null)
      .map((s) => {
        const nodeType = s.__typename.replace("Gql", "").replace("Signal", "");
        return {
          type: "Feature" as const,
          geometry: {
            type: "Point" as const,
            coordinates: [s.location!.lng, s.location!.lat],
          },
          properties: {
            id: s.id,
            title: s.title,
            summary: s.summary,
            node_type: nodeType,
            confidence: s.confidence,
            cause_heat: s.causeHeat,
          },
        };
      }),
  };
}

interface RegionMapProps {
  region: { centerLat: number; centerLng: number; radiusKm: number };
  signals: MapSignal[];
}

export function RegionMap({ region, signals }: RegionMapProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const mapRef = useRef<mapboxgl.Map | null>(null);
  const popupRef = useRef<mapboxgl.Popup | null>(null);
  const [mapReady, setMapReady] = useState(false);

  const geojson = useMemo(() => signalsToGeoJson(signals), [signals]);

  // Initialize map
  useEffect(() => {
    if (!containerRef.current || mapRef.current) return;

    const map = new mapboxgl.Map({
      container: containerRef.current,
      style: "mapbox://styles/mapbox/dark-v11",
      center: [region.centerLng, region.centerLat],
      zoom: 12,
    });

    mapRef.current = map;

    map.on("load", () => {
      map.addSource("signals", {
        type: "geojson",
        data: { type: "FeatureCollection", features: [] },
        cluster: true,
        clusterMaxZoom: 14,
        clusterRadius: 50,
      });

      map.addLayer({
        id: "signal-clusters",
        type: "circle",
        source: "signals",
        filter: ["has", "point_count"],
        paint: {
          "circle-color": [
            "step",
            ["get", "point_count"],
            "#6366f1",
            10,
            "#8b5cf6",
            50,
            "#a78bfa",
          ],
          "circle-radius": [
            "step",
            ["get", "point_count"],
            16,
            10,
            22,
            50,
            30,
          ],
          "circle-stroke-width": 2,
          "circle-stroke-color": "#1e1b4b",
        },
      });

      map.addLayer({
        id: "signal-cluster-count",
        type: "symbol",
        source: "signals",
        filter: ["has", "point_count"],
        layout: {
          "text-field": ["get", "point_count_abbreviated"],
          "text-size": 12,
        },
        paint: { "text-color": "#ffffff" },
      });

      map.addLayer({
        id: "signal-points",
        type: "circle",
        source: "signals",
        filter: ["!", ["has", "point_count"]],
        paint: {
          "circle-color": [
            "match",
            ["get", "node_type"],
            "Gathering", TYPE_COLORS.Gathering!,
            "Aid", TYPE_COLORS.Aid!,
            "Need", TYPE_COLORS.Need!,
            "Notice", TYPE_COLORS.Notice!,
            "Tension", TYPE_COLORS.Tension!,
            "#6366f1",
          ],
          "circle-radius": 7,
          "circle-stroke-width": 2,
          "circle-stroke-color": "#18181b",
        },
      });

      map.on("click", "signal-clusters", (e) => {
        const feature = e.features?.[0];
        if (!feature || feature.geometry.type !== "Point") return;
        const clusterId = feature.properties?.cluster_id as number;
        const source = map.getSource("signals") as mapboxgl.GeoJSONSource;
        source.getClusterExpansionZoom(clusterId, (_err, zoom) => {
          if (zoom == null) return;
          map.easeTo({
            center: feature.geometry.type === "Point"
              ? (feature.geometry.coordinates as [number, number])
              : [0, 0],
            zoom,
          });
        });
      });

      map.on("click", "signal-points", (e) => {
        const feature = e.features?.[0];
        if (!feature || feature.geometry.type !== "Point") return;
        const coords = feature.geometry.coordinates.slice() as [number, number];
        const props = feature.properties ?? {};
        const title = props.title ?? "Untitled";
        const type = props.node_type ?? "";
        const summary = props.summary ?? "";
        const confidence = props.confidence != null ? `${Math.round(props.confidence * 100)}%` : "";
        const color = TYPE_COLORS[type] ?? "#6366f1";
        popupRef.current?.remove();
        popupRef.current = new mapboxgl.Popup({ closeButton: true, closeOnClick: true, maxWidth: "320px" })
          .setLngLat(coords)
          .setHTML(
            `<div style="font-family: system-ui, sans-serif; color: #e4e4e7; background: #18181b; padding: 8px 10px; border-radius: 6px; max-width: 300px;">
              <div style="display: flex; align-items: center; gap: 6px; margin-bottom: 4px;">
                <span style="display: inline-block; width: 8px; height: 8px; border-radius: 50%; background: ${color};"></span>
                <span style="font-size: 11px; color: #a1a1aa;">${type}${confidence ? ` · ${confidence}` : ""}</span>
              </div>
              <div style="font-weight: 600; font-size: 13px; line-height: 1.3; margin-bottom: 4px;">${title}</div>
              ${summary ? `<div style="font-size: 12px; color: #a1a1aa; line-height: 1.4;">${summary.length > 200 ? summary.slice(0, 200) + "…" : summary}</div>` : ""}
            </div>`,
          )
          .addTo(map);
      });

      for (const layer of ["signal-points", "signal-clusters"]) {
        map.on("mouseenter", layer, () => { map.getCanvas().style.cursor = "pointer"; });
        map.on("mouseleave", layer, () => { map.getCanvas().style.cursor = ""; });
      }

      setMapReady(true);
    });

    return () => {
      map.remove();
      mapRef.current = null;
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Update GeoJSON data
  useEffect(() => {
    if (!mapReady) return;
    const source = mapRef.current?.getSource("signals") as mapboxgl.GeoJSONSource | undefined;
    if (!source) return;
    source.setData(geojson);
  }, [geojson, mapReady]);

  return (
    <div
      ref={containerRef}
      className="rounded-lg border border-border overflow-hidden"
      style={{ height: "calc(100vh - 200px)" }}
    />
  );
}
