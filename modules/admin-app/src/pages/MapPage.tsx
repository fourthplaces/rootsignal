import { useState, useEffect, useRef } from "react";
import { useQuery } from "@apollo/client";
import { SIGNALS_NEAR_GEO_JSON, ADMIN_CITIES } from "@/graphql/queries";
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

export function MapPage() {
  const containerRef = useRef<HTMLDivElement>(null);
  const mapRef = useRef<mapboxgl.Map | null>(null);
  const popupRef = useRef<mapboxgl.Popup | null>(null);

  const { data: citiesData } = useQuery(ADMIN_CITIES);
  const cities = citiesData?.adminCities ?? [];

  const [selectedCity, setSelectedCity] = useState<{
    slug: string;
    centerLat: number;
    centerLng: number;
    radiusKm: number;
  } | null>(null);

  useEffect(() => {
    if (cities.length > 0 && !selectedCity) {
      setSelectedCity(cities[0]);
    }
  }, [cities, selectedCity]);

  const { data: geoData } = useQuery(SIGNALS_NEAR_GEO_JSON, {
    variables: selectedCity
      ? {
          lat: selectedCity.centerLat,
          lng: selectedCity.centerLng,
          radiusKm: selectedCity.radiusKm,
        }
      : undefined,
    skip: !selectedCity,
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
      center: [selectedCity?.centerLng ?? -93.27, selectedCity?.centerLat ?? 44.97],
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

      // Cluster circles
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

      // Cluster count labels
      map.addLayer({
        id: "signal-cluster-count",
        type: "symbol",
        source: "signals",
        filter: ["has", "point_count"],
        layout: {
          "text-field": ["get", "point_count_abbreviated"],
          "text-size": 12,
        },
        paint: {
          "text-color": "#ffffff",
        },
      });

      // Individual signal points
      map.addLayer({
        id: "signal-points",
        type: "circle",
        source: "signals",
        filter: ["!", ["has", "point_count"]],
        paint: {
          "circle-color": [
            "match",
            ["get", "type"],
            "Gathering",
            TYPE_COLORS.Gathering!,
            "Aid",
            TYPE_COLORS.Aid!,
            "Need",
            TYPE_COLORS.Need!,
            "Notice",
            TYPE_COLORS.Notice!,
            "Tension",
            TYPE_COLORS.Tension!,
            "#6366f1",
          ],
          "circle-radius": 7,
          "circle-stroke-width": 2,
          "circle-stroke-color": "#18181b",
        },
      });

      // Click clusters → zoom in
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

      // Click signal point → show popup
      map.on("click", "signal-points", (e) => {
        const feature = e.features?.[0];
        if (!feature || feature.geometry.type !== "Point") return;
        const coords = feature.geometry.coordinates.slice() as [number, number];
        const title = feature.properties?.title ?? "";
        const type = feature.properties?.type ?? "";

        popupRef.current?.remove();
        popupRef.current = new mapboxgl.Popup({ closeButton: true, closeOnClick: true })
          .setLngLat(coords)
          .setHTML(`<b>${title}</b><br/>${type}`)
          .addTo(map);
      });

      // Cursor on hover
      map.on("mouseenter", "signal-points", () => {
        map.getCanvas().style.cursor = "pointer";
      });
      map.on("mouseleave", "signal-points", () => {
        map.getCanvas().style.cursor = "";
      });
      map.on("mouseenter", "signal-clusters", () => {
        map.getCanvas().style.cursor = "pointer";
      });
      map.on("mouseleave", "signal-clusters", () => {
        map.getCanvas().style.cursor = "";
      });
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

  // Fly to selected city
  useEffect(() => {
    if (!selectedCity || !mapRef.current) return;
    mapRef.current.flyTo({
      center: [selectedCity.centerLng, selectedCity.centerLat],
      zoom: 12,
      essential: true,
    });
  }, [selectedCity]);

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Map</h1>
        <select
          value={selectedCity?.slug ?? ""}
          onChange={(e) => {
            const c = cities.find(
              (c: { slug: string }) => c.slug === e.target.value,
            );
            if (c) setSelectedCity(c);
          }}
          className="px-3 py-1.5 rounded-md border border-input bg-background text-sm"
        >
          {cities.map((c: { slug: string; name: string }) => (
            <option key={c.slug} value={c.slug}>
              {c.name}
            </option>
          ))}
        </select>
      </div>

      <div
        ref={containerRef}
        className="rounded-lg border border-border overflow-hidden"
        style={{ height: "calc(100vh - 160px)" }}
      />
    </div>
  );
}
