"use client";

import { useRef, useEffect, useState, useCallback } from "react";
import { useSearchParams, useRouter } from "next/navigation";
import "mapbox-gl/dist/mapbox-gl.css";
import Sidebar from "./sidebar";

interface HeatMapPoint {
  id: string;
  latitude: number;
  longitude: number;
  weight: number;
  entityType: string;
  entityId: string;
}

interface ZipDensity {
  zipCode: string;
  city: string;
  latitude: number;
  longitude: number;
  listingCount: number;
  signalDomainCounts: Record<string, number>;
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
    listingType: string | null;
    urgency: string | null;
  };
  intent: "IN_SCOPE" | "OUT_OF_SCOPE" | "NEEDS_CLARIFICATION" | "KNOWLEDGE_QUESTION";
  reasoning: string;
}

type LayerMode = "density" | "gaps" | "entities";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type MapboxGL = any;
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type MapInstance = any;

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
      properties: { weight: p.weight, entityType: p.entityType, entityId: p.entityId },
    })),
  };
}

function zipToGeoJSON(zips: ZipDensity[]): GeoJSON.FeatureCollection {
  return {
    type: "FeatureCollection",
    features: zips.map((z) => ({
      type: "Feature",
      geometry: { type: "Point", coordinates: [z.longitude, z.latitude] },
      properties: { listingCount: z.listingCount, zipCode: z.zipCode, city: z.city },
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
        properties: { id: r.id, title: r.title, entityType: r.entityType || "listing" },
      })),
  };
}

function fitBounds(mapboxgl: MapboxGL, map: MapInstance, geojson: GeoJSON.FeatureCollection) {
  if (geojson.features.length === 0) return;
  const bounds = new mapboxgl.LngLatBounds();
  for (const f of geojson.features) {
    const coords = (f.geometry as GeoJSON.Point).coordinates;
    bounds.extend([coords[0], coords[1]]);
  }
  map.fitBounds(bounds, { padding: 60, maxZoom: 14 });
}

const ENTITY_COLORS: Record<string, string> = {
  nonprofit: "#3b82f6",
  government: "#22c55e",
  business: "#f97316",
  faith_organization: "#a855f7",
  listing: "#ef4444",
  entity: "#3b82f6",
};

