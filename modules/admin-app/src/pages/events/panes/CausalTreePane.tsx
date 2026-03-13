import { useState, useMemo, useRef, useEffect, useCallback } from "react";
import { Search, Copy, Check } from "lucide-react";
import { useEventsPaneContext, type AdminEvent, type FlowSelection } from "../EventsPaneContext";
import { eventTextColor } from "../eventColor";
import { CopyablePayload } from "./TimelinePane";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

function copyToClipboard(text: string) {
  if (navigator.clipboard?.writeText) {
    navigator.clipboard.writeText(text);
    return;
  }
  const ta = document.createElement("textarea");
  ta.value = text;
  ta.style.position = "fixed";
  ta.style.opacity = "0";
  document.body.appendChild(ta);
  ta.select();
  document.execCommand("copy");
  document.body.removeChild(ta);
}

const LAYER_COLORS: Record<string, string> = {
  world: "bg-blue-500/20 text-blue-400",
  system: "bg-amber-500/20 text-amber-400",
  telemetry: "bg-zinc-500/20 text-zinc-400",
};

function formatTs(ts: string): string {
  return new Date(ts).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function compactPayload(raw: string, maxLen = 80): string {
  try {
    const obj = JSON.parse(raw);
    if (typeof obj !== "object" || obj === null) return raw.slice(0, maxLen);
    const entries = Object.entries(obj).filter(([k]) => k !== "type");
    if (entries.length === 0) return "{}";
    const parts: string[] = [];
    let len = 2;
    for (const [k, v] of entries) {
      const val =
        typeof v === "string"
          ? v.length > 60
            ? `"${v.slice(0, 57)}…"`
            : `"${v}"`
          : JSON.stringify(v);
      const part = `${k}: ${val}`;
      if (len + part.length + 2 > maxLen) {
        parts.push("…");
        break;
      }
      parts.push(part);
      len += part.length + 2;
    }
    return `{ ${parts.join(", ")} }`;
  } catch {
    return raw.slice(0, maxLen);
  }
}

// ---------------------------------------------------------------------------
// HandlerNode — intermediate node grouping children by handler_id
// ---------------------------------------------------------------------------

function HandlerNode({
  handlerId,
  parentEventId,
  children,
  childrenMap,
  depth,
  isHighlighted,
}: {
  handlerId: string;
  parentEventId: string;
  children: AdminEvent[];
  childrenMap: Map<string, AdminEvent[]>;
  depth: number;
  isHighlighted: boolean;
}) {
  const { setFlowSelection, flowRunId, setLogsFilter } = useEventsPaneContext();
  const [collapsed, setCollapsed] = useState(false);
  const nodeRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (isHighlighted && nodeRef.current) {
      nodeRef.current.scrollIntoView({ behavior: "smooth", block: "nearest" });
    }
  }, [isHighlighted]);

  const handleClick = useCallback(() => {
    if (flowRunId) {
      setFlowSelection({ kind: "handler", handlerId });
    }
    const runId = children[0]?.runId ?? null;
    setLogsFilter({ eventId: parentEventId, handlerId, runId });
  }, [flowRunId, setFlowSelection, setLogsFilter, parentEventId, handlerId, children]);

  return (
    <div className={depth > 0 ? "pl-6" : ""}>
      <div
        ref={isHighlighted ? nodeRef : undefined}
        className={`group/tree w-full text-left px-2 py-1 rounded transition-colors hover:bg-accent/30 ${
          isHighlighted ? "bg-zinc-700/40 ring-1 ring-zinc-500/50" : ""
        }`}
      >
        <div className="flex items-center gap-1.5 min-w-0">
          <button
            onClick={(e) => { e.stopPropagation(); setCollapsed(v => !v); }}
            className="text-[10px] text-muted-foreground hover:text-foreground shrink-0 w-3 text-center"
          >
            {collapsed ? "▸" : "▾"}
          </button>
          <button
            onClick={handleClick}
            className="flex items-center gap-1.5 min-w-0"
          >
            <span className="px-1 py-0.5 rounded text-[10px] font-medium shrink-0 bg-zinc-600/30 text-zinc-400 italic">
              handler
            </span>
            <span className="text-[10px] font-mono text-zinc-300 shrink-0">
              {handlerId}
            </span>
            {collapsed && (
              <span className="text-[10px] text-muted-foreground shrink-0">
                ({children.length})
              </span>
            )}
          </button>
        </div>
      </div>

      {!collapsed && children.map((child) => (
        <TreeNode
          key={child.seq}
          event={child}
          childrenMap={childrenMap}
          depth={depth + 1}
        />
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// TreeNode (recursive) — renders an event, grouping children by handler_id
// ---------------------------------------------------------------------------

function TreeNode({
  event,
  childrenMap,
  depth,
}: {
  event: AdminEvent;
  childrenMap: Map<string, AdminEvent[]>;
  depth: number;
}) {
  const { selectedSeq, selectSeq, setInvestigation, treeEvents, flowSelection } = useEventsPaneContext();
  const [payloadOpen, setPayloadOpen] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  const [copied, setCopied] = useState(false);
  const isSelected = event.seq === selectedSeq;
  const layerColor = LAYER_COLORS[event.layer] ?? "bg-zinc-500/20 text-zinc-400";
  const children = event.id ? (childrenMap.get(event.id) ?? []) : [];
  const hasChildren = children.length > 0;
  const nodeRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (isSelected && nodeRef.current) {
      nodeRef.current.scrollIntoView({ behavior: "smooth", block: "nearest" });
    }
  }, [isSelected]);

  const handleInvestigate = useCallback(() => {
    setInvestigation({ mode: "event", event, treeEvents: treeEvents ?? undefined });
    selectSeq(event.seq, event.runId ?? undefined);
  }, [event, setInvestigation, selectSeq, treeEvents]);

  // Group children by handler_id. Children with no handler_id render directly.
  const { handlerGroups, directChildren } = useMemo(() => {
    const groups = new Map<string, AdminEvent[]>();
    const direct: AdminEvent[] = [];
    for (const child of children) {
      if (child.handlerId) {
        const group = groups.get(child.handlerId) ?? [];
        group.push(child);
        groups.set(child.handlerId, group);
      } else {
        direct.push(child);
      }
    }
    return { handlerGroups: groups, directChildren: direct };
  }, [children]);

  const highlightedHandlerId = flowSelection?.kind === "handler" ? flowSelection.handlerId : null;

  return (
    <div className={depth > 0 ? "pl-6" : ""}>
      <div
        ref={isSelected ? nodeRef : undefined}
        onClick={() => selectSeq(event.seq, event.runId ?? undefined)}
        className={`group/tree w-full text-left px-2 py-1.5 rounded transition-colors cursor-pointer hover:bg-accent/30 ${
          isSelected ? "bg-accent/50 ring-1 ring-blue-500/50" : ""
        }`}
      >
        <div className="flex items-center gap-1.5 min-w-0">
            {hasChildren ? (
              <button
                onClick={(e) => { e.stopPropagation(); setCollapsed((v) => !v); }}
                className="text-[10px] text-muted-foreground hover:text-foreground shrink-0 w-3 text-center"
              >
                {collapsed ? "▸" : "▾"}
              </button>
            ) : (
              <span className="w-3 shrink-0" />
            )}
            <span className={`px-1 py-0.5 rounded text-[10px] font-medium shrink-0 ${layerColor}`}>
              {event.layer}
            </span>
            <span className="text-[10px] font-mono shrink-0" style={{ color: eventTextColor(event.name) }}>
              {event.name}
            </span>
            {collapsed && hasChildren && (
              <span className="text-[10px] text-muted-foreground shrink-0">
                ({children.length})
              </span>
            )}
            <span className="text-[10px] text-muted-foreground shrink-0">
              {formatTs(event.ts)}
            </span>
            <button
              onClick={(e) => {
                e.stopPropagation();
                const json = buildTreeJson([event], childrenMap);
                const text = JSON.stringify(json[0], null, 2);
                copyToClipboard(text);
                setCopied(true);
                setTimeout(() => setCopied(false), 1500);
              }}
              className="opacity-0 group-hover/tree:opacity-100 transition-opacity ml-auto p-0.5 rounded hover:bg-accent shrink-0"
              title="Copy subtree as JSON"
            >
              {copied ? <Check className="w-3 h-3 text-green-400" /> : <Copy className="w-3 h-3 text-muted-foreground" />}
            </button>
            <button
              onClick={(e) => { e.stopPropagation(); handleInvestigate(); }}
              className="opacity-0 group-hover/tree:opacity-100 transition-opacity p-0.5 rounded hover:bg-accent shrink-0"
              title="Investigate with AI"
            >
              <Search className="w-3 h-3 text-muted-foreground" />
            </button>
          </div>
        <button
          onClick={(e) => { e.stopPropagation(); setPayloadOpen((v) => !v); }}
          className="mt-0.5 ml-3 text-[10px] font-mono text-muted-foreground hover:text-foreground truncate text-left max-w-full block"
          title="Click to expand payload"
        >
          {event.summary ?? compactPayload(event.payload)}
        </button>
        {payloadOpen && (
          <CopyablePayload payload={event.payload} className="mt-1 ml-3 max-h-48" />
        )}
      </div>

      {!collapsed && (
        <>
          {/* Direct children (no handler_id) */}
          {directChildren.map((child) => (
            <TreeNode
              key={child.seq}
              event={child}
              childrenMap={childrenMap}
              depth={depth + 1}
            />
          ))}
          {/* Children grouped by handler */}
          {[...handlerGroups.entries()].map(([hid, group]) => (
            <HandlerNode
              key={hid}
              handlerId={hid}
              parentEventId={event.id!}
              children={group}
              childrenMap={childrenMap}
              depth={depth + 1}
              isHighlighted={hid === highlightedHandlerId}
            />
          ))}
        </>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// CausalTreePane
// ---------------------------------------------------------------------------

function matchesFlowSelection(event: AdminEvent, sel: FlowSelection): boolean {
  if (!sel) return true;
  if (sel.kind === "event-type")
    return event.handlerId === sel.handlerId && event.name === sel.name;
  return event.handlerId === sel.handlerId;
}

type TreeJson = {
  name: string;
  layer: string;
  handlerId: string | null;
  summary: string | null;
  children?: TreeJson[];
};

function buildTreeJson(roots: AdminEvent[], childrenMap: Map<string, AdminEvent[]>): TreeJson[] {
  function toNode(evt: AdminEvent): TreeJson {
    const children = evt.id ? (childrenMap.get(evt.id) ?? []) : [];
    const node: TreeJson = {
      name: evt.name,
      layer: evt.layer,
      handlerId: evt.handlerId,
      summary: evt.summary,
    };
    if (children.length > 0) {
      node.children = children.map(toNode);
    }
    return node;
  }
  return roots.map(toNode);
}

export function CausalTreePane() {
  const { treeEvents, treeLoading, flowSelection, setFlowSelection, flowRunId } = useEventsPaneContext();

  const { roots, childrenMap, totalCount, filteredCount } = useMemo(() => {
    if (!treeEvents || treeEvents.length === 0)
      return { roots: [] as AdminEvent[], childrenMap: new Map<string, AdminEvent[]>(), totalCount: 0, filteredCount: 0 };

    const total = treeEvents.length;

    // Only apply flowSelection filter when flow is active
    const events = (flowRunId && flowSelection)
      ? treeEvents.filter(e => matchesFlowSelection(e, flowSelection))
      : treeEvents;

    const idSet = new Set(events.map(e => e.id).filter(Boolean));
    const cMap = new Map<string, AdminEvent[]>();
    const rootList: AdminEvent[] = [];

    for (const evt of events) {
      if (evt.parentId == null || !idSet.has(evt.parentId)) {
        rootList.push(evt);
      } else {
        const siblings = cMap.get(evt.parentId) ?? [];
        siblings.push(evt);
        cMap.set(evt.parentId, siblings);
      }
    }

    rootList.sort((a, b) => a.seq - b.seq);
    const filtered = rootList.length + [...cMap.values()].reduce((s, a) => s + a.length, 0);
    return { roots: rootList, childrenMap: cMap, totalCount: total, filteredCount: filtered };
  }, [treeEvents, flowRunId, flowSelection]);

  if (treeLoading) {
    return (
      <div className="p-3 space-y-1.5 animate-pulse">
        <div className="h-3 w-32 bg-muted rounded mb-3" />
        <div className="flex items-center gap-1.5">
          <div className="h-4 w-12 bg-muted rounded" />
          <div className="h-4 w-36 bg-muted rounded" />
          <div className="h-3 w-24 bg-muted rounded" />
        </div>
        <div className="pl-6 space-y-1.5">
          <div className="flex items-center gap-1.5">
            <div className="h-4 w-14 bg-muted rounded" />
            <div className="h-4 w-44 bg-muted rounded" />
            <div className="h-3 w-24 bg-muted rounded" />
          </div>
          <div className="flex items-center gap-1.5">
            <div className="h-4 w-10 bg-muted rounded" />
            <div className="h-4 w-32 bg-muted rounded" />
            <div className="h-3 w-24 bg-muted rounded" />
          </div>
          <div className="pl-6 space-y-1.5">
            <div className="flex items-center gap-1.5">
              <div className="h-4 w-12 bg-muted rounded" />
              <div className="h-4 w-40 bg-muted rounded" />
              <div className="h-3 w-24 bg-muted rounded" />
            </div>
          </div>
          <div className="flex items-center gap-1.5">
            <div className="h-4 w-14 bg-muted rounded" />
            <div className="h-4 w-28 bg-muted rounded" />
            <div className="h-3 w-24 bg-muted rounded" />
          </div>
        </div>
      </div>
    );
  }

  if (!treeEvents) {
    return (
      <div className="flex items-center justify-center h-full text-sm text-muted-foreground">
        Select an event to view its causal tree
      </div>
    );
  }

  if (roots.length === 0 && flowSelection) {
    return (
      <div className="h-full overflow-y-auto p-3">
        <div className="flex items-center gap-2 mb-2 px-2 py-1 rounded bg-blue-500/10 text-xs text-blue-400">
          <span>
            {flowSelection.kind === "event-type"
              ? `${flowSelection.name} from ${flowSelection.handlerId ?? "root"}`
              : `outputs of ${flowSelection.handlerId}`}
          </span>
          <button
            onClick={() => setFlowSelection(null)}
            className="ml-auto hover:text-foreground"
          >
            ✕
          </button>
        </div>
        <div className="flex items-center justify-center h-32 text-sm text-muted-foreground">
          No events match the current filter
        </div>
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto p-3">
      {flowSelection && (
        <div className="flex items-center gap-2 mb-2 px-2 py-1 rounded bg-blue-500/10 text-xs text-blue-400">
          <span>
            {flowSelection.kind === "event-type"
              ? `${flowSelection.name} from ${flowSelection.handlerId ?? "root"}`
              : `outputs of ${flowSelection.handlerId}`}
          </span>
          <button
            onClick={() => setFlowSelection(null)}
            className="ml-auto hover:text-foreground"
          >
            ✕
          </button>
        </div>
      )}
      <h3 className="text-xs font-semibold text-muted-foreground mb-2 uppercase tracking-wider">
        Causal Tree ({flowSelection ? `${filteredCount} of ${totalCount}` : totalCount} events)
      </h3>
      {roots.map(root => (
        <TreeNode
          key={root.seq}
          event={root}
          childrenMap={childrenMap}
          depth={0}
        />
      ))}
    </div>
  );
}
