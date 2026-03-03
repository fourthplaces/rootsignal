import { useState, useMemo, useRef, useEffect, useCallback } from "react";
import { Search } from "lucide-react";
import { useEventsPaneContext, type AdminEvent } from "../EventsPaneContext";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

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

function formatPayload(raw: string): string {
  try {
    return JSON.stringify(JSON.parse(raw), null, 2);
  } catch {
    return raw;
  }
}

// ---------------------------------------------------------------------------
// TreeNode (recursive)
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
  const { selectedSeq, selectSeq, setInvestigateEvent } = useEventsPaneContext();
  const [payloadOpen, setPayloadOpen] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
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
    setInvestigateEvent(event);
    selectSeq(event.seq);
  }, [event, setInvestigateEvent, selectSeq]);

  return (
    <div className={depth > 0 ? "pl-6" : ""}>
      <div
        ref={isSelected ? nodeRef : undefined}
        className={`group/tree w-full text-left px-2 py-1.5 rounded transition-colors hover:bg-accent/30 ${
          isSelected ? "bg-accent/50 ring-1 ring-blue-500/50" : ""
        }`}
      >
        <button onClick={() => selectSeq(event.seq)} className="w-full text-left">
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
            <span className="text-[10px] font-mono text-foreground shrink-0">
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
              onClick={(e) => { e.stopPropagation(); handleInvestigate(); }}
              className="opacity-0 group-hover/tree:opacity-100 transition-opacity ml-auto p-0.5 rounded hover:bg-accent shrink-0"
              title="Investigate with AI"
            >
              <Search className="w-3 h-3 text-muted-foreground" />
            </button>
          </div>
        </button>
        <button
          onClick={(e) => { e.stopPropagation(); setPayloadOpen((v) => !v); }}
          className="mt-0.5 ml-3 text-[10px] font-mono text-muted-foreground hover:text-foreground truncate text-left max-w-full block"
          title="Click to expand payload"
        >
          {event.summary ?? compactPayload(event.payload)}
        </button>
        {payloadOpen && (
          <pre className="mt-1 ml-3 p-2 text-[10px] bg-background rounded border border-border overflow-x-auto max-h-48 whitespace-pre-wrap">
            {formatPayload(event.payload)}
          </pre>
        )}
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
// CausalTreePane
// ---------------------------------------------------------------------------

export function CausalTreePane() {
  const { treeData, treeLoading } = useEventsPaneContext();

  const { root, childrenMap } = useMemo(() => {
    if (!treeData) return { root: null, childrenMap: new Map<string, AdminEvent[]>() };

    const bySeq = new Map<number, AdminEvent>();
    const cMap = new Map<string, AdminEvent[]>();

    for (const evt of treeData.events) {
      bySeq.set(evt.seq, evt);
      if (evt.parentId != null) {
        const siblings = cMap.get(evt.parentId) ?? [];
        siblings.push(evt);
        cMap.set(evt.parentId, siblings);
      }
    }

    return { root: bySeq.get(treeData.rootSeq) ?? null, childrenMap: cMap };
  }, [treeData]);

  if (treeLoading) {
    return (
      <div className="flex items-center justify-center h-32 text-sm text-muted-foreground">
        Loading causal tree...
      </div>
    );
  }

  if (!treeData || !root) {
    return (
      <div className="flex items-center justify-center h-full text-sm text-muted-foreground">
        Select an event to view its causal tree
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto p-3">
      <h3 className="text-xs font-semibold text-muted-foreground mb-2 uppercase tracking-wider">
        Causal Tree ({treeData.events.length} events)
      </h3>
      <TreeNode
        event={root}
        childrenMap={childrenMap}
        depth={0}
      />
    </div>
  );
}
