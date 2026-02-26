import { useEffect, useRef, useState, useCallback, useMemo } from "react";
import mapboxgl from "mapbox-gl";
import "mapbox-gl/dist/mapbox-gl.css";
import type { FeatureCollection } from "geojson";

mapboxgl.accessToken = import.meta.env.VITE_MAPBOX_TOKEN ?? "";

const TYPE_COLORS: Record<string, string> = {
  Gathering: "#3b82f6",
  Aid: "#22c55e",
  Need: "#f59e0b",
  Notice: "#a855f7",
  Tension: "#ef4444",
  Actor: "#ec4899",
  Citation: "#6b7280",
};

type GraphMapNode = {
  id: string;
  nodeType: string;
  label: string;
  lat: number | null;
  lng: number | null;
};

export type MapBounds = {
  minLat: number;
  maxLat: number;
  minLng: number;
  maxLng: number;
};

export function GraphMap({
  nodes,
  selectedNodeId,
  highlightedNodeId,
  onBoundsChange,
  onMarkerClick,
  onMarkerHover,
}: {
  nodes: GraphMapNode[];
  selectedNodeId: string | null;
  highlightedNodeId: string | null;
  onBoundsChange: (bounds: MapBounds) => void;
  onMarkerClick: (nodeId: string) => void;
  onMarkerHover: (nodeId: string | null) => void;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const mapRef = useRef<mapboxgl.Map | null>(null);
  const [mapReady, setMapReady] = useState(false);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  const geojson = useMemo<FeatureCollection>(
    () => ({
      type: "FeatureCollection",
      features: nodes
        .filter((n) => n.lat != null && n.lng != null)
        .map((n) => ({
          type: "Feature" as const,
          geometry: {
            type: "Point" as const,
            coordinates: [n.lng!, n.lat!],
          },
          properties: {
            id: n.id,
            title: n.label,
            node_type: n.nodeType,
          },
        })),
    }),
    [nodes],
  );

  const emitBounds = useCallback(() => {
    const map = mapRef.current;
    if (!map) return;
    clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => {
      const b = map.getBounds();
      if (!b) return;
      onBoundsChange({
        minLat: b.getSouth(),
        maxLat: b.getNorth(),
        minLng: b.getWest(),
        maxLng: b.getEast(),
      });
    }, 300);
  }, [onBoundsChange]);

  // Initialize map
  useEffect(() => {
    if (!containerRef.current || mapRef.current) return;

    const map = new mapboxgl.Map({
      container: containerRef.current,
      style: "mapbox://styles/mapbox/dark-v11",
      center: [-97.7, 30.27], // default center (Austin)
      zoom: 10,
    });

    mapRef.current = map;

    map.on("load", () => {
      map.addSource("graph-nodes", {
        type: "geojson",
        data: { type: "FeatureCollection", features: [] },
        cluster: true,
        clusterMaxZoom: 14,
        clusterRadius: 50,
      });

      // Cluster circles
      map.addLayer({
        id: "graph-clusters",
        type: "circle",
        source: "graph-nodes",
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
          "circle-radius": ["step", ["get", "point_count"], 16, 10, 22, 50, 30],
          "circle-stroke-width": 2,
          "circle-stroke-color": "#18181b",
        },
      });

      map.addLayer({
        id: "graph-cluster-count",
        type: "symbol",
        source: "graph-nodes",
        filter: ["has", "point_count"],
        layout: {
          "text-field": ["get", "point_count_abbreviated"],
          "text-size": 12,
        },
        paint: { "text-color": "#ffffff" },
      });

      // Individual points colored by node type
      map.addLayer({
        id: "graph-points",
        type: "circle",
        source: "graph-nodes",
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
            "Actor", TYPE_COLORS.Actor!,
            "#6b7280",
          ],
          "circle-radius": 7,
          "circle-stroke-width": 2,
          "circle-stroke-color": "#18181b",
        },
      });

      // Highlight ring layer for selected/hovered nodes
      map.addLayer({
        id: "graph-point-highlight",
        type: "circle",
        source: "graph-nodes",
        filter: ["==", ["get", "id"], ""],
        paint: {
          "circle-radius": 12,
          "circle-color": "transparent",
          "circle-stroke-width": 3,
          "circle-stroke-color": "#fbbf24",
        },
      });

      // Click handlers
      map.on("click", "graph-clusters", (e) => {
        const feature = e.features?.[0];
        if (!feature || feature.geometry.type !== "Point") return;
        const clusterId = feature.properties?.cluster_id as number;
        const source = map.getSource("graph-nodes") as mapboxgl.GeoJSONSource;
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

      map.on("click", "graph-points", (e) => {
        const feature = e.features?.[0];
        if (!feature) return;
        const id = feature.properties?.id as string;
        if (id) onMarkerClick(id);
      });

      map.on("mouseenter", "graph-points", (e) => {
        map.getCanvas().style.cursor = "pointer";
        const id = e.features?.[0]?.properties?.id as string;
        if (id) onMarkerHover(id);
      });
      map.on("mouseleave", "graph-points", () => {
        map.getCanvas().style.cursor = "";
        onMarkerHover(null);
      });
      map.on("mouseenter", "graph-clusters", () => {
        map.getCanvas().style.cursor = "pointer";
      });
      map.on("mouseleave", "graph-clusters", () => {
        map.getCanvas().style.cursor = "";
      });

      // Emit bounds on move
      map.on("moveend", emitBounds);

      setMapReady(true);
      // Emit initial bounds
      emitBounds();
    });

    return () => {
      clearTimeout(debounceRef.current);
      map.remove();
      mapRef.current = null;
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Update GeoJSON data
  useEffect(() => {
    if (!mapReady) return;
    const source = mapRef.current?.getSource("graph-nodes") as mapboxgl.GeoJSONSource | undefined;
    if (!source) return;
    source.setData(geojson);
  }, [geojson, mapReady]);

  // Highlight selected/hovered node
  useEffect(() => {
    if (!mapReady || !mapRef.current) return;
    const highlightId = selectedNodeId ?? highlightedNodeId ?? "";
    mapRef.current.setFilter("graph-point-highlight", ["==", ["get", "id"], highlightId]);
  }, [selectedNodeId, highlightedNodeId, mapReady]);

  // Fly to selected node
  useEffect(() => {
    if (!mapReady || !mapRef.current || !selectedNodeId) return;
    const node = nodes.find((n) => n.id === selectedNodeId);
    if (node?.lat != null && node?.lng != null) {
      mapRef.current.flyTo({
        center: [node.lng, node.lat],
        zoom: Math.max(mapRef.current.getZoom(), 13),
        duration: 800,
      });
    }
  }, [selectedNodeId, mapReady]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div ref={containerRef} className="w-full h-full" />
  );
}
