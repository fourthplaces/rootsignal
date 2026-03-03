import { useState, useMemo, useCallback, useEffect, useRef } from "react";
import { useSearchParams } from "react-router";
import { useQuery, useLazyQuery } from "@apollo/client";
import {
  Panel,
  Group as PanelGroup,
  Separator as PanelResizeHandle,
} from "react-resizable-panels";
import { ADMIN_EVENTS, ADMIN_CAUSAL_TREE } from "@/graphql/queries";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

type AdminEvent = {
  seq: number;
  ts: string;
  type: string;       // codec name (e.g. "DiscoveryEvent")
  name: string;       // variant name (e.g. "source_discovered")
  layer: string;
  id: string | null;        // this event's UUID
  parentId: string | null;  // parent event's UUID (for tree structure)
  correlationId: string | null;
  runId: string | null;
  summary: string | null;
  payload: string;
};

type AdminEventsPage = {
  events: AdminEvent[];
  nextCursor: number | null;
};

type CausalTreeResult = {
  events: AdminEvent[];
  rootSeq: number;
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const LAYER_COLORS: Record<string, string> = {
  world: "bg-blue-500/20 text-blue-400",
  system: "bg-amber-500/20 text-amber-400",
  telemetry: "bg-zinc-500/20 text-zinc-400",
};

const LAYER_OPTIONS = ["world", "system", "telemetry"] as const;

function formatTs(ts: string): string {
  return new Date(ts).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/** Compact one-line payload preview, excluding the `type` key (already shown as name). */
function compactPayload(raw: string, maxLen = 200): string {
  try {
    const obj = JSON.parse(raw);
    if (typeof obj !== "object" || obj === null) return raw.slice(0, maxLen);
    const entries = Object.entries(obj).filter(([k]) => k !== "type");
    if (entries.length === 0) return "{}";
    const parts: string[] = [];
    let len = 2; // opening/closing braces
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
// FilterBar
// ---------------------------------------------------------------------------

function FilterBar({
  layers,
  onToggleLayer,
  eventTypeFilter,
  onEventTypeFilterChange,
  correlationId,
  onCorrelationIdChange,
  runId,
  onRunIdChange,
  payloadSearch,
  onPayloadSearchChange,
  timeFrom,
  onTimeFromChange,
  timeTo,
  onTimeToChange,
}: {
  layers: Set<string>;
  onToggleLayer: (layer: string) => void;
  eventTypeFilter: string;
  onEventTypeFilterChange: (v: string) => void;
  correlationId: string;
  onCorrelationIdChange: (v: string) => void;
  runId: string;
  onRunIdChange: (v: string) => void;
  payloadSearch: string;
  onPayloadSearchChange: (v: string) => void;
  timeFrom: string;
  onTimeFromChange: (v: string) => void;
  timeTo: string;
  onTimeToChange: (v: string) => void;
}) {
  return (
    <div className="flex flex-wrap items-center gap-2 px-3 py-2 border-b border-border bg-card/50">
      {/* Layer toggles */}
      {LAYER_OPTIONS.map((layer) => {
        const active = layers.has(layer);
        const color = LAYER_COLORS[layer] ?? "";
        return (
          <button
            key={layer}
            onClick={() => onToggleLayer(layer)}
            className={`px-2 py-0.5 rounded text-xs font-medium transition-opacity ${color} ${
              active ? "opacity-100" : "opacity-30"
            }`}
          >
            {layer}
          </button>
        );
      })}

      <span className="w-px h-4 bg-border" />

      {/* Payload search */}
      <input
        type="text"
        placeholder="search payload..."
        value={payloadSearch}
        onChange={(e) => onPayloadSearchChange(e.target.value)}
        className="px-2 py-1 text-xs rounded bg-background border border-border text-foreground placeholder:text-muted-foreground w-48"
      />

      {/* Event type filter */}
      <input
        type="text"
        placeholder="event_type"
        value={eventTypeFilter}
        onChange={(e) => onEventTypeFilterChange(e.target.value)}
        className="px-2 py-1 text-xs rounded bg-background border border-border text-foreground placeholder:text-muted-foreground w-36"
      />

      {/* Correlation ID */}
      <input
        type="text"
        placeholder="correlation_id"
        value={correlationId}
        onChange={(e) => onCorrelationIdChange(e.target.value)}
        className="px-2 py-1 text-xs rounded bg-background border border-border text-foreground placeholder:text-muted-foreground w-48"
      />

      {/* Run ID */}
      <input
        type="text"
        placeholder="run_id"
        value={runId}
        onChange={(e) => onRunIdChange(e.target.value)}
        className="px-2 py-1 text-xs rounded bg-background border border-border text-foreground placeholder:text-muted-foreground w-36"
      />

      <span className="w-px h-4 bg-border" />

      {/* Time range */}
      <label className="text-xs text-muted-foreground">From</label>
      <input
        type="date"
        value={timeFrom}
        onChange={(e) => onTimeFromChange(e.target.value)}
        className="px-2 py-1 text-xs rounded bg-background border border-border text-foreground w-32"
      />
      <label className="text-xs text-muted-foreground">To</label>
      <input
        type="date"
        value={timeTo}
        onChange={(e) => onTimeToChange(e.target.value)}
        className="px-2 py-1 text-xs rounded bg-background border border-border text-foreground w-32"
      />
    </div>
  );
}

// ---------------------------------------------------------------------------
// EventRow
// ---------------------------------------------------------------------------

function EventRow({
  event,
  isSelected,
  onClick,
}: {
  event: AdminEvent;
  isSelected: boolean;
  onClick: () => void;
}) {
  const [payloadOpen, setPayloadOpen] = useState(false);
  const layerColor = LAYER_COLORS[event.layer] ?? "bg-zinc-500/20 text-zinc-400";

  return (
    <div
      className={`w-full text-left px-3 py-2 border-b border-border hover:bg-accent/30 transition-colors ${
        isSelected ? "bg-accent/50 ring-1 ring-blue-500/50" : ""
      }`}
    >
      <button onClick={onClick} className="w-full text-left">
        <div className="flex items-center gap-2 min-w-0">
          <span className="text-[10px] font-mono text-muted-foreground w-12 shrink-0 text-right">
            {event.seq}
          </span>
          <span className="text-[10px] text-muted-foreground shrink-0 w-32">
            {formatTs(event.ts)}
          </span>
          <span className={`px-1.5 py-0.5 rounded text-[10px] font-medium shrink-0 ${layerColor}`}>
            {event.layer}
          </span>
          <span className="text-xs font-mono text-foreground shrink-0">
            {event.name}
          </span>
          <button
            onClick={(e) => { e.stopPropagation(); setPayloadOpen((v) => !v); }}
            className="text-[10px] font-mono text-muted-foreground hover:text-foreground truncate text-left"
            title="Click to expand payload"
          >
            {compactPayload(event.payload)}
          </button>
        </div>
      </button>
      {payloadOpen && (
        <pre className="mt-1 ml-14 p-2 text-[10px] bg-background rounded border border-border overflow-x-auto max-h-64 whitespace-pre-wrap">
          {formatPayload(event.payload)}
        </pre>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// EventTimeline
// ---------------------------------------------------------------------------

function EventTimeline({
  events,
  selectedSeq,
  onSelectSeq,
  loading,
  hasMore,
  onLoadMore,
  loadingMore,
  timelineRef,
}: {
  events: AdminEvent[];
  selectedSeq: number | null;
  onSelectSeq: (seq: number) => void;
  loading: boolean;
  hasMore: boolean;
  onLoadMore: () => void;
  loadingMore: boolean;
  timelineRef: React.RefObject<HTMLDivElement | null>;
}) {
  if (loading && events.length === 0) {
    return (
      <div className="flex items-center justify-center h-32 text-sm text-muted-foreground">
        Loading events...
      </div>
    );
  }

  if (events.length === 0) {
    return (
      <div className="flex items-center justify-center h-32 text-sm text-muted-foreground">
        No events found
      </div>
    );
  }

  return (
    <div ref={timelineRef} className="flex-1 overflow-y-auto">
      {events.map((event) => (
        <EventRow
          key={event.seq}
          event={event}
          isSelected={event.seq === selectedSeq}
          onClick={() => onSelectSeq(event.seq)}
        />
      ))}
      {hasMore && (
        <div className="p-3 text-center">
          <button
            onClick={onLoadMore}
            disabled={loadingMore}
            className="px-4 py-1.5 text-xs rounded bg-accent/50 text-foreground hover:bg-accent transition-colors disabled:opacity-50"
          >
            {loadingMore ? "Loading..." : "Load more"}
          </button>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// TreeNode (recursive)
// ---------------------------------------------------------------------------

function TreeNode({
  event,
  childrenMap,
  selectedSeq,
  onSelectSeq,
  depth,
}: {
  event: AdminEvent;
  childrenMap: Map<string, AdminEvent[]>;
  selectedSeq: number | null;
  onSelectSeq: (seq: number) => void;
  depth: number;
}) {
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

  return (
    <div className={depth > 0 ? "pl-6" : ""}>
      <div
        ref={isSelected ? nodeRef : undefined}
        className={`w-full text-left px-2 py-1.5 rounded transition-colors hover:bg-accent/30 ${
          isSelected ? "bg-accent/50 ring-1 ring-blue-500/50" : ""
        }`}
      >
        <button onClick={() => onSelectSeq(event.seq)} className="w-full text-left">
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
          </div>
        </button>
        <button
          onClick={(e) => { e.stopPropagation(); setPayloadOpen((v) => !v); }}
          className="mt-0.5 ml-3 text-[10px] font-mono text-muted-foreground hover:text-foreground truncate text-left max-w-full block"
          title="Click to expand payload"
        >
          {compactPayload(event.payload, 80)}
        </button>
        {payloadOpen && (
          <pre className="mt-1 ml-3 p-2 text-[10px] bg-background rounded border border-border overflow-x-auto max-h-48 whitespace-pre-wrap">
            {formatPayload(event.payload)}
          </pre>
        )}
      </div>

      {/* Children */}
      {!collapsed && children.map((child) => (
        <TreeNode
          key={child.seq}
          event={child}
          childrenMap={childrenMap}
          selectedSeq={selectedSeq}
          onSelectSeq={onSelectSeq}
          depth={depth + 1}
        />
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// CausalTreePanel
// ---------------------------------------------------------------------------

function CausalTreePanel({
  treeData,
  loading,
  selectedSeq,
  onSelectSeq,
}: {
  treeData: CausalTreeResult | null;
  loading: boolean;
  selectedSeq: number | null;
  onSelectSeq: (seq: number) => void;
}) {
  // Build tree from parentId (UUID) → children mapping.
  // parentId points to the parent event's UUID (the "id" field in the events table / payload).
  const { root, childrenMap } = useMemo(() => {
    if (!treeData) return { root: null, childrenMap: new Map<string, AdminEvent[]>() };

    const bySeq = new Map<number, AdminEvent>();
    // Map from parent UUID → children
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

  if (loading) {
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
        selectedSeq={selectedSeq}
        onSelectSeq={onSelectSeq}
        depth={0}
      />
    </div>
  );
}

// ---------------------------------------------------------------------------
// EventsPage
// ---------------------------------------------------------------------------

export function EventsPage() {
  const [searchParams, setSearchParams] = useSearchParams();

  // Filter state from URL
  const [layers, setLayers] = useState<Set<string>>(
    () => new Set(searchParams.get("layers")?.split(",").filter(Boolean) ?? LAYER_OPTIONS),
  );
  const [eventTypeFilter, setEventTypeFilter] = useState(searchParams.get("type") ?? "");
  const [correlationId, setCorrelationId] = useState(searchParams.get("corr") ?? "");
  const [runId, setRunId] = useState(searchParams.get("run") ?? "");
  const [timeFrom, setTimeFrom] = useState(searchParams.get("from") ?? "");
  const [timeTo, setTimeTo] = useState(searchParams.get("to") ?? "");
  const [payloadSearch, setPayloadSearch] = useState(searchParams.get("q") ?? "");
  const [debouncedPayloadSearch, setDebouncedPayloadSearch] = useState(payloadSearch);
  const [selectedSeq, setSelectedSeq] = useState<number | null>(
    searchParams.get("seq") ? Number(searchParams.get("seq")) : null,
  );

  // Debounce payload search input (300ms)
  useEffect(() => {
    const timer = setTimeout(() => setDebouncedPayloadSearch(payloadSearch), 300);
    return () => clearTimeout(timer);
  }, [payloadSearch]);

  // Accumulated events for infinite scroll
  const [allEvents, setAllEvents] = useState<AdminEvent[]>([]);
  const [cursor, setCursor] = useState<number | null>(null);
  const timelineRef = useRef<HTMLDivElement>(null);

  // Sync URL params
  const lastParamsRef = useRef("");
  useEffect(() => {
    const params: Record<string, string> = {};
    const layersStr = [...layers].sort().join(",");
    if (layersStr !== "system,telemetry,world") params.layers = layersStr;
    if (eventTypeFilter) params.type = eventTypeFilter;
    if (correlationId) params.corr = correlationId;
    if (runId) params.run = runId;
    if (payloadSearch) params.q = payloadSearch;
    if (timeFrom) params.from = timeFrom;
    if (timeTo) params.to = timeTo;
    if (selectedSeq != null) params.seq = String(selectedSeq);
    const serialized = JSON.stringify(params);
    if (serialized !== lastParamsRef.current) {
      lastParamsRef.current = serialized;
      setSearchParams(params, { replace: true });
    }
  }, [layers, eventTypeFilter, correlationId, runId, payloadSearch, timeFrom, timeTo, selectedSeq, setSearchParams]);

  // Build event_types filter from text input (matches against codec name in event_type column)
  const eventTypes = useMemo(() => {
    if (eventTypeFilter.trim()) {
      return eventTypeFilter.split(",").map((s) => s.trim()).filter(Boolean);
    }
    return undefined;
  }, [eventTypeFilter]);

  const queryVars = useMemo(
    () => ({
      limit: 50,
      cursor: cursor ?? undefined,
      eventTypes,
      runId: runId || undefined,
      correlationId: correlationId || undefined,
      from: timeFrom ? new Date(timeFrom).toISOString() : undefined,
      to: timeTo ? new Date(timeTo + "T23:59:59").toISOString() : undefined,
      payloadSearch: debouncedPayloadSearch || undefined,
    }),
    [cursor, eventTypes, runId, correlationId, timeFrom, timeTo, debouncedPayloadSearch],
  );

  const { data, loading } = useQuery<{ adminEvents: AdminEventsPage }>(ADMIN_EVENTS, {
    variables: queryVars,
    fetchPolicy: "network-only",
  });

  // When filters change (but not cursor), reset accumulated events
  const filterKey = useMemo(
    () => JSON.stringify({ eventTypes, runId, correlationId, timeFrom, timeTo, debouncedPayloadSearch }),
    [eventTypes, runId, correlationId, timeFrom, timeTo, debouncedPayloadSearch],
  );
  const prevFilterKeyRef = useRef(filterKey);
  useEffect(() => {
    if (filterKey !== prevFilterKeyRef.current) {
      prevFilterKeyRef.current = filterKey;
      setAllEvents([]);
      setCursor(null);
    }
  }, [filterKey]);

  // Append new data to accumulated events
  useEffect(() => {
    if (data?.adminEvents?.events) {
      const newEvents = data.adminEvents.events;
      if (cursor == null) {
        // Fresh load
        setAllEvents(newEvents);
      } else {
        // Append (deduplicate by seq)
        setAllEvents((prev) => {
          const existing = new Set(prev.map((e) => e.seq));
          const deduped = newEvents.filter((e) => !existing.has(e.seq));
          return [...prev, ...deduped];
        });
      }
    }
  }, [data, cursor]);

  // Filter by active layers client-side
  const filteredEvents = useMemo(
    () => allEvents.filter((e) => layers.has(e.layer)),
    [allEvents, layers],
  );

  const hasMore = data?.adminEvents?.nextCursor != null;

  const handleLoadMore = useCallback(() => {
    if (data?.adminEvents?.nextCursor != null) {
      setCursor(data.adminEvents.nextCursor);
    }
  }, [data]);

  // Causal tree query
  const [fetchTree, { data: treeData, loading: treeLoading }] = useLazyQuery<{
    adminCausalTree: CausalTreeResult;
  }>(ADMIN_CAUSAL_TREE);

  const handleSelectSeq = useCallback(
    (seq: number) => {
      setSelectedSeq(seq);
      fetchTree({ variables: { seq } });
    },
    [fetchTree],
  );

  const toggleLayer = useCallback((layer: string) => {
    setLayers((prev) => {
      const next = new Set(prev);
      if (next.has(layer)) next.delete(layer);
      else next.add(layer);
      return next;
    });
  }, []);

  return (
    <div className="h-[calc(100vh-3rem)] -m-6">
      <PanelGroup orientation="horizontal" className="h-full">
        {/* Left: Timeline */}
        <Panel defaultSize={60} minSize={30}>
          <div className="flex flex-col h-full">
            <FilterBar
              layers={layers}
              onToggleLayer={toggleLayer}
              eventTypeFilter={eventTypeFilter}
              onEventTypeFilterChange={setEventTypeFilter}
              correlationId={correlationId}
              onCorrelationIdChange={setCorrelationId}
              runId={runId}
              onRunIdChange={setRunId}
              payloadSearch={payloadSearch}
              onPayloadSearchChange={setPayloadSearch}
              timeFrom={timeFrom}
              onTimeFromChange={setTimeFrom}
              timeTo={timeTo}
              onTimeToChange={setTimeTo}
            />
            <EventTimeline
              events={filteredEvents}
              selectedSeq={selectedSeq}
              onSelectSeq={handleSelectSeq}
              loading={loading}
              hasMore={hasMore}
              onLoadMore={handleLoadMore}
              loadingMore={loading && allEvents.length > 0}
              timelineRef={timelineRef}
            />
          </div>
        </Panel>

        <PanelResizeHandle className="w-1.5 bg-border hover:bg-accent transition-colors cursor-col-resize" />

        {/* Right: Causal Tree */}
        <Panel defaultSize={40} minSize={20}>
          <CausalTreePanel
            treeData={treeData?.adminCausalTree ?? null}
            loading={treeLoading}
            selectedSeq={selectedSeq}
            onSelectSeq={handleSelectSeq}
          />
        </Panel>
      </PanelGroup>
    </div>
  );
}
