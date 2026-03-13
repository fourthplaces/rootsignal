import { useState, useMemo, useCallback, useEffect, useRef } from "react";
import { useSearchParams } from "react-router";
import { useQuery } from "@apollo/client";
import {
  Panel,
  Group as PanelGroup,
  Separator as PanelResizeHandle,
} from "react-resizable-panels";

import { GRAPH_NEIGHBORHOOD } from "@/graphql/queries";
import { FilterSidebar } from "@/components/graph/FilterSidebar";
import { GraphMap, type MapBounds } from "@/components/graph/GraphMap";
import { ForceGraph } from "@/components/graph/ForceGraph";
import { InvestigateDrawer, type InvestigateMode } from "@/components/InvestigateDrawer";

type GqlNode = {
  id: string;
  nodeType: string;
  label: string;
  lat: number | null;
  lng: number | null;
  confidence: number | null;
  metadata: string;
};

type GqlEdge = {
  sourceId: string;
  targetId: string;
  edgeType: string;
};

function defaultTimeFrom(): string {
  const d = new Date();
  d.setDate(d.getDate() - 30);
  return d.toISOString().slice(0, 10);
}

function defaultTimeTo(): string {
  return new Date().toISOString().slice(0, 10);
}

export function GraphExplorerPage() {
  const [searchParams, setSearchParams] = useSearchParams();

  const [search, setSearch] = useState(searchParams.get("q") ?? "");
  const [maxNodes] = useState(5000);
  const [timeFrom, setTimeFrom] = useState(
    searchParams.get("from") ?? defaultTimeFrom(),
  );
  const [timeTo, setTimeTo] = useState(
    searchParams.get("to") ?? defaultTimeTo(),
  );
  const [enabledTypes, setEnabledTypes] = useState<Set<string>>(
    () =>
      new Set(
        searchParams.get("types")?.split(",").filter(Boolean) ?? [
          "Gathering",
          "Resource",
          "HelpRequest",
          "Announcement",
          "Concern",
          "Actor",
          "Location",
        ],
      ),
  );
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(
    searchParams.get("node"),
  );
  const [mapBounds, setMapBounds] = useState<MapBounds | null>(null);
  const [highlightedNodeId, setHighlightedNodeId] = useState<string | null>(null);
  const [investigation, setInvestigation] = useState<InvestigateMode | null>(null);

  const initialCenter = useMemo<[number, number] | undefined>(() => {
    const lat = parseFloat(searchParams.get("lat") ?? "");
    const lng = parseFloat(searchParams.get("lng") ?? "");
    return !isNaN(lat) && !isNaN(lng) ? [lng, lat] : undefined;
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Sync URL params
  const lastParamsRef = useRef("");
  useEffect(() => {
    const params: Record<string, string> = {};
    if (search) params.q = search;
    if (timeFrom !== defaultTimeFrom()) params.from = timeFrom;
    if (timeTo !== defaultTimeTo()) params.to = timeTo;
    const typesStr = [...enabledTypes].sort().join(",");
    const defaultTypesStr = "Actor,Announcement,Concern,Gathering,HelpRequest,Location,Resource";
    if (typesStr !== defaultTypesStr) params.types = typesStr;
    if (selectedNodeId) params.node = selectedNodeId;
    if (initialCenter) {
      params.lat = String(initialCenter[1]);
      params.lng = String(initialCenter[0]);
    }
    const serialized = JSON.stringify(params);
    if (serialized !== lastParamsRef.current) {
      lastParamsRef.current = serialized;
      setSearchParams(params, { replace: true });
    }
  }, [search, maxNodes, timeFrom, timeTo, enabledTypes, selectedNodeId, setSearchParams]);

  const nodeTypesArray = useMemo(() => [...enabledTypes], [enabledTypes]);

  const variables = useMemo(
    () => ({
      from: new Date(timeFrom).toISOString(),
      to: new Date(timeTo + "T23:59:59").toISOString(),
      nodeTypes: nodeTypesArray,
      limit: maxNodes,
      ...(mapBounds && {
        minLat: mapBounds.minLat,
        maxLat: mapBounds.maxLat,
        minLng: mapBounds.minLng,
        maxLng: mapBounds.maxLng,
      }),
    }),
    [timeFrom, timeTo, nodeTypesArray, maxNodes, mapBounds],
  );

  const { data, loading } = useQuery(GRAPH_NEIGHBORHOOD, { variables });

  const EMPTY_NODES: GqlNode[] = useMemo(() => [], []);
  const EMPTY_EDGES: GqlEdge[] = useMemo(() => [], []);
  const gqlNodes: GqlNode[] = data?.graphNeighborhood?.nodes ?? EMPTY_NODES;
  const gqlEdges: GqlEdge[] = data?.graphNeighborhood?.edges ?? EMPTY_EDGES;
  const totalCount: number = data?.graphNeighborhood?.totalCount ?? 0;

  // Filter to nodes visible on the map
  const mapVisibleNodes = useMemo(() => {
    if (!mapBounds) return gqlNodes;
    return gqlNodes.filter((n) => {
      if (n.lat == null || n.lng == null) return false;
      return (
        n.lat >= mapBounds.minLat &&
        n.lat <= mapBounds.maxLat &&
        n.lng >= mapBounds.minLng &&
        n.lng <= mapBounds.maxLng
      );
    });
  }, [gqlNodes, mapBounds]);

  // Filter by search
  const filteredNodes = useMemo(() => {
    if (!search) return mapVisibleNodes;
    const q = search.toLowerCase();
    return mapVisibleNodes.filter(
      (n) =>
        n.label.toLowerCase().includes(q) ||
        n.nodeType.toLowerCase().includes(q) ||
        n.id.includes(q),
    );
  }, [mapVisibleNodes, search]);

  const nodeCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const n of gqlNodes) {
      counts[n.nodeType] = (counts[n.nodeType] ?? 0) + 1;
    }
    return counts;
  }, [gqlNodes]);

  const visibleEdges = useMemo(() => {
    const idSet = new Set(filteredNodes.map((n) => n.id));
    return gqlEdges.filter((e) => idSet.has(e.sourceId) && idSet.has(e.targetId));
  }, [filteredNodes, gqlEdges]);

  const toggleNodeType = useCallback((type: string) => {
    setEnabledTypes((prev) => {
      const next = new Set(prev);
      if (next.has(type)) next.delete(type);
      else next.add(type);
      return next;
    });
  }, []);

  const handleBoundsChange = useCallback((bounds: MapBounds) => {
    setMapBounds(bounds);
  }, []);

  const handleMarkerClick = useCallback((nodeId: string) => {
    setSelectedNodeId(nodeId);
  }, []);

  const handleGraphNodeClick = useCallback((nodeId: string | null) => {
    setSelectedNodeId(nodeId);
  }, []);

  const handleGraphNodeHover = useCallback((nodeId: string | null) => {
    setHighlightedNodeId(nodeId);
  }, []);

  const handleInvestigate = useCallback((node: { id: string; label: string; nodeType: string }) => {
    setInvestigation({ mode: "node", nodeId: node.id, nodeLabel: node.label, nodeType: node.nodeType });
  }, []);

  const nodeMap = useMemo(() => {
    const m = new Map<string, GqlNode>();
    for (const n of gqlNodes) m.set(n.id, n);
    return m;
  }, [gqlNodes]);

  const selectedNode = selectedNodeId ? (nodeMap.get(selectedNodeId) ?? null) : null;

  return (
    <PanelGroup orientation="horizontal" className="h-[calc(100vh-4rem)]">
      {/* Map panel */}
      <Panel defaultSize={30} minSize={15}>
        <GraphMap
          nodes={filteredNodes}
          selectedNodeId={selectedNodeId}
          highlightedNodeId={highlightedNodeId}
          initialCenter={initialCenter}
          onBoundsChange={handleBoundsChange}
          onMarkerClick={handleMarkerClick}
          onMarkerHover={setHighlightedNodeId}
        />
      </Panel>

      <PanelResizeHandle className="w-1.5 bg-border hover:bg-accent transition-colors cursor-col-resize" />

      {/* Force graph panel */}
      <Panel defaultSize={50} minSize={20}>
        <div className="relative w-full h-full">
          {loading && (
            <div className="absolute top-3 left-3 z-10 px-3 py-1.5 rounded bg-card border border-border text-xs text-muted-foreground">
              Loading graph...
            </div>
          )}
          <ForceGraph
            nodes={filteredNodes}
            edges={visibleEdges}
            selectedNodeId={selectedNodeId}
            onNodeClick={handleGraphNodeClick}
            onNodeHover={handleGraphNodeHover}
          />
        </div>
      </Panel>

      <PanelResizeHandle className="w-1.5 bg-border hover:bg-accent transition-colors cursor-col-resize" />

      {/* Right pane: search + properties */}
      <Panel defaultSize={20} minSize={10}>
        <FilterSidebar
          nodeTypes={enabledTypes}
          onToggleNodeType={toggleNodeType}
          timeFrom={timeFrom}
          timeTo={timeTo}
          onTimeFromChange={setTimeFrom}
          onTimeToChange={setTimeTo}
          search={search}
          onSearchChange={setSearch}
          totalCount={totalCount}
          visibleCount={filteredNodes.length}
          nodeCounts={nodeCounts}
          allNodes={gqlNodes}
          selectedNode={selectedNode}
          edges={gqlEdges}
          nodeMap={nodeMap}
          onInvestigate={handleInvestigate}
        />
      </Panel>

      {/* AI investigation drawer — slides over the right panel */}
      {investigation && (
        <div className="fixed inset-y-16 right-0 w-[420px] z-50 border-l border-border bg-card shadow-2xl">
          <InvestigateDrawer
            investigation={investigation}
            onClose={() => setInvestigation(null)}
          />
        </div>
      )}
    </PanelGroup>
  );
}
