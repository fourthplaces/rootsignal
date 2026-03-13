import { useMemo, useState } from "react";
import { useQuery } from "@apollo/client";
import { Search } from "lucide-react";
import { ADMIN_NODE_EVENTS, SOURCE_DETAIL } from "@/graphql/queries";

const NODE_TYPE_OPTIONS = [
  { key: "Gathering", label: "Gathering", color: "#3b82f6" },
  { key: "Resource", label: "Resource", color: "#22c55e" },
  { key: "HelpRequest", label: "Help", color: "#f59e0b" },
  { key: "Announcement", label: "Announce", color: "#a855f7" },
  { key: "Concern", label: "Concern", color: "#ef4444" },
  { key: "Actor", label: "Actor", color: "#ec4899" },
  { key: "Location", label: "Location", color: "#14b8a6" },
  { key: "Citation", label: "Citation", color: "#6b7280" },
] as const;

type NodeMetadata = {
  id: string;
  metadata: string;
};

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

function CollapsibleSection({
  title,
  defaultOpen = true,
  children,
}: {
  title: string;
  defaultOpen?: boolean;
  children: React.ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div>
      <button
        onClick={() => setOpen(!open)}
        className="w-full flex items-center gap-1.5 py-1.5 text-xs font-semibold uppercase tracking-wider text-muted-foreground hover:text-foreground transition-colors"
      >
        <span className="w-3 text-center">{open ? "▾" : "▸"}</span>
        {title}
      </button>
      {open && <div className="space-y-3 pb-3">{children}</div>}
    </div>
  );
}

