import { useState } from "react";
import { useQuery } from "@apollo/client";
import { ADMIN_NODE_EVENTS, SOURCE_DETAIL } from "@/graphql/queries";

type InspectorTab = "properties" | "relationships" | "logs" | "tree";

type GraphNodeInfo = {
  id: string;
  nodeType: string;
  label: string;
  lat?: number | null;
  lng?: number | null;
  confidence?: number | null;
  metadata: string;
};

type GraphEdgeInfo = {
  sourceId: string;
  targetId: string;
  edgeType: string;
};

export function InspectorPane({
  selectedNode,
  edges,
  nodeMap,
  collapsed,
  onToggleCollapse,
}: {
  selectedNode: GraphNodeInfo | null;
  edges: GraphEdgeInfo[];
  nodeMap: Map<string, GraphNodeInfo>;
  collapsed: boolean;
  onToggleCollapse: () => void;
}) {
  const [tab, setTab] = useState<InspectorTab>("properties");

  // Find edges connected to the selected node
  const connectedEdges = selectedNode
    ? edges.filter(
        (e) => e.sourceId === selectedNode.id || e.targetId === selectedNode.id,
      )
    : [];

  const tabs: { key: InspectorTab; label: string }[] = [
    { key: "properties", label: "Properties" },
    { key: "relationships", label: `Relationships (${connectedEdges.length})` },
    { key: "logs", label: "Logs" },
    { key: "tree", label: "Tree" },
  ];

  return (
    <div className="border-t border-border bg-card z-50">
      {/* Header */}
      <button
        onClick={onToggleCollapse}
        className="w-full flex items-center justify-between px-4 py-2 text-sm hover:bg-accent/30"
      >
        <div className="flex items-center gap-3">
          <span className="text-muted-foreground">{collapsed ? "▸" : "▾"} Inspector</span>
          {!collapsed &&
            tabs.map((t) => (
              <button
                key={t.key}
                onClick={(e) => {
                  e.stopPropagation();
                  setTab(t.key);
                }}
                className={`px-2 py-0.5 rounded text-xs ${
                  tab === t.key
                    ? "bg-accent text-accent-foreground"
                    : "text-muted-foreground hover:text-foreground"
                }`}
              >
                {t.label}
              </button>
            ))}
        </div>
        <span className="text-xs text-muted-foreground">
          {selectedNode ? selectedNode.label : "No node selected"}
        </span>
      </button>

      {/* Body */}
      {!collapsed && (
        <div className="px-4 pb-3 max-h-[250px] overflow-y-auto text-sm">
          {!selectedNode ? (
            <p className="text-muted-foreground text-xs py-2">
              Click a node in the graph to inspect it.
            </p>
          ) : tab === "properties" ? (
            <PropertiesTab node={selectedNode} />
          ) : tab === "relationships" ? (
            <RelationshipsTab
              edges={connectedEdges}
              selectedId={selectedNode.id}
              nodeMap={nodeMap}
            />
          ) : tab === "logs" ? (
            <LogsTab nodeId={selectedNode.id} />
          ) : (
            <TreeTab node={selectedNode} />
          )}
        </div>
      )}
    </div>
  );
}

