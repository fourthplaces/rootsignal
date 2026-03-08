import { useMemo, useCallback, useEffect, memo } from "react";
import { ReactFlow, Background, Controls, useReactFlow, Handle, MarkerType, type Node, type Edge, type NodeChange, type NodeProps, Position } from "@xyflow/react";
import { useQuery } from "@apollo/client";
import dagre from "@dagrejs/dagre";
import "@xyflow/react/dist/style.css";
import { useEventsPaneContext, type AdminEvent } from "../EventsPaneContext";
import { eventBg, eventBorder } from "../eventColor";
import { ADMIN_HANDLER_DESCRIPTIONS, ADMIN_HANDLER_OUTCOMES, ADMIN_SCOUT_RUN } from "../../../graphql/queries";

// ---------------------------------------------------------------------------
// Block DSL types (mirrors rootsignal-common::describe)
// ---------------------------------------------------------------------------

type Block =
  | { type: "label"; text: string }
  | { type: "counter"; label: string; value: number; total: number }
  | { type: "progress"; label: string; fraction: number }
  | { type: "checklist"; label: string; items: { text: string; done: boolean }[] }
  | { type: "key_value"; key: string; value: string }
  | { type: "status"; label: string; state: "waiting" | "running" | "done" | "error" };

// ---------------------------------------------------------------------------
// Handler outcome (from seesaw_effect_executions aggregation)
// ---------------------------------------------------------------------------

type HandlerOutcome = {
  handlerId: string;
  status: string;
  error: string | null;
  attempts: number;
  startedAt: string | null;
  completedAt: string | null;
};

// ---------------------------------------------------------------------------
// Flow node data (discriminated union for structured identity)
// ---------------------------------------------------------------------------

type FlowNodeData =
  | { nodeKind: "event-type"; handlerId: string | null; eventName: string; label: string }
  | { nodeKind: "handler"; handlerId: string; label: string; blocks?: Block[]; outcome?: HandlerOutcome };

const NODE_WIDTH = 200;
const NODE_HEIGHT = 50;
const HANDLER_WIDTH = 180;
const HANDLER_HEIGHT = 36;

// ---------------------------------------------------------------------------
// Block renderers
// ---------------------------------------------------------------------------

function BlockRenderer({ block }: { block: Block }) {
  switch (block.type) {
    case "checklist":
      return (
        <div style={{ marginTop: 4 }}>
          <div style={{ fontSize: 9, color: "#71717a", marginBottom: 2 }}>{block.label}</div>
          {block.items.map((item, i) => (
            <div key={i} style={{ fontSize: 9, color: item.done ? "#22c55e" : "#52525b", display: "flex", gap: 3, alignItems: "center" }}>
              <span>{item.done ? "✓" : "○"}</span>
              <span>{item.text}</span>
            </div>
          ))}
        </div>
      );
    case "counter":
      return (
        <div style={{ fontSize: 9, color: "#a1a1aa", marginTop: 2 }}>
          {block.label}: {block.value}/{block.total}
        </div>
      );
    case "progress": {
      const pct = Math.round(block.fraction * 100);
      return (
        <div style={{ marginTop: 2 }}>
          <div style={{ fontSize: 9, color: "#a1a1aa" }}>{block.label}: {pct}%</div>
          <div style={{ height: 3, background: "#3f3f46", borderRadius: 2, marginTop: 1 }}>
            <div style={{ height: "100%", width: `${pct}%`, background: "#22c55e", borderRadius: 2 }} />
          </div>
        </div>
      );
    }
    case "label":
      return <div style={{ fontSize: 9, color: "#a1a1aa", marginTop: 2 }}>{block.text}</div>;
    case "key_value":
      return (
        <div style={{ fontSize: 9, color: "#a1a1aa", marginTop: 2 }}>
          <span style={{ color: "#71717a" }}>{block.key}:</span> {block.value}
        </div>
      );
    case "status": {
      const colors: Record<string, string> = { waiting: "#71717a", running: "#eab308", done: "#22c55e", error: "#ef4444" };
      return (
        <div style={{ fontSize: 9, color: colors[block.state] ?? "#a1a1aa", marginTop: 2 }}>
          {block.label}: {block.state}
        </div>
      );
    }
    default:
      return null;
  }
}