export function FilterSidebar({
  nodeTypes,
  onToggleNodeType,
  timeFrom,
  timeTo,
  onTimeFromChange,
  onTimeToChange,
  search,
  onSearchChange,
  totalCount,
  visibleCount,
  nodeCounts,
  allNodes,
  selectedNode,
  edges,
  nodeMap,
  onInvestigate,
}: {
  nodeTypes: Set<string>;
  onToggleNodeType: (type: string) => void;
  timeFrom: string;
  timeTo: string;
  onTimeFromChange: (d: string) => void;
  onTimeToChange: (d: string) => void;
  search: string;
  onSearchChange: (s: string) => void;
  totalCount: number;
  visibleCount: number;
  nodeCounts: Record<string, number>;
  allNodes: NodeMetadata[];
  selectedNode: GraphNodeInfo | null;
  edges: GraphEdgeInfo[];
  nodeMap: Map<string, GraphNodeInfo>;
  onInvestigate?: (node: GraphNodeInfo) => void;
}) {
  const histogram = useMemo(() => {
    const counts = new Map<string, number>();
    for (const node of allNodes) {
      try {
        const meta = JSON.parse(node.metadata);
        const dateStr = (meta.extractedAt ?? meta.firstSeen ?? "").slice(0, 10);
        if (dateStr) {
          counts.set(dateStr, (counts.get(dateStr) ?? 0) + 1);
        }
      } catch {
        // skip
      }
    }
    if (counts.size === 0) return [];

    const start = new Date(timeFrom);
    const end = new Date(timeTo);
    const days: { day: string; count: number }[] = [];
    const d = new Date(start);
    while (d <= end) {
      const key = d.toISOString().slice(0, 10);
      days.push({ day: key, count: counts.get(key) ?? 0 });
      d.setDate(d.getDate() + 1);
    }
    return days;
  }, [allNodes, timeFrom, timeTo]);

  const maxCount = useMemo(
    () => Math.max(1, ...histogram.map((d) => d.count)),
    [histogram],
  );

  return (
    <div className="h-full border-l border-border bg-card p-3 space-y-1 overflow-y-auto text-sm">
      {/* Search & Filters */}
      <CollapsibleSection title="Search & Filters">
        <div>
          <input
            type="text"
            placeholder="Search nodes..."
            value={search}
            onChange={(e) => onSearchChange(e.target.value)}
            className="w-full px-2 py-1.5 rounded border border-input bg-background text-xs"
          />
        </div>

        <div className="space-y-1.5">
          <label className="text-xs text-muted-foreground font-medium">Time window</label>
          {histogram.length > 0 && (
            <div className="flex items-end gap-px h-8 px-0.5">
              {histogram.map((d) => (
                <div
                  key={d.day}
                  className="flex-1 bg-indigo-500/40 rounded-t-sm min-w-[2px] transition-all"
                  style={{ height: `${(d.count / maxCount) * 100}%` }}
                  title={`${d.day}: ${d.count} nodes`}
                />
              ))}
            </div>
          )}
          <div className="space-y-1">
            <input
              type="date"
              value={timeFrom}
              onChange={(e) => onTimeFromChange(e.target.value)}
              className="w-full px-2 py-1 rounded border border-input bg-background text-xs"
            />
            <input
              type="date"
              value={timeTo}
              onChange={(e) => onTimeToChange(e.target.value)}
              className="w-full px-2 py-1 rounded border border-input bg-background text-xs"
            />
          </div>
        </div>

        <div className="text-[10px] text-muted-foreground">
          Showing {visibleCount} of {totalCount}
        </div>

        <div className="space-y-1.5">
          <label className="text-xs text-muted-foreground font-medium">Node types</label>
          <div className="flex flex-wrap gap-1.5">
            {NODE_TYPE_OPTIONS.map((opt) => {
              const active = nodeTypes.has(opt.key);
              const count = nodeCounts[opt.key] ?? 0;
              return (
                <button
                  key={opt.key}
                  onClick={() => onToggleNodeType(opt.key)}
                  className="inline-flex items-center gap-1 px-2 py-1 rounded-full text-[11px] font-medium transition-all cursor-pointer border"
                  style={
                    active
                      ? {
                          backgroundColor: opt.color + "25",
                          borderColor: opt.color + "60",
                          color: opt.color,
                        }
                      : {
                          backgroundColor: "transparent",
                          borderColor: "rgba(255,255,255,0.1)",
                          color: "rgba(255,255,255,0.3)",
                        }
                  }
                >
                  <span
                    className="w-2 h-2 rounded-full shrink-0"
                    style={{ backgroundColor: active ? opt.color : "rgba(255,255,255,0.2)" }}
                  />
                  {opt.label}
                  {count > 0 && (
                    <span className="text-[9px] opacity-70 tabular-nums">{count}</span>
                  )}
                </button>
              );
            })}
          </div>
        </div>
      </CollapsibleSection>

      <div className="border-t border-border/50" />

      {/* Properties */}
      <CollapsibleSection title="Properties" defaultOpen={!!selectedNode}>
        {!selectedNode ? (
          <p className="text-muted-foreground text-xs">Click a node to inspect it.</p>
        ) : (
          <PropertiesContent node={selectedNode} edges={edges} nodeMap={nodeMap} onInvestigate={onInvestigate} />
        )}
      </CollapsibleSection>
    </div>
  );
}

// --- Properties content (tabs: properties, relationships, logs, tree) ---

type InspectorTab = "properties" | "relationships" | "logs" | "tree";

