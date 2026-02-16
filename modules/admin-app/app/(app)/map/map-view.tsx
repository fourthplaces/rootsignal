"use client";

import { useRef, useState, useCallback, useEffect } from "react";
import { useSearchParams, useRouter } from "next/navigation";
import mapboxgl from "mapbox-gl";
import "mapbox-gl/dist/mapbox-gl.css";
import Sidebar from "./sidebar";

interface HeatMapPoint {
  id: string;
  latitude: number;
  longitude: number;
  weight: number;
  entityType: string;
  entityId: string;
  signalType: string | null;
}

interface SearchResult {
  id: string;
  title: string;
  description: string | null;
  status: string;
  entityName: string | null;
  entityType: string | null;
  locationText: string | null;
  locations: { latitude: number | null; longitude: number | null }[];
}

interface ParsedQuery {
  searchText: string | null;
  filters: {
    signalDomain: string | null;
    category: string | null;
    urgency: string | null;
  };
  intent: "IN_SCOPE" | "OUT_OF_SCOPE" | "NEEDS_CLARIFICATION" | "KNOWLEDGE_QUESTION";
  reasoning: string;
}

async function gqlFetch<T>(query: string, variables?: Record<string, unknown>): Promise<T> {
  const res = await fetch("/api/graphql", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ query, variables }),
  });
  const data = await res.json();
  if (data.errors) throw new Error(data.errors[0].message);
  return data.data;
}

function toGeoJSON(points: HeatMapPoint[]): GeoJSON.FeatureCollection {
  return {
    type: "FeatureCollection",
    features: points.map((p) => ({
      type: "Feature",
      geometry: { type: "Point", coordinates: [p.longitude, p.latitude] },
      properties: { weight: p.weight, entityType: p.entityType, entityId: p.entityId, signalType: p.signalType },
    })),
  };
}

function searchToGeoJSON(results: SearchResult[]): GeoJSON.FeatureCollection {
  return {
    type: "FeatureCollection",
    features: results
      .filter((r) => r.locations[0]?.latitude != null && r.locations[0]?.longitude != null)
      .map((r) => ({
        type: "Feature",
        geometry: {
          type: "Point",
          coordinates: [r.locations[0].longitude!, r.locations[0].latitude!],
        },
        properties: { id: r.id, title: r.title, entityType: r.entityType || "signal" },
      })),
  };
}

function fitBoundsToData(map: mapboxgl.Map, geojson: GeoJSON.FeatureCollection) {
  if (geojson.features.length === 0) return;
  const coords = geojson.features.map(
    (f) => (f.geometry as GeoJSON.Point).coordinates,
  );
  if (coords.length === 1) {
    map.flyTo({ center: coords[0] as [number, number], zoom: 12 });
    return;
  }
  const lngs = coords.map((c) => c[0]);
  const lats = coords.map((c) => c[1]);
  map.fitBounds(
    [
      [Math.min(...lngs), Math.min(...lats)],
      [Math.max(...lngs), Math.max(...lats)],
    ],
    { padding: 60, maxZoom: 14 },
  );
}

function addHeatSources(map: mapboxgl.Map, data: GeoJSON.FeatureCollection) {
  for (const id of ["heatmap-layer", "clusters", "cluster-count", "unclustered-point", "search-pins"]) {
    if (map.getLayer(id)) map.removeLayer(id);
  }
  for (const id of ["heat-source", "search-source"]) {
    if (map.getSource(id)) map.removeSource(id);
  }

  if (data.features.length === 0) return;

  map.addSource("heat-source", {
    type: "geojson",
    data,
    cluster: true,
    clusterMaxZoom: 14,
    clusterRadius: 50,
  });

  map.addLayer({
    id: "heatmap-layer",
    type: "heatmap",
    source: "heat-source",
    maxzoom: 9,
    paint: {
      "heatmap-weight": ["interpolate", ["linear"], ["get", "weight"], 0, 0, 10, 1],
      "heatmap-intensity": ["interpolate", ["linear"], ["zoom"], 0, 1, 9, 3],
      "heatmap-color": [
        "interpolate",
        ["linear"],
        ["heatmap-density"],
        0, "rgba(33,102,172,0)",
        0.2, "rgb(103,169,207)",
        0.4, "rgb(209,229,240)",
        0.6, "rgb(253,219,199)",
        0.8, "rgb(239,138,98)",
        1, "rgb(178,24,43)",
      ],
      "heatmap-radius": ["interpolate", ["linear"], ["zoom"], 0, 2, 9, 20],
      "heatmap-opacity": ["interpolate", ["linear"], ["zoom"], 7, 1, 9, 0],
    },
  });

  map.addLayer({
    id: "clusters",
    type: "circle",
    source: "heat-source",
    filter: ["has", "point_count"],
    paint: {
      "circle-color": ["step", ["get", "point_count"], "#51bbd6", 100, "#f1f075", 750, "#f28cb1"],
      "circle-radius": ["step", ["get", "point_count"], 20, 100, 30, 750, 40],
      "circle-opacity": 0.8,
    },
  });

  map.addLayer({
    id: "cluster-count",
    type: "symbol",
    source: "heat-source",
    filter: ["has", "point_count"],
    layout: {
      "text-field": ["get", "point_count_abbreviated"],
      "text-size": 12,
    },
    paint: { "text-color": "#000" },
  });

  map.addLayer({
    id: "unclustered-point",
    type: "circle",
    source: "heat-source",
    filter: ["!", ["has", "point_count"]],
    paint: {
      "circle-color": [
        "match",
        ["get", "signalType"],
        "ask", "#f97316",
        "give", "#22c55e",
        "event", "#3b82f6",
        "informative", "#9ca3af",
        "#11b4da",
      ],
      "circle-radius": 6,
      "circle-stroke-width": 1,
      "circle-stroke-color": "#fff",
    },
  });
}