export default function MapView() {
  const containerRef = useRef<HTMLDivElement>(null);
  const mapRef = useRef<MapInstance>(null);
  const mbRef = useRef<MapboxGL>(null);
  const searchParams = useSearchParams();
  const router = useRouter();
  const [layer, setLayer] = useState<LayerMode>(
    (searchParams.get("layer") as LayerMode) || "density",
  );
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [searchQuery, setSearchQuery] = useState(searchParams.get("q") || "");
  const [filters, setFilters] = useState({
    signalDomain: searchParams.get("domain") || "",
    category: searchParams.get("category") || "",
    entityType: searchParams.get("entityType") || "",
    zipCode: searchParams.get("zip") || "",
    radiusMiles: searchParams.get("radius") || "",
  });
  const [selectedPin, setSelectedPin] = useState<{
    entityType: string;
    entityId: string;
  } | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  // Sync state to URL
  const syncUrl = useCallback(
    (q: string, f: typeof filters, l: LayerMode) => {
      const params = new URLSearchParams();
      if (q) params.set("q", q);
      if (f.signalDomain) params.set("domain", f.signalDomain);
      if (f.category) params.set("category", f.category);
      if (f.entityType) params.set("entityType", f.entityType);
      if (f.zipCode) params.set("zip", f.zipCode);
      if (f.radiusMiles) params.set("radius", f.radiusMiles);
      if (l !== "density") params.set("layer", l);
      const qs = params.toString();
      router.replace(`/map${qs ? `?${qs}` : ""}`, { scroll: false });
    },
    [router],
  );

  const clearMapLayers = useCallback(() => {
    const map = mapRef.current;
    if (!map) return;
    const layerIds = [
      "heatmap-layer",
      "heatmap-points",
      "clusters",
      "cluster-count",
      "unclustered-point",
      "gaps-circles",
      "gaps-labels",
      "search-pins",
    ];
    for (const id of layerIds) {
      if (map.getLayer(id)) map.removeLayer(id);
    }
    for (const id of ["heat-source", "gaps-source", "search-source"]) {
      if (map.getSource(id)) map.removeSource(id);
    }
  }, []);

  const loadDensityLayer = useCallback(
    async (map: MapInstance) => {
      setLoading(true);
      setError("");
      try {
        const vars: Record<string, unknown> = {};
        if (filters.zipCode) vars.zipCode = filters.zipCode;
        if (filters.radiusMiles) vars.radiusMiles = parseFloat(filters.radiusMiles);
        if (filters.entityType) vars.entityType = filters.entityType;

        const data = await gqlFetch<{ heatMapPoints: HeatMapPoint[] }>(
          `query($zipCode: String, $radiusMiles: Float, $entityType: String) {
            heatMapPoints(zipCode: $zipCode, radiusMiles: $radiusMiles, entityType: $entityType) {
              id latitude longitude weight entityType entityId
            }
          }`,
          vars,
        );

        clearMapLayers();
        const geojson = toGeoJSON(data.heatMapPoints);

        map.addSource("heat-source", {
          type: "geojson",
          data: geojson,
          cluster: true,
          clusterMaxZoom: 14,
          clusterRadius: 50,
        });

        // Heatmap layer visible at low zoom
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

        // Cluster circles
        map.addLayer({
          id: "clusters",
          type: "circle",
          source: "heat-source",
          filter: ["has", "point_count"],
          paint: {
            "circle-color": [
              "step",
              ["get", "point_count"],
              "#51bbd6", 100,
              "#f1f075", 750,
              "#f28cb1",
            ],
            "circle-radius": ["step", ["get", "point_count"], 20, 100, 30, 750, 40],
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
        });

        // Individual points at high zoom
        map.addLayer({
          id: "unclustered-point",
          type: "circle",
          source: "heat-source",
          filter: ["!", ["has", "point_count"]],
          paint: {
            "circle-color": "#11b4da",
            "circle-radius": 6,
            "circle-stroke-width": 1,
            "circle-stroke-color": "#fff",
          },
        });

        fitBounds(mbRef.current, map, geojson);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Failed to load heat map data");
      } finally {
        setLoading(false);
      }
    },
    [filters, clearMapLayers],
  );

  const loadGapsLayer = useCallback(
    async (map: MapInstance) => {
      setLoading(true);
      setError("");
      try {
        const vars: Record<string, unknown> = { limit: 50 };
        if (filters.signalDomain) vars.signalDomain = filters.signalDomain;
        if (filters.category) vars.category = filters.category;

        const data = await gqlFetch<{ signalGaps: ZipDensity[] }>(
          `query($signalDomain: String, $category: String, $limit: Int) {
            signalGaps(signalDomain: $signalDomain, category: $category, limit: $limit) {
              zipCode city latitude longitude listingCount signalDomainCounts
            }
          }`,
          vars,
        );

        clearMapLayers();
        const geojson = zipToGeoJSON(data.signalGaps);

        map.addSource("gaps-source", { type: "geojson", data: geojson });

        // Bigger circle = bigger gap (fewer listings)
        const maxCount = Math.max(...data.signalGaps.map((z) => z.listingCount), 1);
        map.addLayer({
          id: "gaps-circles",
          type: "circle",
          source: "gaps-source",
          paint: {
            "circle-color": [
              "interpolate",
              ["linear"],
              ["get", "listingCount"],
              0, "#dc2626",
              maxCount, "#fca5a5",
            ],
            "circle-radius": [
              "interpolate",
              ["linear"],
              ["get", "listingCount"],
              0, 20,
              maxCount, 6,
            ],
            "circle-opacity": 0.7,
            "circle-stroke-width": 1,
            "circle-stroke-color": "#fff",
          },
        });

        map.addLayer({
          id: "gaps-labels",
          type: "symbol",
          source: "gaps-source",
          layout: {
            "text-field": ["concat", ["get", "city"], "\n", ["to-string", ["get", "listingCount"]]],
            "text-size": 10,
            "text-offset": [0, 2.5],
          },
          paint: { "text-color": "#6b7280" },
        });

        fitBounds(mbRef.current, map, geojson);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Failed to load gaps data");
      } finally {
        setLoading(false);
      }
    },
    [filters, clearMapLayers],
  );

  const loadEntitiesLayer = useCallback(
    async (map: MapInstance) => {
      setLoading(true);
      setError("");
      try {
        const vars: Record<string, unknown> = {};
        if (filters.zipCode) vars.zipCode = filters.zipCode;
        if (filters.radiusMiles) vars.radiusMiles = parseFloat(filters.radiusMiles);
        if (filters.entityType) vars.entityType = filters.entityType;

        const data = await gqlFetch<{ heatMapPoints: HeatMapPoint[] }>(
          `query($zipCode: String, $radiusMiles: Float, $entityType: String) {
            heatMapPoints(zipCode: $zipCode, radiusMiles: $radiusMiles, entityType: $entityType) {
              id latitude longitude weight entityType entityId
            }
          }`,
          vars,
        );

        clearMapLayers();
        const geojson = toGeoJSON(data.heatMapPoints);

        map.addSource("heat-source", {
          type: "geojson",
          data: geojson,
          cluster: true,
          clusterMaxZoom: 14,
          clusterRadius: 50,
        });

        map.addLayer({
          id: "clusters",
          type: "circle",
          source: "heat-source",
          filter: ["has", "point_count"],
          paint: {
            "circle-color": "#94a3b8",
            "circle-radius": ["step", ["get", "point_count"], 18, 50, 25, 200, 35],
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
          paint: { "text-color": "#fff" },
        });

        map.addLayer({
          id: "unclustered-point",
          type: "circle",
          source: "heat-source",
          filter: ["!", ["has", "point_count"]],
          paint: {
            "circle-color": [
              "match",
              ["get", "entityType"],
              "nonprofit", ENTITY_COLORS.nonprofit,
              "government", ENTITY_COLORS.government,
              "business", ENTITY_COLORS.business,
              "faith_organization", ENTITY_COLORS.faith_organization,
              "listing", ENTITY_COLORS.listing,
              "#6b7280",
            ],
            "circle-radius": 7,
            "circle-stroke-width": 2,
            "circle-stroke-color": "#fff",
          },
        });

        fitBounds(mbRef.current, map, geojson);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Failed to load entity data");
      } finally {
        setLoading(false);
      }
    },
    [filters, clearMapLayers],
  );

  const loadLayer = useCallback(
    (mode: LayerMode) => {
      const map = mapRef.current;
      if (!map) return;
      switch (mode) {
        case "density":
          loadDensityLayer(map);
          break;
        case "gaps":
          loadGapsLayer(map);
          break;
        case "entities":
          loadEntitiesLayer(map);
          break;
      }
    },
    [loadDensityLayer, loadGapsLayer, loadEntitiesLayer],
  );

  const handleSearch = useCallback(
    async (e: React.FormEvent) => {
      e.preventDefault();
      if (!searchQuery.trim()) return;
      const map = mapRef.current;
      const mapboxgl = mbRef.current;
      if (!map || !mapboxgl) return;

      if (abortRef.current) abortRef.current.abort();
      abortRef.current = new AbortController();

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
                filters { signalDomain category listingType urgency }
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
            setMessage("This query is outside our scope. Try searching for volunteer needs, events, or organizations.");
            break;
          case "NEEDS_CLARIFICATION":
            setMessage(parsed.reasoning);
            break;
          case "KNOWLEDGE_QUESTION":
            setMessage(parsed.reasoning);
            break;
          case "IN_SCOPE": {
            // Auto-populate filters from parsed query
            if (parsed.filters.signalDomain) {
              setFilters((f) => ({ ...f, signalDomain: parsed.filters.signalDomain! }));
            }
            if (parsed.filters.category) {
              setFilters((f) => ({ ...f, category: parsed.filters.category! }));
            }

            // Plot search results as pins overlay
            if (results?.results) {
              const geojson = searchToGeoJSON(results.results);

              // Remove previous search pins
              if (map.getLayer("search-pins")) map.removeLayer("search-pins");
              if (map.getSource("search-source")) map.removeSource("search-source");

              if (geojson.features.length > 0) {
                map.addSource("search-source", { type: "geojson", data: geojson });
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
                fitBounds(mapboxgl, map, geojson);
                setMessage(`${results.results.length} results found`);
              } else {
                setMessage("No results with location data found.");
              }
            }
            break;
          }
        }
        syncUrl(searchQuery, filters, layer);
      } catch (err) {
        if ((err as Error).name !== "AbortError") {
          setError(err instanceof Error ? err.message : "Search failed");
        }
      } finally {
        setLoading(false);
      }
    },
    [searchQuery, filters, layer, syncUrl],
  );

  // Initialize map (dynamic import of mapbox-gl)
  useEffect(() => {
    if (!containerRef.current || mapRef.current) return;

    let cancelled = false;

    (async () => {
      const mapboxgl = (await import("mapbox-gl")).default;

      if (cancelled || !containerRef.current) return;

      mapboxgl.accessToken = process.env.NEXT_PUBLIC_MAPBOX_TOKEN || "";
      mbRef.current = mapboxgl;

      const savedViewport = localStorage.getItem("map-viewport");
      const viewport = savedViewport ? JSON.parse(savedViewport) : null;

      const map = new mapboxgl.Map({
        container: containerRef.current,
        style: "mapbox://styles/mapbox/light-v11",
        center: viewport?.center || [-93.265, 44.978], // Minneapolis default
        zoom: viewport?.zoom || 6,
      });

      map.addControl(new mapboxgl.NavigationControl(), "top-right");

      map.on("load", () => {
        if (cancelled) {
          map.remove();
          return;
        }
        mapRef.current = map;
        loadLayer("density");
      });

      // Click cluster to zoom in
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      map.on("click", "clusters", (e: any) => {
        const features = map.queryRenderedFeatures(e.point, { layers: ["clusters"] });
        if (!features.length) return;
        const clusterId = features[0].properties?.cluster_id;
        const source = map.getSource("heat-source");
        if (!source) return;
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        (source as any).getClusterExpansionZoom(clusterId, (err: Error | null, zoom: number) => {
          if (err) return;
          map.easeTo({
            center: (features[0].geometry as GeoJSON.Point).coordinates as [number, number],
            zoom,
          });
        });
      });

      // Click individual point to show sidebar
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      map.on("click", "unclustered-point", (e: any) => {
        const props = e.features?.[0]?.properties;
        if (props?.entityType && props?.entityId) {
          setSelectedPin({ entityType: props.entityType, entityId: props.entityId });
        }
      });

      // Click search pin to show sidebar
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      map.on("click", "search-pins", (e: any) => {
        const props = e.features?.[0]?.properties;
        if (props?.id) {
          setSelectedPin({ entityType: props.entityType || "listing", entityId: props.id });
        }
      });

      // Cursor changes
      for (const layerId of ["clusters", "unclustered-point", "search-pins"]) {
        map.on("mouseenter", layerId, () => {
          map.getCanvas().style.cursor = "pointer";
        });
        map.on("mouseleave", layerId, () => {
          map.getCanvas().style.cursor = "";
        });
      }

      // Save viewport on move
      map.on("moveend", () => {
        localStorage.setItem(
          "map-viewport",
          JSON.stringify({ center: map.getCenter().toArray(), zoom: map.getZoom() }),
        );
      });
    })();

    return () => {
      cancelled = true;
      if (mapRef.current) {
        mapRef.current.remove();
        mapRef.current = null;
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Reload layer when layer mode or filters change (after initial load)
  useEffect(() => {
    if (!mapRef.current) return;
    loadLayer(layer);
  }, [layer, loadLayer]);

  const handleLayerChange = (mode: LayerMode) => {
    setLayer(mode);
    setMessage("");
    syncUrl(searchQuery, filters, mode);
  };

  const handleFilterChange = (key: string, value: string) => {
    const next = { ...filters, [key]: value };
    setFilters(next);
    syncUrl(searchQuery, next, layer);
  };

  const isFilterDisabled = (key: string) => {
    if (layer === "gaps") return ["entityType", "zipCode", "radiusMiles"].includes(key);
    if (layer === "density" || layer === "entities") return ["signalDomain", "category"].includes(key);
    return false;
  };

  return (
    <div className="relative flex h-full flex-col">
      {/* Controls bar */}
      <div className="flex flex-wrap items-center gap-2 border-b border-gray-200 bg-white px-4 py-2">
        {/* Search */}
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

        <div className="mx-1 h-6 w-px bg-gray-200" />

        {/* Filters */}
        <input
          type="text"
          value={filters.signalDomain}
          onChange={(e) => handleFilterChange("signalDomain", e.target.value)}
          placeholder="Domain"
          disabled={isFilterDisabled("signalDomain")}
          className="w-28 rounded border border-gray-300 px-2 py-1.5 text-sm disabled:bg-gray-100 disabled:text-gray-400"
        />
        <input
          type="text"
          value={filters.category}
          onChange={(e) => handleFilterChange("category", e.target.value)}
          placeholder="Category"
          disabled={isFilterDisabled("category")}
          className="w-28 rounded border border-gray-300 px-2 py-1.5 text-sm disabled:bg-gray-100 disabled:text-gray-400"
        />
        <input
          type="text"
          value={filters.entityType}
          onChange={(e) => handleFilterChange("entityType", e.target.value)}
          placeholder="Entity type"
          disabled={isFilterDisabled("entityType")}
          className="w-28 rounded border border-gray-300 px-2 py-1.5 text-sm disabled:bg-gray-100 disabled:text-gray-400"
        />
        <input
          type="text"
          value={filters.zipCode}
          onChange={(e) => handleFilterChange("zipCode", e.target.value)}
          placeholder="Zip code"
          disabled={isFilterDisabled("zipCode")}
          className="w-24 rounded border border-gray-300 px-2 py-1.5 text-sm disabled:bg-gray-100 disabled:text-gray-400"
        />
        <input
          type="text"
          value={filters.radiusMiles}
          onChange={(e) => handleFilterChange("radiusMiles", e.target.value)}
          placeholder="Radius (mi)"
          disabled={isFilterDisabled("radiusMiles")}
          className="w-24 rounded border border-gray-300 px-2 py-1.5 text-sm disabled:bg-gray-100 disabled:text-gray-400"
        />

        <div className="mx-1 h-6 w-px bg-gray-200" />

        {/* Layer toggle */}
        <div className="flex gap-1 rounded bg-gray-100 p-0.5">
          {(["density", "gaps", "entities"] as LayerMode[]).map((mode) => (
            <button
              key={mode}
              onClick={() => handleLayerChange(mode)}
              className={`rounded px-3 py-1 text-xs font-medium capitalize ${
                layer === mode
                  ? "bg-white text-green-800 shadow-sm"
                  : "text-gray-600 hover:text-gray-800"
              }`}
            >
              {mode}
            </button>
          ))}
        </div>

        {/* Entity type legend for entities layer */}
        {layer === "entities" && (
          <>
            <div className="mx-1 h-6 w-px bg-gray-200" />
            <div className="flex gap-2 text-xs text-gray-500">
              {Object.entries(ENTITY_COLORS)
                .filter(([k]) => !["listing", "entity"].includes(k))
                .map(([type, color]) => (
                  <span key={type} className="flex items-center gap-1">
                    <span
                      className="inline-block h-2.5 w-2.5 rounded-full"
                      style={{ backgroundColor: color }}
                    />
                    {type.replace("_", " ")}
                  </span>
                ))}
            </div>
          </>
        )}
      </div>

      {/* Messages */}
      {(error || message) && (
        <div className={`px-4 py-2 text-sm ${error ? "bg-red-50 text-red-700" : "bg-blue-50 text-blue-700"}`}>
          {error || message}
        </div>
      )}

      {/* Loading overlay */}
      {loading && (
        <div className="absolute top-12 left-1/2 z-10 -translate-x-1/2 rounded-full bg-white px-4 py-1.5 text-sm text-gray-500 shadow">
          Loading...
        </div>
      )}

      {/* Map container */}
      <div ref={containerRef} className="flex-1" />

      {/* Detail sidebar */}
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