function PropertiesContent({
  node,
  edges,
  nodeMap,
  onInvestigate,
}: {
  node: GraphNodeInfo;
  edges: GraphEdgeInfo[];
  nodeMap: Map<string, GraphNodeInfo>;
  onInvestigate?: (node: GraphNodeInfo) => void;
}) {
  const [tab, setTab] = useState<InspectorTab>("properties");

  const connectedEdges = edges.filter(
    (e) => e.sourceId === node.id || e.targetId === node.id,
  );

  const tabs: { key: InspectorTab; label: string }[] = [
    { key: "properties", label: "Props" },
    { key: "relationships", label: `Rels (${connectedEdges.length})` },
    { key: "logs", label: "Logs" },
    { key: "tree", label: "Tree" },
  ];

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-1.5">
        <div className="text-xs font-medium truncate text-foreground flex-1">{node.label}</div>
        {onInvestigate && (
          <button
            onClick={() => onInvestigate(node)}
            title="Investigate this node with AI"
            className="shrink-0 p-1 rounded hover:bg-accent text-muted-foreground hover:text-foreground transition-colors"
          >
            <Search className="w-3.5 h-3.5" />
          </button>
        )}
      </div>
      <div className="flex gap-1">
        {tabs.map((t) => (
          <button
            key={t.key}
            onClick={() => setTab(t.key)}
            className={`px-1.5 py-0.5 rounded text-[10px] ${
              tab === t.key
                ? "bg-accent text-accent-foreground"
                : "text-muted-foreground hover:text-foreground"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>
      {tab === "properties" ? (
        <PropertiesTab node={node} />
      ) : tab === "relationships" ? (
        <RelationshipsTab edges={connectedEdges} selectedId={node.id} nodeMap={nodeMap} />
      ) : tab === "logs" ? (
        <LogsTab nodeId={node.id} />
      ) : (
        <TreeTab node={node} />
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
            <td className="py-1 pr-3 text-muted-foreground whitespace-nowrap font-medium">
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
    return <p className="text-muted-foreground text-xs">No relationships.</p>;
  }

  return (
    <div className="space-y-1">
      {edges.map((e, i) => {
        const isOutgoing = e.sourceId === selectedId;
        const otherId = isOutgoing ? e.targetId : e.sourceId;
        const other = nodeMap.get(otherId);
        return (
          <div key={i} className="flex items-center gap-1.5 text-xs">
            <span className="text-muted-foreground text-[10px]">
              {isOutgoing ? "→" : "←"}
            </span>
            <span className="text-[10px] text-muted-foreground">{e.edgeType}</span>
            <span className="truncate">{other?.label ?? otherId.slice(0, 8) + "..."}</span>
          </div>
        );
      })}
    </div>
  );
}

// --- Logs & Tree tabs (moved from InspectorPane) ---

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
    return <p className="text-muted-foreground text-xs">Loading events...</p>;
  }

  if (events.length === 0) {
    return <p className="text-muted-foreground text-xs">No scout events found.</p>;
  }

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
    <div className="space-y-0.5">
      <div className="text-[10px] text-muted-foreground mb-1">
        {events.length} event{events.length !== 1 ? "s" : ""}
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
    <div style={{ marginLeft: depth * 12 }}>
      <div
        className="flex items-center gap-1.5 py-0.5 text-[10px] cursor-default hover:bg-accent/20 rounded px-0.5"
        onClick={() => children.length > 0 && setExpanded(!expanded)}
      >
        {children.length > 0 ? (
          <span className="text-muted-foreground w-2.5 text-center cursor-pointer">
            {expanded ? "▾" : "▸"}
          </span>
        ) : (
          <span className="w-2.5" />
        )}
        <span className="text-muted-foreground tabular-nums whitespace-nowrap">{ts}</span>
        <span className={`px-1 py-0.5 rounded font-medium ${colorClass}`}>{event.type}</span>
        {detail && <span className="truncate text-foreground/80">{detail}</span>}
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
  let metadata: Record<string, unknown> = {};
  try {
    metadata = JSON.parse(node.metadata);
  } catch {
    // ignore
  }

  const sourceId = metadata.sourceId as string | undefined;

  const { data, loading } = useQuery(SOURCE_DETAIL, {
    variables: { id: sourceId ?? node.id },
    skip: !sourceId && node.nodeType !== "Citation",
  });

  const tree: DiscoveryTree | null = data?.sourceDetail?.discoveryTree ?? null;

  if (node.nodeType !== "Citation" && !sourceId) {
    return <p className="text-muted-foreground text-xs">Available for citation nodes.</p>;
  }

  if (loading) {
    return <p className="text-muted-foreground text-xs">Loading tree...</p>;
  }

  if (!tree || tree.edges.length === 0) {
    return <p className="text-muted-foreground text-xs">No discovery lineage.</p>;
  }

  const nodesById = new Map(tree.nodes.map((n) => [n.id, n]));
  const childrenOf = new Map<string, string[]>();
  for (const edge of tree.edges) {
    const children = childrenOf.get(edge.parentId) ?? [];
    children.push(edge.childId);
    childrenOf.set(edge.parentId, children);
  }

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
      <div className="flex items-center gap-1.5 py-0.5 text-xs" style={{ paddingLeft: depth * 12 }}>
        {depth > 0 && <span className="text-muted-foreground">└</span>}
        <span className={isCurrent ? "text-blue-400 font-medium" : "text-foreground"}>
          {node.canonicalValue}
        </span>
        <span className="text-[10px] text-muted-foreground tabular-nums">
          {node.signalsProduced}
        </span>
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