function PropertiesTab({ node }: { node: GraphNodeInfo }) {
  let metadata: Record<string, unknown> = {};
  try {
    metadata = JSON.parse(node.metadata);
  } catch {
    // ignore
  }

  const rows: [string, string][] = [
    ["ID", node.id],
    ["Type", node.nodeType],
    ["Label", node.label],
  ];

  if (node.confidence != null) {
    rows.push(["Confidence", `${(node.confidence * 100).toFixed(0)}%`]);
  }
  if (node.lat != null && node.lng != null) {
    rows.push(["Location", `${node.lat.toFixed(4)}, ${node.lng.toFixed(4)}`]);
  }

  for (const [key, val] of Object.entries(metadata)) {
    if (val != null && val !== "") {
      rows.push([key, String(val)]);
    }
  }

  return (
    <table className="w-full text-xs">
      <tbody>
        {rows.map(([k, v]) => (
          <tr key={k} className="border-b border-border/30">
            <td className="py-1 pr-4 text-muted-foreground whitespace-nowrap font-medium">
              {k}
            </td>
            <td className="py-1 break-all">{v}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function RelationshipsTab({
  edges,
  selectedId,
  nodeMap,
}: {
  edges: GraphEdgeInfo[];
  selectedId: string;
  nodeMap: Map<string, GraphNodeInfo>;
}) {
  if (edges.length === 0) {
    return <p className="text-muted-foreground text-xs py-2">No relationships.</p>;
  }

  return (
    <table className="w-full text-xs">
      <thead>
        <tr className="text-muted-foreground border-b border-border/50">
          <th className="py-1 text-left font-medium">Direction</th>
          <th className="py-1 text-left font-medium">Edge Type</th>
          <th className="py-1 text-left font-medium">Node</th>
          <th className="py-1 text-left font-medium">Type</th>
        </tr>
      </thead>
      <tbody>
        {edges.map((e, i) => {
          const isOutgoing = e.sourceId === selectedId;
          const otherId = isOutgoing ? e.targetId : e.sourceId;
          const other = nodeMap.get(otherId);
          return (
            <tr key={i} className="border-b border-border/30">
              <td className="py-1 text-muted-foreground">
                {isOutgoing ? "→ out" : "← in"}
              </td>
              <td className="py-1">{e.edgeType}</td>
              <td className="py-1 max-w-[200px] truncate">
                {other?.label ?? otherId.slice(0, 8) + "..."}
              </td>
              <td className="py-1 text-muted-foreground">{other?.nodeType ?? "?"}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

type ScoutEvent = {
  id: string;
  parentId: string | null;
  seq: number | null;
  ts: string;
  type: string;
  sourceUrl: string | null;
  query: string | null;
  url: string | null;
  signalType: string | null;
  title: string | null;
  confidence: number | null;
  success: boolean | null;
  action: string | null;
  nodeId: string | null;
  matchedId: string | null;
  existingId: string | null;
  reason: string | null;
  field: string | null;
  oldValue: string | null;
  newValue: string | null;
  summary: string | null;
};

const EVENT_TYPE_COLORS: Record<string, string> = {
  extract: "bg-blue-500/20 text-blue-400",
  dedup: "bg-amber-500/20 text-amber-400",
  store: "bg-green-500/20 text-green-400",
  lint: "bg-purple-500/20 text-purple-400",
  scrape: "bg-cyan-500/20 text-cyan-400",
  search: "bg-indigo-500/20 text-indigo-400",
  enrich: "bg-pink-500/20 text-pink-400",
};

function LogsTab({ nodeId }: { nodeId: string }) {
  const { data, loading } = useQuery(ADMIN_NODE_EVENTS, {
    variables: { nodeId, limit: 100 },
  });

  const events: ScoutEvent[] = data?.adminNodeEvents ?? [];

  if (loading) {
    return <p className="text-muted-foreground text-xs py-2">Loading events...</p>;
  }

  if (events.length === 0) {
    return <p className="text-muted-foreground text-xs py-2">No scout events found for this node.</p>;
  }

  // Group events by parent (top-level events have no parentId)
  const topLevel = events.filter((e) => !e.parentId);
  const childrenMap = new Map<string, ScoutEvent[]>();
  for (const e of events) {
    if (e.parentId) {
      const arr = childrenMap.get(e.parentId) ?? [];
      arr.push(e);
      childrenMap.set(e.parentId, arr);
    }
  }

  return (
    <div className="space-y-1">
      <div className="text-[10px] text-muted-foreground mb-2">
        {events.length} event{events.length !== 1 ? "s" : ""} across scout runs
      </div>
      {(topLevel.length > 0 ? topLevel : events).map((event) => (
        <EventRow key={event.id} event={event} childrenMap={childrenMap} depth={0} />
      ))}
    </div>
  );
}

function EventRow({
  event,
  childrenMap,
  depth,
}: {
  event: ScoutEvent;
  childrenMap: Map<string, ScoutEvent[]>;
  depth: number;
}) {
  const [expanded, setExpanded] = useState(depth === 0);
  const children = childrenMap.get(event.id) ?? [];
  const colorClass = EVENT_TYPE_COLORS[event.type] ?? "bg-zinc-500/20 text-zinc-400";
  const ts = new Date(event.ts).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });

  const detail =
    event.summary ??
    event.title ??
    event.action ??
    event.reason ??
    (event.field ? `${event.field}: ${event.oldValue} → ${event.newValue}` : null);

  return (
    <div style={{ marginLeft: depth * 16 }}>
      <div
        className="flex items-center gap-2 py-0.5 text-xs cursor-default hover:bg-accent/20 rounded px-1"
        onClick={() => children.length > 0 && setExpanded(!expanded)}
      >
        {children.length > 0 ? (
          <span className="text-muted-foreground w-3 text-center cursor-pointer">
            {expanded ? "▾" : "▸"}
          </span>
        ) : (
          <span className="w-3" />
        )}
        <span className="text-[10px] text-muted-foreground tabular-nums whitespace-nowrap">
          {ts}
        </span>
        <span className={`px-1.5 py-0.5 rounded text-[10px] font-medium ${colorClass}`}>
          {event.type}
        </span>
        {detail && (
          <span className="truncate text-foreground/80">{detail}</span>
        )}
        {event.confidence != null && (
          <span className="text-[10px] text-muted-foreground ml-auto tabular-nums">
            {(event.confidence * 100).toFixed(0)}%
          </span>
        )}
      </div>
      {expanded &&
        children.map((child) => (
          <EventRow key={child.id} event={child} childrenMap={childrenMap} depth={depth + 1} />
        ))}
    </div>
  );
}

type TreeNode = {
  id: string;
  canonicalValue: string;
  discoveryMethod: string;
  active: boolean;
  signalsProduced: number;
};

type TreeEdge = {
  childId: string;
  parentId: string;
};

type DiscoveryTree = {
  nodes: TreeNode[];
  edges: TreeEdge[];
  rootId: string;
};

function TreeTab({ node }: { node: GraphNodeInfo }) {
  // Parse metadata to get a source ID if this is a citation node with sourceId
  let metadata: Record<string, unknown> = {};
  try {
    metadata = JSON.parse(node.metadata);
  } catch {
    // ignore
  }

  // The tree tab is relevant for source-related nodes (Citations link to sources)
  // For signals, we can show the event tree from the Logs tab instead
  const sourceId = metadata.sourceId as string | undefined;

  // Only query if we have something to look up
  const { data, loading } = useQuery(SOURCE_DETAIL, {
    variables: { id: sourceId ?? node.id },
    skip: !sourceId && node.nodeType !== "Citation",
  });

  const tree: DiscoveryTree | null = data?.sourceDetail?.discoveryTree ?? null;

  if (node.nodeType !== "Citation" && !sourceId) {
    return (
      <p className="text-muted-foreground text-xs py-2">
        Tree view is available for source/citation nodes. Select a citation to see its discovery lineage.
      </p>
    );
  }

  if (loading) {
    return <p className="text-muted-foreground text-xs py-2">Loading tree...</p>;
  }

  if (!tree || tree.edges.length === 0) {
    return <p className="text-muted-foreground text-xs py-2">No discovery lineage.</p>;
  }

  const nodesById = new Map(tree.nodes.map((n) => [n.id, n]));
  const childrenOf = new Map<string, string[]>();
  for (const edge of tree.edges) {
    const children = childrenOf.get(edge.parentId) ?? [];
    children.push(edge.childId);
    childrenOf.set(edge.parentId, children);
  }

  // Find the topmost root (walk up from rootId)
  const parentOf = new Map<string, string>();
  for (const edge of tree.edges) {
    parentOf.set(edge.childId, edge.parentId);
  }
  let topRoot = tree.rootId;
  while (parentOf.has(topRoot)) {
    topRoot = parentOf.get(topRoot)!;
  }

  return (
    <div className="space-y-0.5">
      <DiscoveryTreeNode
        id={topRoot}
        nodesById={nodesById}
        childrenOf={childrenOf}
        currentId={sourceId ?? node.id}
        depth={0}
      />
    </div>
  );
}

function DiscoveryTreeNode({
  id,
  nodesById,
  childrenOf,
  currentId,
  depth,
}: {
  id: string;
  nodesById: Map<string, TreeNode>;
  childrenOf: Map<string, string[]>;
  currentId: string;
  depth: number;
}) {
  const node = nodesById.get(id);
  const children = childrenOf.get(id) ?? [];
  const isCurrent = id === currentId;

  if (!node) return null;

  return (
    <>
      <div
        className="flex items-center gap-2 py-0.5 text-xs"
        style={{ paddingLeft: depth * 16 }}
      >
        {depth > 0 && <span className="text-muted-foreground">└─</span>}
        <span className={isCurrent ? "text-blue-400 font-medium" : "text-foreground"}>
          {node.canonicalValue}
        </span>
        <span className="text-[10px] text-muted-foreground tabular-nums">
          {node.signalsProduced} signals
        </span>
        {!node.active && (
          <span className="text-[10px] px-1 py-0.5 rounded bg-muted text-muted-foreground">
            Inactive
          </span>
        )}
        <span className="text-[10px] text-muted-foreground">{node.discoveryMethod}</span>
      </div>
      {children.map((childId) => (
        <DiscoveryTreeNode
          key={childId}
          id={childId}
          nodesById={nodesById}
          childrenOf={childrenOf}
          currentId={currentId}
          depth={depth + 1}
        />
      ))}
    </>
  );
}
