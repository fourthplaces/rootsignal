import { useState, useCallback, useEffect, useRef } from "react";
import { Search, GitBranch, X } from "lucide-react";
import { useEventsPaneContext, type AdminEvent } from "../EventsPaneContext";
import { eventTextColor } from "../eventColor";

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

function compactPayload(raw: string, maxLen = 200): string {
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
// FilterBar
// ---------------------------------------------------------------------------

function FilterBar() {
  const { layers, toggleLayer, search, setSearch, runId, setRunId, timeFrom, setTimeFrom, timeTo, setTimeTo } =
    useEventsPaneContext();

  return (
    <div className="flex flex-wrap items-center gap-2 px-3 py-2 border-b border-border bg-card/50">
      {LAYER_OPTIONS.map((layer) => {
        const active = layers.has(layer);
        const color = LAYER_COLORS[layer] ?? "";
        return (
          <button
            key={layer}
            onClick={() => toggleLayer(layer)}
            className={`px-2 py-0.5 rounded text-xs font-medium transition-opacity ${color} ${
              active ? "opacity-100" : "opacity-30"
            }`}
          >
            {layer}
          </button>
        );
      })}

      <span className="w-px h-4 bg-border" />

      <input
        type="text"
        placeholder="search events..."
        value={search}
        onChange={(e) => setSearch(e.target.value)}
        className="px-2 py-1 text-xs rounded bg-background border border-border text-foreground placeholder:text-muted-foreground w-64"
      />

      <span className="w-px h-4 bg-border" />

      <label className="text-xs text-muted-foreground">From</label>
      <input
        type="date"
        value={timeFrom}
        onChange={(e) => setTimeFrom(e.target.value)}
        className="px-2 py-1 text-xs rounded bg-background border border-border text-foreground w-32"
      />
      <label className="text-xs text-muted-foreground">To</label>
      <input
        type="date"
        value={timeTo}
        onChange={(e) => setTimeTo(e.target.value)}
        className="px-2 py-1 text-xs rounded bg-background border border-border text-foreground w-32"
      />

      {runId && (
        <>
          <span className="w-px h-4 bg-border" />
          <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded bg-purple-500/20 text-purple-400 text-[10px] font-mono">
            run: {runId.slice(0, 8)}…
            <button onClick={() => setRunId("")} className="hover:text-foreground">
              <X className="w-3 h-3" />
            </button>
          </span>
        </>
      )}
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
  onInvestigate,
  onFilterRun,
  onViewFlow,
}: {
  event: AdminEvent;
  isSelected: boolean;
  onClick: () => void;
  onInvestigate: () => void;
  onFilterRun: (runId: string) => void;
  onViewFlow: (event: AdminEvent) => void;
}) {
  const [payloadOpen, setPayloadOpen] = useState(false);
  const layerColor = LAYER_COLORS[event.layer] ?? "bg-zinc-500/20 text-zinc-400";

  return (
    <div
      className={`group w-full text-left px-3 py-2 border-b border-border hover:bg-accent/30 transition-colors ${
        isSelected ? "bg-accent/50 ring-1 ring-blue-500/50" : ""
      }`}
    >
      <div onClick={onClick} role="button" tabIndex={0} onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") onClick(); }} className="w-full text-left cursor-pointer">
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
          {event.runId && (
            <button
              onClick={(e) => { e.stopPropagation(); onFilterRun(event.runId!); }}
              className="px-1 py-0.5 rounded text-[10px] font-mono bg-purple-500/10 text-purple-400 hover:bg-purple-500/20 shrink-0 transition-colors"
              title={`Filter by run ${event.runId}`}
            >
              {event.runId.slice(0, 8)}
            </button>
          )}
          <span className="text-xs font-mono shrink-0" style={{ color: eventTextColor(event.name) }}>
            {event.name}
          </span>
          <button
            onClick={(e) => { e.stopPropagation(); setPayloadOpen((v) => !v); }}
            className="text-[10px] font-mono text-muted-foreground hover:text-foreground truncate text-left"
            title="Click to expand payload"
          >
            {event.summary ?? compactPayload(event.payload)}
          </button>
          {event.runId && (
            <button
              onClick={(e) => { e.stopPropagation(); onViewFlow(event); }}
              className="opacity-0 group-hover:opacity-100 transition-opacity ml-auto p-1 rounded hover:bg-accent shrink-0"
              title="View causal flow"
            >
              <GitBranch className="w-3.5 h-3.5 text-muted-foreground" />
            </button>
          )}
          <button
            onClick={(e) => { e.stopPropagation(); onInvestigate(); }}
            className={`opacity-0 group-hover:opacity-100 transition-opacity ${event.runId ? "" : "ml-auto"} p-1 rounded hover:bg-accent shrink-0`}
            title="Investigate with AI"
          >
            <Search className="w-3.5 h-3.5 text-muted-foreground" />
          </button>
        </div>
      </div>
      {payloadOpen && (
        <pre className="mt-1 ml-14 p-2 text-[10px] bg-background rounded border border-border overflow-x-auto max-h-64 whitespace-pre-wrap">
          {formatPayload(event.payload)}
        </pre>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// InfiniteScrollSentinel
// ---------------------------------------------------------------------------

function InfiniteScrollSentinel({ onVisible, loading }: { onVisible: () => void; loading: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  const onVisibleRef = useRef(onVisible);
  onVisibleRef.current = onVisible;

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      ([entry]) => { if (entry.isIntersecting) onVisibleRef.current(); },
      { rootMargin: "200px" },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  return (
    <div ref={ref} className="flex items-center justify-center py-3">
      {loading && <span className="text-[10px] text-muted-foreground">Loading...</span>}
    </div>
  );
}

// ---------------------------------------------------------------------------
// TimelinePane
// ---------------------------------------------------------------------------

export function TimelinePane() {
  const {
    filteredEvents,
    loading,
    hasMore,
    loadMore,
    loadingMore,
    live,
    selectedSeq,
    selectSeq,
    setRunId,
    setInvestigateEvent,
    openFlow,
  } = useEventsPaneContext();

  const handleInvestigate = useCallback(
    (event: AdminEvent) => {
      setInvestigateEvent(event);
      selectSeq(event.seq, event.runId ?? undefined);
    },
    [setInvestigateEvent, selectSeq],
  );

  return (
    <div className="flex flex-col h-full">
      <FilterBar />
      {live && (
        <div className="flex items-center gap-1.5 px-3 py-1 border-b border-border text-[10px] text-emerald-400">
          <span className="relative flex h-2 w-2">
            <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75" />
            <span className="relative inline-flex rounded-full h-2 w-2 bg-emerald-500" />
          </span>
          Live
        </div>
      )}
      {loading && filteredEvents.length === 0 ? (
        <div className="animate-pulse">
          {Array.from({ length: 12 }).map((_, i) => (
            <div key={i} className="flex items-center gap-2 px-3 py-2 border-b border-border">
              <div className="h-3 w-12 bg-muted rounded shrink-0" />
              <div className="h-3 w-32 bg-muted rounded shrink-0" />
              <div className="h-4 w-14 bg-muted rounded shrink-0" />
              <div className="h-4 w-16 bg-muted rounded shrink-0" />
              <div className="h-3 w-28 bg-muted rounded shrink-0" />
              <div className="h-3 bg-muted rounded flex-1" style={{ maxWidth: `${150 + (i * 37) % 200}px` }} />
            </div>
          ))}
        </div>
      ) : filteredEvents.length === 0 ? (
        <div className="flex items-center justify-center h-32 text-sm text-muted-foreground">
          No events found
        </div>
      ) : (
        <div className="flex-1 overflow-y-auto">
          {filteredEvents.map((event) => (
            <EventRow
              key={event.seq}
              event={event}
              isSelected={event.seq === selectedSeq}
              onClick={() => selectSeq(event.seq, event.runId ?? undefined)}
              onInvestigate={() => handleInvestigate(event)}
              onFilterRun={setRunId}
              onViewFlow={(evt) => { selectSeq(evt.seq, evt.runId ?? undefined); if (evt.runId) openFlow(evt.runId, { kind: "event-type", handlerId: evt.handlerId, name: evt.name }); }}
            />
          ))}
          {hasMore && (
            <InfiniteScrollSentinel onVisible={loadMore} loading={loadingMore} />
          )}
        </div>
      )}
    </div>
  );
}
