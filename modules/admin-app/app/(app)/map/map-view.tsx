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
}

interface ZipDensity {
  zipCode: string;
  addressLocality: string;
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
      properties: { listingCount: z.listingCount, zipCode: z.zipCode, city: z.addressLocality },
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

const ENTITY_COLORS: Record<string, string> = {
  nonprofit: "#3b82f6",
  government: "#22c55e",
  business: "#f97316",
  faith_organization: "#a855f7",
  listing: "#ef4444",
  entity: "#3b82f6",
};

const EMPTY_GEOJSON: GeoJSON.FeatureCollection = { type: "FeatureCollection", features: [] };

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

function addHeatSources(map: mapboxgl.Map, data: GeoJSON.FeatureCollection, mode: LayerMode, maxGapCount: number) {
  // Remove old layers/sources
  for (const id of ["heatmap-layer", "clusters", "cluster-count", "unclustered-point", "gaps-circles", "gaps-labels", "search-pins"]) {
    if (map.getLayer(id)) map.removeLayer(id);
  }
  for (const id of ["heat-source", "gaps-source", "search-source"]) {
    if (map.getSource(id)) map.removeSource(id);
  }

  if (data.features.length === 0) return;

  if (mode === "density" || mode === "entities") {
    map.addSource("heat-source", {
      type: "geojson",
      data,
      cluster: true,
      clusterMaxZoom: 14,
      clusterRadius: 50,
    });

    if (mode === "density") {
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
    }

    map.addLayer({
      id: "clusters",
      type: "circle",
      source: "heat-source",
      filter: ["has", "point_count"],
      paint: {
        "circle-color":
          mode === "entities"
            ? "#94a3b8"
            : ["step", ["get", "point_count"], "#51bbd6", 100, "#f1f075", 750, "#f28cb1"],
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
      paint: { "text-color": mode === "entities" ? "#fff" : "#000" },
    });

    map.addLayer({
      id: "unclustered-point",
      type: "circle",
      source: "heat-source",
      filter: ["!", ["has", "point_count"]],
      paint: {
        "circle-color":
          mode === "entities"
            ? [
                "match",
                ["get", "entityType"],
                "nonprofit", ENTITY_COLORS.nonprofit,
                "government", ENTITY_COLORS.government,
                "business", ENTITY_COLORS.business,
                "faith_organization", ENTITY_COLORS.faith_organization,
                "listing", ENTITY_COLORS.listing,
                "#6b7280",
              ]
            : "#11b4da",
        "circle-radius": mode === "entities" ? 7 : 6,
        "circle-stroke-width": mode === "entities" ? 2 : 1,
        "circle-stroke-color": "#fff",
      },
    });
  } else if (mode === "gaps") {
    map.addSource("gaps-source", { type: "geojson", data });

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
          maxGapCount, "#fca5a5",
        ],
        "circle-radius": [
          "interpolate",
          ["linear"],
          ["get", "listingCount"],
          0, 20,
          maxGapCount, 6,
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
        "text-offset": [0, 2.5] as [number, number],
      },
      paint: { "text-color": "#6b7280" },
    });
  }
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

  // Refs for latest values (accessed inside map callbacks without stale closures)
  const layerRef = useRef(layer);
  const filtersRef = useRef(filters);
  layerRef.current = layer;
  filtersRef.current = filters;

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

  const loadDensity = useCallback(async (map: mapboxgl.Map, mode: LayerMode) => {
    setLoading(true);
    setError("");
    try {
      const f = filtersRef.current;
      const vars: Record<string, unknown> = {};
      if (f.zipCode) vars.zipCode = f.zipCode;
      if (f.radiusMiles) vars.radiusMiles = parseFloat(f.radiusMiles);
      if (f.entityType) vars.entityType = f.entityType;

      const data = await gqlFetch<{ heatMapPoints: HeatMapPoint[] }>(
        `query($zipCode: String, $radiusMiles: Float, $entityType: String) {
          heatMapPoints(zipCode: $zipCode, radiusMiles: $radiusMiles, entityType: $entityType) {
            id latitude longitude weight entityType entityId
          }
        }`,
        vars,
      );
      const geojson = toGeoJSON(data.heatMapPoints);
      addHeatSources(map, geojson, mode, 1);
      fitBoundsToData(map, geojson);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load heat map data");
    } finally {
      setLoading(false);
    }
  }, []);

  const loadGaps = useCallback(async (map: mapboxgl.Map) => {
    setLoading(true);
    setError("");
    try {
      const f = filtersRef.current;
      const vars: Record<string, unknown> = { limit: 50 };
      if (f.signalDomain) vars.signalDomain = f.signalDomain;
      if (f.category) vars.category = f.category;

      const data = await gqlFetch<{ signalGaps: ZipDensity[] }>(
        `query($signalDomain: String, $category: String, $limit: Int) {
          signalGaps(signalDomain: $signalDomain, category: $category, limit: $limit) {
            zipCode addressLocality latitude longitude listingCount signalDomainCounts
          }
        }`,
        vars,
      );
      const geojson = zipToGeoJSON(data.signalGaps);
      const maxCount = Math.max(...data.signalGaps.map((z) => z.listingCount), 1);
      addHeatSources(map, geojson, "gaps", maxCount);
      fitBoundsToData(map, geojson);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load gaps data");
    } finally {
      setLoading(false);
    }
  }, []);

  const loadLayer = useCallback(
    (map: mapboxgl.Map, mode: LayerMode) => {
      switch (mode) {
        case "density":
        case "entities":
          loadDensity(map, mode);
          break;
        case "gaps":
          loadGaps(map);
          break;
      }
    },
    [loadDensity, loadGaps],
  );

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
      loadLayer(map, layerRef.current);
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

      // Cluster click — zoom in
      if (f.properties?.cluster_id) {
        const source = map.getSource("heat-source") as mapboxgl.GeoJSONSource;
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
          entityType: f.properties.entityType || "listing",
          entityId: f.properties.entityId,
        });
      } else if (f.properties?.id) {
        setSelectedPin({
          entityType: f.properties.entityType || "listing",
          entityId: f.properties.id as string,
        });
      }
    });

    return () => {
      map.remove();
      mapRef.current = null;
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

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
            if (parsed.filters.signalDomain) {
              setFilters((f) => ({ ...f, signalDomain: parsed.filters.signalDomain! }));
            }
            if (parsed.filters.category) {
              setFilters((f) => ({ ...f, category: parsed.filters.category! }));
            }

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
        syncUrl(searchQuery, filters, layer);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Search failed");
      } finally {
        setLoading(false);
      }
    },
    [searchQuery, filters, layer, syncUrl],
  );

  const handleLayerChange = (mode: LayerMode) => {
    setLayer(mode);
    setMessage("");
    const map = mapRef.current;
    if (map) {
      // Clear search overlay
      if (map.getLayer("search-pins")) map.removeLayer("search-pins");
      if (map.getSource("search-source")) map.removeSource("search-source");
      loadLayer(map, mode);
    }
    syncUrl(searchQuery, filters, mode);
  };

  const handleFilterChange = (key: string, value: string) => {
    const next = { ...filters, [key]: value };
    setFilters(next);
    syncUrl(searchQuery, next, layer);
  };

  const filterTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const handleFilterBlur = () => {
    if (filterTimeoutRef.current) clearTimeout(filterTimeoutRef.current);
    filterTimeoutRef.current = setTimeout(() => {
      const map = mapRef.current;
      if (map) loadLayer(map, layer);
    }, 300);
  };

  const isFilterDisabled = (key: string) => {
    if (layer === "gaps") return ["entityType", "zipCode", "radiusMiles"].includes(key);
    if (layer === "density" || layer === "entities")
      return ["signalDomain", "category"].includes(key);
    return false;
  };

  return (
    <div className="relative flex h-full flex-col">
      {/* Controls bar */}
      <div className="flex flex-wrap items-center gap-2 border-b border-gray-200 bg-white px-4 py-2">
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

        <input
          type="text"
          value={filters.signalDomain}
          onChange={(e) => handleFilterChange("signalDomain", e.target.value)}
          onBlur={handleFilterBlur}
          placeholder="Domain"
          disabled={isFilterDisabled("signalDomain")}
          className="w-28 rounded border border-gray-300 px-2 py-1.5 text-sm disabled:bg-gray-100 disabled:text-gray-400"
        />
        <input
          type="text"
          value={filters.category}
          onChange={(e) => handleFilterChange("category", e.target.value)}
          onBlur={handleFilterBlur}
          placeholder="Category"
          disabled={isFilterDisabled("category")}
          className="w-28 rounded border border-gray-300 px-2 py-1.5 text-sm disabled:bg-gray-100 disabled:text-gray-400"
        />
        <input
          type="text"
          value={filters.entityType}
          onChange={(e) => handleFilterChange("entityType", e.target.value)}
          onBlur={handleFilterBlur}
          placeholder="Entity type"
          disabled={isFilterDisabled("entityType")}
          className="w-28 rounded border border-gray-300 px-2 py-1.5 text-sm disabled:bg-gray-100 disabled:text-gray-400"
        />
        <input
          type="text"
          value={filters.zipCode}
          onChange={(e) => handleFilterChange("zipCode", e.target.value)}
          onBlur={handleFilterBlur}
          placeholder="Zip code"
          disabled={isFilterDisabled("zipCode")}
          className="w-24 rounded border border-gray-300 px-2 py-1.5 text-sm disabled:bg-gray-100 disabled:text-gray-400"
        />
        <input
          type="text"
          value={filters.radiusMiles}
          onChange={(e) => handleFilterChange("radiusMiles", e.target.value)}
          onBlur={handleFilterBlur}
          placeholder="Radius (mi)"
          disabled={isFilterDisabled("radiusMiles")}
          className="w-24 rounded border border-gray-300 px-2 py-1.5 text-sm disabled:bg-gray-100 disabled:text-gray-400"
        />

        <div className="mx-1 h-6 w-px bg-gray-200" />

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