// ---------------------------------------------------------------------------
// Custom handler node with optional block rendering
// ---------------------------------------------------------------------------

function formatDuration(startedAt: string, completedAt: string): string {
  const ms = new Date(completedAt).getTime() - new Date(startedAt).getTime();
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

const STATUS_BORDER: Record<string, string> = {
  pending: "#52525b",
  running: "#eab308",
  completed: "#22c55e",
  error: "#ef4444",
};

const HandlerNode = memo(({ data }: NodeProps) => {
  const d = data as FlowNodeData & { nodeKind: "handler" };
  const blocks = d.blocks;
  const outcome = d.outcome;
  const hasBlocks = blocks && blocks.length > 0;
  const borderColor = STATUS_BORDER[outcome?.status ?? "pending"] ?? "#52525b";
  const isRunning = outcome?.status === "running";
  const duration = outcome?.status === "completed" && outcome.startedAt && outcome.completedAt
    ? formatDuration(outcome.startedAt, outcome.completedAt)
    : null;

  return (
    <div style={{
      background: "#27272a",
      border: `1px solid ${borderColor}`,
      borderRadius: hasBlocks ? 8 : 20,
      fontSize: 10,
      padding: hasBlocks ? "6px 10px" : "4px 12px",
      width: HANDLER_WIDTH,
      color: "#a1a1aa",
      fontStyle: "italic",
      animation: isRunning ? "pulse 2s ease-in-out infinite" : undefined,
    }}>
      <Handle type="target" position={Position.Top} style={{ visibility: "hidden" }} />
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <span>{d.label}</span>
        {duration && <span style={{ fontSize: 9, color: "#71717a", fontStyle: "normal" }}>{duration}</span>}
      </div>
      {hasBlocks && blocks.map((block, i) => <BlockRenderer key={i} block={block} />)}
      {outcome?.status === "error" && outcome.error && (
        <div style={{ fontSize: 9, color: "#ef4444", marginTop: 4 }}>{outcome.error}</div>
      )}
      <Handle type="source" position={Position.Bottom} style={{ visibility: "hidden" }} />
    </div>
  );
});

const nodeTypes = { handler: HandlerNode };

// ---------------------------------------------------------------------------
// Graph building
// ---------------------------------------------------------------------------

type FlowGraph = { nodes: Node[]; edges: Edge[] };

function buildFlowGraph(events: AdminEvent[], descriptions?: Map<string, Block[]>, outcomes?: Map<string, HandlerOutcome>): FlowGraph {
  // Group events by (handlerId, name) for event-type nodes
  // and create handler nodes from unique handlerIds
  const eventGroups = new Map<string, { name: string; layer: string; count: number; events: AdminEvent[] }>();
  const handlerIds = new Set<string>();
  const parentToHandler = new Map<string, Set<string>>(); // parentId -> set of handlerIds
  const handlerToChildren = new Map<string, Set<string>>(); // handlerId -> set of event group keys

  for (const evt of events) {
    const handler = evt.handlerId ?? "__root__";
    const groupKey = `${handler}::${evt.name}`;

    const group = eventGroups.get(groupKey);
    if (group) {
      group.count++;
      group.events.push(evt);
    } else {
      eventGroups.set(groupKey, { name: evt.name, layer: evt.layer, count: 1, events: [evt] });
    }

    if (evt.handlerId) {
      handlerIds.add(evt.handlerId);

      // Track handler -> child event groups
      const children = handlerToChildren.get(evt.handlerId) ?? new Set();
      children.add(groupKey);
      handlerToChildren.set(evt.handlerId, children);
    }

    // Track parent -> handler edges (which handlers consumed which events)
    if (evt.parentId && evt.handlerId) {
      const handlers = parentToHandler.get(evt.parentId) ?? new Set();
      handlers.add(evt.handlerId);
      parentToHandler.set(evt.parentId, handlers);
    }
  }

  // Build a map from event UUID to its group key
  const eventIdToGroup = new Map<string, string>();
  for (const [groupKey, group] of eventGroups) {
    for (const evt of group.events) {
      if (evt.id) {
        eventIdToGroup.set(evt.id, groupKey);
      }
    }
  }

  const nodes: Node[] = [];
  const edges: Edge[] = [];
  const edgeSet = new Set<string>(); // dedup edges

  // Create event-type nodes
  for (const [groupKey, group] of eventGroups) {
    nodes.push({
      id: `evt:${groupKey}`,
      type: "default",
      position: { x: 0, y: 0 },
      data: {
        label: `${group.name} (${group.count})`,
        nodeKind: "event-type" as const,
        handlerId: group.events[0]?.handlerId ?? null,
        eventName: group.name,
      },
      style: {
        background: eventBg(group.name),
        border: `1px solid ${eventBorder(group.name)}`,
        borderRadius: 6,
        fontSize: 11,
        padding: "6px 10px",
        width: NODE_WIDTH,
        color: "#e4e4e7",
      },
      sourcePosition: Position.Bottom,
      targetPosition: Position.Top,
    });
  }

  // Create handler nodes
  for (const handlerId of handlerIds) {
    const blocks = descriptions?.get(handlerId);
    const outcome = outcomes?.get(handlerId);
    nodes.push({
      id: `hdl:${handlerId}`,
      type: "handler",
      position: { x: 0, y: 0 },
      data: { label: handlerId, nodeKind: "handler" as const, handlerId, blocks, outcome },
      sourcePosition: Position.Bottom,
      targetPosition: Position.Top,
    });
  }

  const arrowMarker = { type: MarkerType.ArrowClosed, color: "#52525b", width: 16, height: 16 };

  // Edges: event group -> handler (via parentToHandler)
  for (const [parentId, handlers] of parentToHandler) {
    const sourceGroupKey = eventIdToGroup.get(parentId);
    if (!sourceGroupKey) continue;
    for (const handlerId of handlers) {
      const edgeKey = `evt:${sourceGroupKey}->hdl:${handlerId}`;
      if (!edgeSet.has(edgeKey)) {
        edgeSet.add(edgeKey);
        edges.push({
          id: edgeKey,
          source: `evt:${sourceGroupKey}`,
          target: `hdl:${handlerId}`,
          style: { stroke: "#52525b", strokeWidth: 1 },
          markerEnd: arrowMarker,
          animated: false,
        });
      }
    }
  }

  // Edges: handler -> child event groups (with count labels)
  for (const [handlerId, childGroupKeys] of handlerToChildren) {
    for (const groupKey of childGroupKeys) {
      const edgeKey = `hdl:${handlerId}->evt:${groupKey}`;
      if (!edgeSet.has(edgeKey)) {
        edgeSet.add(edgeKey);
        const count = eventGroups.get(groupKey)?.count ?? 0;
        edges.push({
          id: edgeKey,
          source: `hdl:${handlerId}`,
          target: `evt:${groupKey}`,
          style: { stroke: "#52525b", strokeWidth: 1 },
          markerEnd: arrowMarker,
          animated: false,
          ...(count > 1 ? { label: `×${count}`, labelStyle: { fontSize: 9, fill: "#71717a" } } : {}),
        });
      }
    }
  }

  // Root events (no handlerId) that are parents to handlers — add edges
  for (const [groupKey, group] of eventGroups) {
    if (group.events[0]?.handlerId) continue; // not a root group
    for (const evt of group.events) {
      if (!evt.id) continue;
      const handlers = parentToHandler.get(evt.id);
      if (!handlers) continue;
      for (const handlerId of handlers) {
        const edgeKey = `evt:${groupKey}->hdl:${handlerId}`;
        if (!edgeSet.has(edgeKey)) {
          edgeSet.add(edgeKey);
          edges.push({
            id: edgeKey,
            source: `evt:${groupKey}`,
            target: `hdl:${handlerId}`,
            style: { stroke: "#52525b", strokeWidth: 1 },
            markerEnd: arrowMarker,
          });
        }
      }
    }
  }

  return layoutGraph(nodes, edges);
}

function estimateHandlerHeight(data: FlowNodeData): number {
  if (data.nodeKind !== "handler") return HANDLER_HEIGHT;
  const hasBlocks = data.blocks && data.blocks.length > 0;
  const outcome = data.outcome;
  if (!hasBlocks && !outcome) return HANDLER_HEIGHT;
  let h = 24; // base label height
  if (data.blocks) {
    for (const block of data.blocks) {
      if (block.type === "checklist") {
        h += 14 + block.items.length * 12;
      } else {
        h += 14;
      }
    }
  }
  if (outcome?.status === "error" && outcome.error) h += 14;
  return h;
}

function layoutGraph(nodes: Node[], edges: Edge[]): FlowGraph {
  const g = new dagre.graphlib.Graph();
  g.setDefaultEdgeLabel(() => ({}));
  g.setGraph({ rankdir: "TB", ranksep: 60, nodesep: 30 });

  const heights = new Map<string, number>();
  for (const node of nodes) {
    const isHandler = node.id.startsWith("hdl:");
    const h = isHandler ? estimateHandlerHeight(node.data as FlowNodeData) : NODE_HEIGHT;
    heights.set(node.id, h);
    g.setNode(node.id, {
      width: isHandler ? HANDLER_WIDTH : NODE_WIDTH,
      height: h,
    });
  }

  for (const edge of edges) {
    g.setEdge(edge.source, edge.target);
  }

  dagre.layout(g);

  const laidOut = nodes.map((node) => {
    const pos = g.node(node.id);
    const isHandler = node.id.startsWith("hdl:");
    const w = isHandler ? HANDLER_WIDTH : NODE_WIDTH;
    const h = heights.get(node.id) ?? NODE_HEIGHT;
    return {
      ...node,
      position: { x: pos.x - w / 2, y: pos.y - h / 2 },
    };
  });

  return { nodes: laidOut, edges };
}

// ---------------------------------------------------------------------------
// Auto-center on selected tree event
// ---------------------------------------------------------------------------

function FocusOnSelection({ nodes, flowData }: { nodes: Node[]; flowData: AdminEvent[] | null }) {
  const { selectedSeq } = useEventsPaneContext();
  const { setCenter, getZoom } = useReactFlow();

  useEffect(() => {
    if (selectedSeq == null || !flowData) return;
    const evt = flowData.find(e => e.seq === selectedSeq);
    if (!evt) return;

    const handler = evt.handlerId ?? "__root__";
    const nodeId = `evt:${handler}::${evt.name}`;
    const node = nodes.find(n => n.id === nodeId);
    if (!node) return;

    const isHandler = node.id.startsWith("hdl:");
    const w = isHandler ? HANDLER_WIDTH : NODE_WIDTH;
    const h = isHandler ? estimateHandlerHeight(node.data as FlowNodeData) : NODE_HEIGHT;

    setCenter(
      node.position.x + w / 2,
      node.position.y + h / 2,
      { zoom: getZoom(), duration: 400 },
    );
  }, [selectedSeq, flowData, nodes, setCenter, getZoom]);

  return null;
}

// ---------------------------------------------------------------------------
// CausalFlowPane
// ---------------------------------------------------------------------------

export function CausalFlowPane() {
  const { flowRunId, closeFlow, flowData, flowLoading, flowSelection, setFlowSelection, selectSeq, setLogsFilter } = useEventsPaneContext();

  const { data: descData } = useQuery<{
    adminHandlerDescriptions: { handlerId: string; blocks: unknown }[];
  }>(ADMIN_HANDLER_DESCRIPTIONS, {
    variables: flowRunId ? { runId: flowRunId } : undefined,
    skip: !flowRunId,
    pollInterval: 5000,
  });

  const { data: outcomesData } = useQuery<{
    adminHandlerOutcomes: HandlerOutcome[];
  }>(ADMIN_HANDLER_OUTCOMES, {
    variables: flowRunId ? { runId: flowRunId } : undefined,
    skip: !flowRunId,
    pollInterval: 5000,
  });

  const { data: runData } = useQuery<{
    adminScoutRun: {
      startedAt: string;
      finishedAt: string | null;
      stats: {
        urlsScraped: number | null;
        urlsUnchanged: number | null;
        urlsFailed: number | null;
        signalsExtracted: number | null;
        handlerFailures: number | null;
      };
    } | null;
  }>(ADMIN_SCOUT_RUN, {
    variables: flowRunId ? { runId: flowRunId } : undefined,
    skip: !flowRunId,
    pollInterval: 5000,
  });

  const descriptions = useMemo(() => {
    if (!descData?.adminHandlerDescriptions) return undefined;
    const map = new Map<string, Block[]>();
    for (const d of descData.adminHandlerDescriptions) {
      map.set(d.handlerId, d.blocks as Block[]);
    }
    return map;
  }, [descData]);

  const outcomes = useMemo(() => {
    if (!outcomesData?.adminHandlerOutcomes) return undefined;
    const map = new Map<string, HandlerOutcome>();
    for (const o of outcomesData.adminHandlerOutcomes) {
      map.set(o.handlerId, o);
    }
    return map;
  }, [outcomesData]);

  const { nodes: rawNodes, edges } = useMemo(() => {
    if (!flowData || flowData.length === 0) return { nodes: [], edges: [] };
    return buildFlowGraph(flowData, descriptions, outcomes);
  }, [flowData, descriptions, outcomes]);

  // Derive selected node ID from flowSelection
  const selectedNodeId = useMemo(() => {
    if (!flowSelection) return null;
    if (flowSelection.kind === "handler") return `hdl:${flowSelection.handlerId}`;
    const handler = flowSelection.handlerId ?? "__root__";
    return `evt:${handler}::${flowSelection.name}`;
  }, [flowSelection]);

  // Apply selected state to matching node
  const nodes = useMemo(
    () => rawNodes.map(n => ({ ...n, selected: n.id === selectedNodeId })),
    [rawNodes, selectedNodeId],
  );

  const handleClose = useCallback(() => closeFlow(), [closeFlow]);

  // Find a representative event in flowData to load the right causal tree
  const syncTree = useCallback((d: FlowNodeData) => {
    if (!flowData) return;
    const match = d.nodeKind === "event-type"
      ? flowData.find(e => e.handlerId === d.handlerId && e.name === d.eventName)
      : flowData.find(e => e.handlerId === d.handlerId);
    if (match) selectSeq(match.seq, match.runId ?? undefined);
  }, [flowData, selectSeq]);

  const openLogsForHandler = useCallback((handlerId: string) => {
    if (!flowData) return;
    const evt = flowData.find(e => e.handlerId === handlerId && e.parentId);
    if (evt) {
      setLogsFilter({ eventId: evt.parentId!, handlerId, runId: evt.runId });
    }
  }, [flowData, setLogsFilter]);

  const onNodeClick = useCallback((_event: React.MouseEvent, node: Node) => {
    const d = node.data as FlowNodeData;

    if (d.nodeKind === "event-type") {
      if (
        flowSelection?.kind === "event-type" &&
        flowSelection.handlerId === d.handlerId &&
        flowSelection.name === d.eventName
      ) {
        setFlowSelection(null);
      } else {
        setFlowSelection({ kind: "event-type", handlerId: d.handlerId, name: d.eventName });
        syncTree(d);
      }
    } else if (d.nodeKind === "handler") {
      if (flowSelection?.kind === "handler" && flowSelection.handlerId === d.handlerId) {
        setFlowSelection(null);
      } else {
        setFlowSelection({ kind: "handler", handlerId: d.handlerId });
        syncTree(d);
        openLogsForHandler(d.handlerId);
      }
    }
  }, [flowSelection, setFlowSelection, syncTree, openLogsForHandler]);

  const onPaneClick = useCallback(() => setFlowSelection(null), [setFlowSelection]);

  // No-op: we manage node state ourselves, but React Flow needs this in controlled mode
  const onNodesChange = useCallback((_changes: NodeChange[]) => {}, []);

  if (!flowRunId) {
    return (
      <div className="flex items-center justify-center h-full text-sm text-muted-foreground">
        Click "View Flow" on a timeline event with a run_id to visualize its causal decision tree
      </div>
    );
  }

  if (flowLoading) {
    return (
      <div className="h-full flex flex-col">
        <div className="flex items-center gap-2 px-3 py-1.5 border-b border-border shrink-0">
          <div className="h-3 w-10 bg-muted rounded animate-pulse" />
          <div className="h-3 w-48 bg-muted rounded animate-pulse" />
        </div>
        <div className="flex-1 flex items-center justify-center">
          <div className="animate-pulse flex flex-col items-center gap-3">
            {/* Root node */}
            <div className="h-8 w-40 bg-muted rounded-md" />
            {/* Connector */}
            <div className="h-6 w-px bg-muted" />
            {/* Handler */}
            <div className="h-6 w-28 bg-muted rounded-full" />
            {/* Fork */}
            <div className="flex items-start gap-8">
              <div className="flex flex-col items-center gap-3">
                <div className="h-6 w-px bg-muted" />
                <div className="h-8 w-36 bg-muted rounded-md" />
                <div className="h-6 w-px bg-muted" />
                <div className="h-6 w-24 bg-muted rounded-full" />
              </div>
              <div className="flex flex-col items-center gap-3">
                <div className="h-6 w-px bg-muted" />
                <div className="h-8 w-36 bg-muted rounded-md" />
              </div>
            </div>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col">
      <div className="flex items-center gap-2 px-3 py-1.5 border-b border-border shrink-0">
        <h3 className="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
          Flow
        </h3>
        <span className="text-xs font-mono text-foreground truncate">{flowRunId}</span>
        <span className="text-xs text-muted-foreground">
          {flowData?.length ?? 0} events, {nodes.length} nodes
        </span>
        {runData?.adminScoutRun && (() => {
          const run = runData.adminScoutRun;
          const s = run.stats;
          const elapsed = run.finishedAt
            ? `${((new Date(run.finishedAt).getTime() - new Date(run.startedAt).getTime()) / 1000).toFixed(0)}s`
            : "running\u2026";
          return (
            <span className="text-xs text-muted-foreground font-mono">
              signals: {s.signalsExtracted ?? 0}
              {" | "}urls: {s.urlsScraped ?? 0}/{s.urlsUnchanged ?? 0}/{s.urlsFailed ?? 0}
              {" | "}failures: {s.handlerFailures ?? 0}
              {" | "}{elapsed}
            </span>
          );
        })()}
        <button
          onClick={handleClose}
          className="ml-auto text-xs text-muted-foreground hover:text-foreground px-1"
        >
          Close
        </button>
      </div>
      <div className="flex-1">
        <ReactFlow
          nodes={nodes}
          edges={edges}
          nodeTypes={nodeTypes}
          onNodesChange={onNodesChange}
          onNodeClick={onNodeClick}
          onPaneClick={onPaneClick}
          fitView
          proOptions={{ hideAttribution: true }}
          nodesDraggable={false}
          nodesConnectable={false}
          colorMode="dark"
        >
          <FocusOnSelection nodes={nodes} flowData={flowData} />
          <Background color="#27272a" gap={20} />
          <Controls showInteractive={false} />
        </ReactFlow>
      </div>
    </div>
  );
}
