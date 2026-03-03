import { useState, useMemo, useCallback, useEffect, useRef } from "react";
import { useSearchParams } from "react-router";
import { useQuery } from "@apollo/client";
import {
  ReactFlow,
  Background,
  Controls,
  type Node as RFNode,
  type Edge as RFEdge,
  type NodeChange,
  type NodeMouseHandler,
  ReactFlowProvider,
  MarkerType,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import {
  Panel,
  Group as PanelGroup,
  Separator as PanelResizeHandle,
} from "react-resizable-panels";

import { GRAPH_NEIGHBORHOOD } from "@/graphql/queries";
import { GraphNodeMemo, type GraphNodeData } from "@/components/graph/GraphNode";
import { FilterSidebar } from "@/components/graph/FilterSidebar";
import { InspectorPane } from "@/components/graph/InspectorPane";
import { GraphMap, type MapBounds } from "@/components/graph/GraphMap";

const nodeTypes = { graphNode: GraphNodeMemo };

const EDGE_STYLES: Record<string, { strokeDasharray?: string; stroke: string }> = {
  Contains: { stroke: "rgba(34,211,238,0.4)" },
  ActedIn: { stroke: "rgba(236,72,153,0.4)" },
  SourcedFrom: { stroke: "rgba(156,163,175,0.3)", strokeDasharray: "4 2" },
  RespondsTo: { stroke: "rgba(239,68,68,0.4)", strokeDasharray: "6 3" },
};

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

// Simple layout: arrange nodes in a grid, grouped by type
function layoutNodes(gqlNodes: GqlNode[], gqlEdges: GqlEdge[]): RFNode[] {
  const adjacency = new Map<string, Set<string>>();
  for (const n of gqlNodes) adjacency.set(n.id, new Set());
  for (const e of gqlEdges) {
    adjacency.get(e.sourceId)?.add(e.targetId);
    adjacency.get(e.targetId)?.add(e.sourceId);
  }

  const typeOrder: Record<string, number> = {
    Gathering: 0,
    Resource: 0,
    HelpRequest: 0,
    Announcement: 0,
    Concern: 0,
    Actor: 1,
    Citation: 2,
  };

  const sorted = [...gqlNodes].sort((a, b) => {
    const oa = typeOrder[a.nodeType] ?? 4;
    const ob = typeOrder[b.nodeType] ?? 4;
    if (oa !== ob) return oa - ob;
    const ca = adjacency.get(a.id)?.size ?? 0;
    const cb = adjacency.get(b.id)?.size ?? 0;
    return cb - ca;
  });

  const cols = Math.max(4, Math.ceil(Math.sqrt(sorted.length)));
  const xGap = 260;
  const yGap = 120;

  return sorted.map((n, i) => ({
    id: n.id,
    type: "graphNode",
    position: {
      x: (i % cols) * xGap,
      y: Math.floor(i / cols) * yGap,
    },
    data: {
      label: n.label,
      nodeType: n.nodeType,
      confidence: n.confidence ?? undefined,
    } satisfies GraphNodeData,
  }));
}

function layoutEdges(gqlEdges: GqlEdge[], nodeIdSet: Set<string>): RFEdge[] {
  return gqlEdges
    .filter((e) => nodeIdSet.has(e.sourceId) && nodeIdSet.has(e.targetId))
    .map((e, i) => {
      const style = EDGE_STYLES[e.edgeType] ?? { stroke: "rgba(156,163,175,0.3)" };
      return {
        id: `e-${i}`,
        source: e.sourceId,
        target: e.targetId,
        label: e.edgeType,
        type: "default",
        style: {
          stroke: style.stroke,
          strokeDasharray: style.strokeDasharray,
        },
        labelStyle: { fill: "rgba(156,163,175,0.6)", fontSize: 10 },
        markerEnd: { type: MarkerType.ArrowClosed, color: style.stroke },
      };
    });
}

function GraphExplorerInner() {
  const [searchParams, setSearchParams] = useSearchParams();

  // Filters with URL param persistence
  const [search, setSearch] = useState(searchParams.get("q") ?? "");
  const [maxNodes, setMaxNodes] = useState(
    Number(searchParams.get("limit")) || 100,
  );
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
        ],
      ),
  );
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(
    searchParams.get("node"),
  );
  const [inspectorCollapsed, setInspectorCollapsed] = useState(false);
  const [mapBounds, setMapBounds] = useState<MapBounds | null>(null);
  const [highlightedNodeId, setHighlightedNodeId] = useState<string | null>(null);

  // Sync URL params (guarded to avoid re-render loops from setSearchParams)
  const lastParamsRef = useRef("");
  useEffect(() => {
    const params: Record<string, string> = {};
    if (search) params.q = search;
    if (maxNodes !== 100) params.limit = String(maxNodes);
    if (timeFrom !== defaultTimeFrom()) params.from = timeFrom;
    if (timeTo !== defaultTimeTo()) params.to = timeTo;
    const typesStr = [...enabledTypes].sort().join(",");
    const defaultTypesStr = "Actor,Announcement,Concern,Gathering,HelpRequest,Resource";
    if (typesStr !== defaultTypesStr) params.types = typesStr;
    if (selectedNodeId) params.node = selectedNodeId;
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

  // Filter by search
  const filteredNodes = useMemo(() => {
    if (!search) return gqlNodes;
    const q = search.toLowerCase();
    return gqlNodes.filter(
      (n) =>
        n.label.toLowerCase().includes(q) ||
        n.nodeType.toLowerCase().includes(q) ||
        n.id.includes(q),
    );
  }, [gqlNodes, search]);

  // Count nodes by type (before search filter, for sidebar counts)
  const nodeCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const n of gqlNodes) {
      counts[n.nodeType] = (counts[n.nodeType] ?? 0) + 1;
    }
    return counts;
  }, [gqlNodes]);

  // Layout
  const rfNodes = useMemo(
    () => layoutNodes(filteredNodes, gqlEdges),
    [filteredNodes, gqlEdges],
  );
  const nodeIdSet = useMemo(
    () => new Set(filteredNodes.map((n) => n.id)),
    [filteredNodes],
  );
  const rfEdges = useMemo(
    () => layoutEdges(gqlEdges, nodeIdSet),
    [gqlEdges, nodeIdSet],
  );

  // Track user-initiated drag positions separately to avoid dual-state sync loops
  const [positionOverrides, setPositionOverrides] = useState<
    Map<string, { x: number; y: number }>
  >(new Map());

  // Reset overrides when the underlying data changes (new query results)
  useEffect(() => {
    setPositionOverrides(new Map());
  }, [rfNodes]);

  const nodes = useMemo(
    () =>
      rfNodes.map((n) => {
        const pos = positionOverrides.get(n.id);
        return pos ? { ...n, position: pos } : n;
      }),
    [rfNodes, positionOverrides],
  );

  const onNodesChange = useCallback((changes: NodeChange[]) => {
    // Only capture position changes from dragging; ignore selection/dimensions
    const posChanges = changes.filter(
      (c): c is NodeChange & { type: "position"; id: string; position?: { x: number; y: number } } =>
        c.type === "position" && "position" in c && c.position != null,
    );
    if (posChanges.length > 0) {
      setPositionOverrides((prev) => {
        const next = new Map(prev);
        for (const c of posChanges) {
          next.set(c.id, c.position!);
        }
        return next;
      });
    }
  }, []);

  const onNodeClick: NodeMouseHandler = useCallback((_event, node) => {
    setSelectedNodeId(node.id);
  }, []);

  const onPaneClick = useCallback(() => {
    setSelectedNodeId(null);
  }, []);

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

  // Build node map for inspector
  const nodeMap = useMemo(() => {
    const m = new Map<string, GqlNode>();
    for (const n of gqlNodes) m.set(n.id, n);
    return m;
  }, [gqlNodes]);

  const selectedNode = selectedNodeId ? (nodeMap.get(selectedNodeId) ?? null) : null;

  return (
    <PanelGroup orientation="vertical" className="h-[calc(100vh-4rem)]">
      {/* Top: map + graph + sidebar */}
      <Panel defaultSize={75} minSize={30}>
        <div className="flex h-full min-h-0">
          {/* Map + Graph split pane */}
          <PanelGroup orientation="horizontal" className="flex-1">
            {/* Map panel */}
            <Panel defaultSize={40} minSize={15}>
              <GraphMap
                nodes={filteredNodes}
                selectedNodeId={selectedNodeId}
                highlightedNodeId={highlightedNodeId}
                onBoundsChange={handleBoundsChange}
                onMarkerClick={handleMarkerClick}
                onMarkerHover={setHighlightedNodeId}
              />
            </Panel>

            {/* Draggable divider */}
            <PanelResizeHandle className="w-1.5 bg-border hover:bg-accent transition-colors cursor-col-resize" />

            {/* Graph canvas panel */}
            <Panel defaultSize={60} minSize={20}>
              <div className="relative w-full h-full">
                {loading && (
                  <div className="absolute top-3 left-3 z-10 px-3 py-1.5 rounded bg-card border border-border text-xs text-muted-foreground">
                    Loading graph...
                  </div>
                )}
                <ReactFlow
                  nodes={nodes}
                  edges={rfEdges}
                  onNodesChange={onNodesChange}
                  onNodeClick={onNodeClick}
                  onPaneClick={onPaneClick}
                  nodeTypes={nodeTypes}
                  fitView
                  minZoom={0.1}
                  maxZoom={2}
                  proOptions={{ hideAttribution: true }}
                  className="bg-background"
                >
                  <Background color="rgba(255,255,255,0.03)" gap={20} />
                  <Controls
                    showInteractive={false}
                    className="!bg-card !border-border !shadow-none [&>button]:!bg-card [&>button]:!border-border [&>button]:!fill-muted-foreground"
                  />
                </ReactFlow>
              </div>
            </Panel>
          </PanelGroup>

          {/* Filter sidebar */}
          <FilterSidebar
            nodeTypes={enabledTypes}
            onToggleNodeType={toggleNodeType}
            maxNodes={maxNodes}
            onMaxNodesChange={setMaxNodes}
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
          />
        </div>
      </Panel>

      {/* Horizontal divider for inspector */}
      <PanelResizeHandle className="h-1.5 bg-border hover:bg-accent transition-colors cursor-row-resize" />

      {/* Bottom: inspector pane */}
      <Panel defaultSize={25} minSize={5} collapsible>
        <InspectorPane
          selectedNode={selectedNode}
          edges={gqlEdges}
          nodeMap={nodeMap}
          collapsed={inspectorCollapsed}
          onToggleCollapse={() => setInspectorCollapsed((c) => !c)}
        />
      </Panel>
    </PanelGroup>
  );
}

export function GraphExplorerPage() {
  return (
    <ReactFlowProvider>
      <GraphExplorerInner />
    </ReactFlowProvider>
  );
}
