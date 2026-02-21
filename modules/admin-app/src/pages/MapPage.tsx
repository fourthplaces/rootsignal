import { useEffect, useRef } from "react";
import { useQuery } from "@apollo/client";
import { SIGNALS_NEAR_GEO_JSON } from "@/graphql/queries";
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

interface RegionMapProps {
  region: { centerLat: number; centerLng: number; radiusKm: number };
}

export function RegionMap({ region }: RegionMapProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const mapRef = useRef<mapboxgl.Map | null>(null);
  const popupRef = useRef<mapboxgl.Popup | null>(null);

  const { data: geoData } = useQuery(SIGNALS_NEAR_GEO_JSON, {
    variables: { lat: region.centerLat, lng: region.centerLng, radiusKm: region.radiusKm },
  });

  const geojson: FeatureCollection | null = geoData?.signalsNearGeoJson
    ? JSON.parse(geoData.signalsNearGeoJson)
    : null;

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
        const title = feature.properties?.title ?? "";
        const type = feature.properties?.node_type ?? "";
        popupRef.current?.remove();
        popupRef.current = new mapboxgl.Popup({ closeButton: true, closeOnClick: true })
          .setLngLat(coords)
          .setHTML(`<b>${title}</b><br/>${type}`)
          .addTo(map);
      });

      for (const layer of ["signal-points", "signal-clusters"]) {
        map.on("mouseenter", layer, () => { map.getCanvas().style.cursor = "pointer"; });
        map.on("mouseleave", layer, () => { map.getCanvas().style.cursor = ""; });
      }
    });

    return () => {
      map.remove();
      mapRef.current = null;
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Update GeoJSON data
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !map.isStyleLoaded()) return;
    const source = map.getSource("signals") as mapboxgl.GeoJSONSource | undefined;
    if (!source) return;
    source.setData(geojson ?? { type: "FeatureCollection", features: [] });
  }, [geojson]);

  return (
    <div
      ref={containerRef}
      className="rounded-lg border border-border overflow-hidden"
      style={{ height: "calc(100vh - 200px)" }}
    />
  );
}