function addSearchLayer(map: mapboxgl.Map, data: GeoJSON.FeatureCollection) {
  if (map.getLayer("search-pins")) map.removeLayer("search-pins");
  if (map.getSource("search-source")) map.removeSource("search-source");
  if (data.features.length === 0) return;

  map.addSource("search-source", { type: "geojson", data });
  map.addLayer({
    id: "search-pins",
    type: "circle",
    source: "search-source",
    paint: {
      "circle-color": "#7c3aed",
      "circle-radius": 8,
      "circle-stroke-width": 2,
      "circle-stroke-color": "#fff",
    },
  });
}

export default function MapView() {
  const mapNode = useRef<HTMLDivElement>(null);
  const mapRef = useRef<mapboxgl.Map | null>(null);
  const searchParams = useSearchParams();
  const router = useRouter();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [searchQuery, setSearchQuery] = useState(searchParams.get("q") || "");
  const [signalType, setSignalType] = useState(searchParams.get("signalType") || "");
  const [regenerating, setRegenerating] = useState(false);
  const [selectedPin, setSelectedPin] = useState<{
    entityType: string;
    entityId: string;
  } | null>(null);

  const signalTypeRef = useRef(signalType);
  signalTypeRef.current = signalType;

  const syncUrl = useCallback(
    (q: string, st: string) => {
      const params = new URLSearchParams();
      if (q) params.set("q", q);
      if (st) params.set("signalType", st);
      const qs = params.toString();
      router.replace(`/map${qs ? `?${qs}` : ""}`, { scroll: false });
    },
    [router],
  );

  const loadDensity = useCallback(async (map: mapboxgl.Map) => {
    setLoading(true);
    setError("");
    try {
      const vars: Record<string, unknown> = {};
      if (signalTypeRef.current) vars.signalType = signalTypeRef.current;

      const data = await gqlFetch<{ heatMapPoints: HeatMapPoint[] }>(
        `query($signalType: String) {
          heatMapPoints(signalType: $signalType) {
            id latitude longitude weight entityType entityId signalType
          }
        }`,
        vars,
      );
      const geojson = toGeoJSON(data.heatMapPoints);
      addHeatSources(map, geojson);
      fitBoundsToData(map, geojson);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load heat map data");
    } finally {
      setLoading(false);
    }
  }, []);

  // Initialize map
  useEffect(() => {
    const node = mapNode.current;
    if (typeof window === "undefined" || !node) return;

    const savedViewport = JSON.parse(localStorage.getItem("map-viewport") || "null");

    const map = new mapboxgl.Map({
      container: node,
      accessToken: process.env.NEXT_PUBLIC_MAPBOX_TOKEN,
      style: "mapbox://styles/mapbox/light-v11",
      center: [savedViewport?.center?.[0] ?? -93.265, savedViewport?.center?.[1] ?? 44.978],
      zoom: savedViewport?.zoom ?? 6,
    });

    mapRef.current = map;

    map.on("load", () => {
      loadDensity(map);
    });

    map.on("moveend", () => {
      const center = map.getCenter();
      localStorage.setItem(
        "map-viewport",
        JSON.stringify({ center: [center.lng, center.lat], zoom: map.getZoom() }),
      );
    });

    map.on("click", ["clusters", "unclustered-point", "search-pins"], (e) => {
      const features = e.features;
      if (!features?.length) return;
      const f = features[0];

      // Mapbox visual cluster click — zoom in
      if (f.properties?.cluster_id) {
        const sourceId = f.source as string;
        const source = map.getSource(sourceId) as mapboxgl.GeoJSONSource;
        if (!source) return;
        source.getClusterExpansionZoom(f.properties.cluster_id, (err, zoom) => {
          if (err) return;
          map.flyTo({ center: (f.geometry as GeoJSON.Point).coordinates as [number, number], zoom: zoom! });
        });
        return;
      }

      // Individual point — open sidebar
      if (f.properties?.entityId) {
        setSelectedPin({
          entityType: f.properties.entityType || "signal",
          entityId: f.properties.entityId,
        });
      } else if (f.properties?.id) {
        setSelectedPin({
          entityType: f.properties.entityType || "signal",
          entityId: f.properties.id as string,
        });
      }
    });

    return () => {
      map.remove();
      mapRef.current = null;
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const handleRegenerate = useCallback(async () => {
    const map = mapRef.current;
    if (!map) return;
    setRegenerating(true);
    setError("");
    setMessage("");
    try {
      const data = await gqlFetch<{ recomputeHeatMap: number }>(
        `mutation { recomputeHeatMap }`,
      );
      setMessage(`Regenerated ${data.recomputeHeatMap} heat map points`);
      await loadDensity(map);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to regenerate heat map");
    } finally {
      setRegenerating(false);
    }
  }, [loadDensity]);

  const handleSearch = useCallback(
    async (e: React.FormEvent) => {
      e.preventDefault();
      if (!searchQuery.trim()) return;
      const map = mapRef.current;
      if (!map) return;

      setLoading(true);
      setError("");
      setMessage("");

      try {
        const data = await gqlFetch<{
          parseQuery: { parsed: ParsedQuery; results: { results: SearchResult[] } | null };
        }>(
          `query($q: String!) {
            parseQuery(q: $q, autoSearch: true) {
              parsed {
                searchText
                filters { signalDomain category urgency }
                intent
                reasoning
              }
              results {
                results {
                  id title description status entityName entityType locationText
                  locations { latitude longitude }
                }
              }
            }
          }`,
          { q: searchQuery },
        );

        const { parsed, results } = data.parseQuery;

        switch (parsed.intent) {
          case "OUT_OF_SCOPE":
            setMessage(
              "This query is outside our scope. Try searching for volunteer needs, events, or organizations.",
            );
            break;
          case "NEEDS_CLARIFICATION":
            setMessage(parsed.reasoning);
            break;
          case "KNOWLEDGE_QUESTION":
            setMessage(parsed.reasoning);
            break;
          case "IN_SCOPE": {
            if (results?.results) {
              const geojson = searchToGeoJSON(results.results);
              addSearchLayer(map, geojson);
              if (geojson.features.length > 0) {
                fitBoundsToData(map, geojson);
                setMessage(`${results.results.length} results found`);
              } else {
                setMessage("No results with location data found.");
              }
            }
            break;
          }
        }
        syncUrl(searchQuery, signalType);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Search failed");
      } finally {
        setLoading(false);
      }
    },
    [searchQuery, signalType, syncUrl],
  );

  return (
    <div className="relative flex h-full flex-col">
      {/* Controls bar */}
      <div className="flex items-center gap-2 border-b border-gray-200 bg-white px-4 py-2">
        <form onSubmit={handleSearch} className="flex gap-1">
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Ask a question... e.g. where is help needed?"
            className="w-72 rounded border border-gray-300 px-3 py-1.5 text-sm focus:border-green-500 focus:outline-none"
          />
          <button
            type="submit"
            disabled={loading}
            className="rounded bg-green-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-green-800 disabled:opacity-50"
          >
            Search
          </button>
        </form>
        <select
          value={signalType}
          onChange={(e) => {
            setSignalType(e.target.value);
            syncUrl(searchQuery, e.target.value);
            const map = mapRef.current;
            if (map) {
              signalTypeRef.current = e.target.value;
              loadDensity(map);
            }
          }}
          className="rounded border border-gray-300 px-2 py-1.5 text-sm"
        >
          <option value="">All signals</option>
          <option value="ask">Ask</option>
          <option value="give">Give</option>
          <option value="event">Event</option>
          <option value="informative">Informative</option>
        </select>
        <button
          type="button"
          onClick={handleRegenerate}
          disabled={regenerating || loading}
          className="rounded border border-gray-300 bg-white px-3 py-1.5 text-sm font-medium text-gray-700 hover:bg-gray-50 disabled:opacity-50"
        >
          {regenerating ? "Regenerating..." : "Regenerate Map"}
        </button>
      </div>

      {(error || message) && (
        <div
          className={`px-4 py-2 text-sm ${error ? "bg-red-50 text-red-700" : "bg-blue-50 text-blue-700"}`}
        >
          {error || message}
        </div>
      )}

      {loading && (
        <div className="absolute top-12 left-1/2 z-10 -translate-x-1/2 rounded-full bg-white px-4 py-1.5 text-sm text-gray-500 shadow">
          Loading...
        </div>
      )}

      {/* Map */}
      <div ref={mapNode} className="flex-1" style={{ minHeight: 0 }} />

      {selectedPin && (
        <Sidebar
          entityType={selectedPin.entityType}
          entityId={selectedPin.entityId}
          onClose={() => setSelectedPin(null)}
        />
      )}
    </div>
  );
}
