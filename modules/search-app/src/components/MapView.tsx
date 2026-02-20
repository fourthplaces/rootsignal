import { useRef, useEffect } from "react";
import mapboxgl from "mapbox-gl";
import "mapbox-gl/dist/mapbox-gl.css";
import type { Bounds } from "@/hooks/useDebouncedBounds";

mapboxgl.accessToken = import.meta.env.VITE_MAPBOX_TOKEN ?? "";

const TYPE_COLORS: Record<string, string> = {
  Gathering: "#3b82f6",
  Aid: "#22c55e",
  Need: "#f97316",
  Notice: "#6b7280",
  Tension: "#ef4444",
};

interface Signal {
  id: string;
  title: string;
  location?: { lat: number; lng: number } | null;
  __typename?: string;
}

interface MapViewProps {
  signals: Signal[];
  onBoundsChange: (bounds: Bounds) => void;
  onSignalClick: (id: string, lng: number, lat: number) => void;
  flyToTarget: { lng: number; lat: number } | null;
  initialCenter?: [number, number];
  initialZoom?: number;
}

export function MapView({
  signals,
  onBoundsChange,
  onSignalClick,
  flyToTarget,
  initialCenter,
  initialZoom,
}: MapViewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const mapRef = useRef<mapboxgl.Map | null>(null);
  const onBoundsChangeRef = useRef(onBoundsChange);
  onBoundsChangeRef.current = onBoundsChange;
  const onSignalClickRef = useRef(onSignalClick);
  onSignalClickRef.current = onSignalClick;

  // Initialize map
  useEffect(() => {
    if (!containerRef.current || mapRef.current) return;

    const map = new mapboxgl.Map({
      container: containerRef.current,
      style: "mapbox://styles/mapbox/dark-v11",
      center: initialCenter ?? [-93.27, 44.97], // Twin Cities default
      zoom: initialZoom ?? 10,
    });

    mapRef.current = map;

    map.on("load", () => {
      // GeoJSON source with clustering
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

      // Click signal point → notify parent
      map.on("click", "signal-points", (e) => {
        const feature = e.features?.[0];
        if (!feature || feature.geometry.type !== "Point") return;
        const id = feature.properties?.id as string;
        const [lng, lat] = feature.geometry.coordinates;
        onSignalClickRef.current(id, lng!, lat!);
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

      // Fire initial bounds
      const ib = map.getBounds();
      if (ib) {
        onBoundsChangeRef.current({
          minLat: ib.getSouth(),
          maxLat: ib.getNorth(),
          minLng: ib.getWest(),
          maxLng: ib.getEast(),
        });
      }
    });

    // Viewport changes
    map.on("moveend", () => {
      const mb = map.getBounds();
      if (mb) {
        onBoundsChangeRef.current({
          minLat: mb.getSouth(),
          maxLat: mb.getNorth(),
          minLng: mb.getWest(),
          maxLng: mb.getEast(),
        });
      }
    });

    return () => {
      map.remove();
      mapRef.current = null;
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Update GeoJSON data when signals change
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !map.isStyleLoaded()) return;

    const source = map.getSource("signals") as mapboxgl.GeoJSONSource | undefined;
    if (!source) return;

    source.setData({
      type: "FeatureCollection",
      features: signals
        .filter((s) => s.location?.lat && s.location?.lng)
        .map((s) => ({
          type: "Feature" as const,
          geometry: {
            type: "Point" as const,
            coordinates: [s.location!.lng, s.location!.lat],
          },
          properties: {
            id: s.id,
            title: s.title,
            type: s.__typename?.replace("Gql", "").replace("Signal", "") ?? "Gathering",
          },
        })),
    });
  }, [signals]);

  // Fly to target
  useEffect(() => {
    if (!flyToTarget || !mapRef.current) return;
    mapRef.current.flyTo({
      center: [flyToTarget.lng, flyToTarget.lat],
      zoom: Math.max(mapRef.current.getZoom(), 14),
      essential: true,
    });
  }, [flyToTarget]);

  return <div ref={containerRef} className="h-full w-full" />;
}
