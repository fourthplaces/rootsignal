import { useState, useCallback, useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { Search, X, Copy, Check, Maximize2 } from "lucide-react";
import { useEventsPaneContext, type AdminEvent } from "../EventsPaneContext";
import { eventTextColor, eventBg } from "../eventColor";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const LAYER_COLORS: Record<string, { bg: string; text: string; dot: string; border: string }> = {
  world: { bg: "bg-blue-500/15", text: "text-blue-400", dot: "bg-blue-400", border: "border-l-blue-500/60" },
  system: { bg: "bg-amber-500/15", text: "text-amber-400", dot: "bg-amber-400", border: "border-l-amber-500/60" },
  telemetry: { bg: "bg-zinc-500/15", text: "text-zinc-400", dot: "bg-zinc-400", border: "border-l-zinc-500/40" },
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
// JsonSyntax — colorized JSON renderer (no dependencies)
// ---------------------------------------------------------------------------

function JsonSyntax({ json }: { json: string }) {
  const tokens = tokenizeJson(json);
  return (
    <>
      {tokens.map((t, i) => (
        <span key={i} className={t.color}>{t.text}</span>
      ))}
    </>
  );
}

type JsonToken = { text: string; color: string };

function tokenizeJson(json: string): JsonToken[] {
  const tokens: JsonToken[] = [];
  const re = /("(?:[^"\\]|\\.)*")\s*:|("(?:[^"\\]|\\.)*")|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|(\btrue\b|\bfalse\b)|(\bnull\b)|([{}[\]:,])/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = re.exec(json)) !== null) {
    if (match.index > lastIndex) {
      tokens.push({ text: json.slice(lastIndex, match.index), color: "" });
    }
    if (match[1] !== undefined) {
      tokens.push({ text: match[1], color: "text-blue-400" });
      tokens.push({ text: ":", color: "text-zinc-500" });
    } else if (match[2] !== undefined) {
      tokens.push({ text: match[2], color: "text-green-400" });
    } else if (match[3] !== undefined) {
      tokens.push({ text: match[3], color: "text-amber-400" });
    } else if (match[4] !== undefined) {
      tokens.push({ text: match[4], color: "text-purple-400" });
    } else if (match[5] !== undefined) {
      tokens.push({ text: match[5], color: "text-zinc-500" });
    } else if (match[6] !== undefined) {
      tokens.push({ text: match[6], color: "text-zinc-500" });
    }
    lastIndex = re.lastIndex;
  }
  if (lastIndex < json.length) {
    tokens.push({ text: json.slice(lastIndex), color: "" });
  }
  return tokens;
}

// ---------------------------------------------------------------------------
// PayloadModal — full-screen overlay for reading large payloads
// ---------------------------------------------------------------------------

function PayloadModal({ formatted, onClose }: { formatted: string; onClose: () => void }) {
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [onClose]);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(formatted);
    } catch {
      const ta = document.createElement("textarea");
      ta.value = formatted;
      ta.style.position = "fixed";
      ta.style.opacity = "0";
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      document.body.removeChild(ta);
    }
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70" onClick={onClose}>
      <div
        className="relative w-[90vw] max-h-[90vh] overflow-auto rounded-lg border border-border bg-background p-4"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="absolute top-2 right-2 flex gap-1">
          <button
            onClick={handleCopy}
            className="p-1 rounded hover:bg-accent transition-colors"
            title="Copy payload"
          >
            {copied ? <Check className="w-4 h-4 text-green-400" /> : <Copy className="w-4 h-4 text-muted-foreground" />}
          </button>
          <button
            onClick={onClose}
            className="p-1 rounded hover:bg-accent transition-colors"
            title="Close"
          >
            <X className="w-4 h-4 text-muted-foreground" />
          </button>
        </div>
        <pre className="text-xs whitespace-pre-wrap">
          <JsonSyntax json={formatted} />
        </pre>
      </div>
    </div>,
    document.body,
  );
}

// ---------------------------------------------------------------------------
// CopyablePayload — shared expandable payload block with copy button
// ---------------------------------------------------------------------------

export function CopyablePayload({ payload, className = "" }: { payload: string; className?: string }) {
  const [copied, setCopied] = useState(false);
  const [modalOpen, setModalOpen] = useState(false);
  const formatted = formatPayload(payload);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(formatted);
    } catch {
      const ta = document.createElement("textarea");
      ta.value = formatted;
      ta.style.position = "fixed";
      ta.style.opacity = "0";
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      document.body.removeChild(ta);
    }
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className={`relative ${className}`}>
      <pre className="p-2 text-[10px] bg-background rounded border border-border overflow-auto whitespace-pre-wrap resize-y min-h-24 max-h-[80vh]">
        <JsonSyntax json={formatted} />
      </pre>
      <div className="absolute top-1.5 right-1.5 z-10 flex gap-1">
        <button
          onClick={(e) => { e.stopPropagation(); setModalOpen(true); }}
          className="p-1 rounded bg-background/80 border border-border hover:bg-accent transition-colors"
          title="Expand payload"
        >
          <Maximize2 className="w-3 h-3 text-muted-foreground" />
        </button>
        <button
          onClick={(e) => { e.stopPropagation(); handleCopy(); }}
          className="p-1 rounded bg-background/80 border border-border hover:bg-accent transition-colors"
          title="Copy payload"
        >
          {copied ? <Check className="w-3 h-3 text-green-400" /> : <Copy className="w-3 h-3 text-muted-foreground" />}
        </button>
      </div>
      {modalOpen && <PayloadModal formatted={formatted} onClose={() => setModalOpen(false)} />}
    </div>
  );
}

// ---------------------------------------------------------------------------
// FilterBar
// ---------------------------------------------------------------------------

function FilterBar() {
  const { layers, toggleLayer, search, setSearch, runId, setRunId } =
    useEventsPaneContext();

  return (
    <div className="flex flex-wrap items-center gap-1.5 px-3 py-2 border-b border-border bg-card/60 backdrop-blur-sm">
      <div className="flex items-center rounded-md border border-border/60 overflow-hidden">
        {LAYER_OPTIONS.map((layer) => {
          const active = layers.has(layer);
          const colors = LAYER_COLORS[layer];
          return (
            <button
              key={layer}
              onClick={() => toggleLayer(layer)}
              className={`flex items-center gap-1.5 px-2.5 py-1 text-[11px] font-medium transition-all border-r border-border/40 last:border-r-0 ${
                active ? `${colors.bg} ${colors.text}` : "text-muted-foreground/40 hover:text-muted-foreground/70"
              }`}
            >
              <span className={`w-1.5 h-1.5 rounded-full transition-all ${active ? colors.dot : "bg-muted-foreground/20"}`} />
              {layer}
            </button>
          );
        })}
      </div>

      <div className="relative flex-1 min-w-48 max-w-80">
        <Search className="absolute left-2 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-muted-foreground/50" />
        <input
          type="text"
          placeholder="Search events..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="w-full pl-7 pr-2 py-1 text-xs rounded-md bg-background/80 border border-border/60 text-foreground placeholder:text-muted-foreground/50 focus:outline-none focus:ring-1 focus:ring-ring/40 focus:border-ring/40 transition-colors"
        />
      </div>

      {runId && (
        <span className="inline-flex items-center gap-1.5 pl-2.5 pr-1.5 py-1 rounded-md bg-purple-500/15 border border-purple-500/20 text-purple-300 text-[11px] font-mono transition-colors">
          <span className="w-1.5 h-1.5 rounded-full bg-purple-400 animate-pulse" />
          {runId.slice(0, 8)}
          <button
            onClick={() => setRunId("")}
            className="ml-0.5 p-0.5 rounded hover:bg-purple-500/20 transition-colors"
          >
            <X className="w-3 h-3" />
          </button>
        </span>
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
}: {
  event: AdminEvent;
  isSelected: boolean;
  onClick: () => void;
  onInvestigate: () => void;
  onFilterRun: (runId: string) => void;
}) {
  const [payloadOpen, setPayloadOpen] = useState(false);
  const colors = LAYER_COLORS[event.layer] ?? LAYER_COLORS.telemetry;

  return (
    <div
      className={`group w-full text-left border-l-2 border-b border-b-border/50 hover:bg-accent/20 transition-all ${
        isSelected
          ? `bg-accent/40 border-l-blue-400 ${colors.border.replace("border-l-", "")}`
          : colors.border
      }`}
    >
      <div onClick={onClick} role="button" tabIndex={0} onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") onClick(); }} className="w-full text-left cursor-pointer px-3 py-1.5">
        <div className="flex items-center gap-2 min-w-0">
          <span className="text-[10px] font-mono text-muted-foreground/60 w-10 shrink-0 text-right tabular-nums">
            {event.seq}
          </span>
          <span className="text-[10px] font-mono text-muted-foreground/50 shrink-0 w-28 tabular-nums">
            {formatTs(event.ts)}
          </span>
          {event.runId && (
            <button
              onClick={(e) => { e.stopPropagation(); onFilterRun(event.runId!); }}
              className="px-1.5 py-0.5 rounded text-[10px] font-mono text-purple-400/70 hover:text-purple-300 hover:bg-purple-500/15 shrink-0 transition-colors"
              title={`Filter by run ${event.runId}`}
            >
              {event.runId.slice(0, 8)}
            </button>
          )}
          <span
            className="px-1.5 py-0.5 rounded text-[11px] font-mono font-medium shrink-0"
            style={{ backgroundColor: eventBg(event.name), color: eventTextColor(event.name) }}
          >
            {event.name}
          </span>
          <button
            onClick={(e) => { e.stopPropagation(); setPayloadOpen((v) => !v); }}
            className="text-[10px] font-mono text-muted-foreground/40 hover:text-muted-foreground truncate text-left transition-colors"
            title="Click to expand payload"
          >
            {event.summary ?? compactPayload(event.payload)}
          </button>
          <button
            onClick={(e) => { e.stopPropagation(); onInvestigate(); }}
            className="opacity-0 group-hover:opacity-100 transition-opacity ml-auto p-1 rounded hover:bg-accent shrink-0"
            title="Investigate with AI"
          >
            <Search className="w-3.5 h-3.5 text-muted-foreground" />
          </button>
        </div>
      </div>
      {payloadOpen && (
        <CopyablePayload payload={event.payload} className="mt-1 mx-3 mb-2 ml-14 max-h-64" />
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
    <div ref={ref} className="flex items-center justify-center py-4">
      {loading && (
        <div className="flex items-center gap-2">
          <div className="w-1 h-1 rounded-full bg-muted-foreground/40 animate-pulse" />
          <div className="w-1 h-1 rounded-full bg-muted-foreground/40 animate-pulse [animation-delay:150ms]" />
          <div className="w-1 h-1 rounded-full bg-muted-foreground/40 animate-pulse [animation-delay:300ms]" />
        </div>
      )}
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
    selectedSeq,
    selectSeq,
    setRunId,
    setInvestigation,
    treeEvents,
    openFlow,
  } = useEventsPaneContext();

  const handleInvestigate = useCallback(
    (event: AdminEvent) => {
      setInvestigation({ mode: "event", event, treeEvents: treeEvents ?? undefined });
      selectSeq(event.seq, event.runId ?? undefined);
    },
    [setInvestigation, selectSeq, treeEvents],
  );

  return (
    <div className="flex flex-col h-full">
      <FilterBar />
      {loading && filteredEvents.length === 0 ? (
        <div className="animate-pulse">
          {Array.from({ length: 14 }).map((_, i) => (
            <div key={i} className="flex items-center gap-2 px-3 py-1.5 border-l-2 border-l-muted/30 border-b border-b-border/30" style={{ opacity: 1 - i * 0.05 }}>
              <div className="h-3 w-10 bg-muted/40 rounded shrink-0" />
              <div className="h-3 w-28 bg-muted/30 rounded shrink-0" />
              <div className="h-4 w-14 bg-muted/20 rounded shrink-0" />
              <div className="h-4 bg-muted/25 rounded shrink-0" style={{ width: `${80 + (i * 47) % 120}px` }} />
              <div className="h-3 bg-muted/15 rounded flex-1" style={{ maxWidth: `${120 + (i * 37) % 200}px` }} />
            </div>
          ))}
        </div>
      ) : filteredEvents.length === 0 ? (
        <div className="flex flex-col items-center justify-center h-40 gap-2">
          <Search className="w-5 h-5 text-muted-foreground/30" />
          <span className="text-xs text-muted-foreground/50">No events found</span>
        </div>
      ) : (
        <div className="flex-1 overflow-y-auto">
          {filteredEvents.map((event) => (
            <EventRow
              key={event.seq}
              event={event}
              isSelected={event.seq === selectedSeq}
              onClick={() => { selectSeq(event.seq, event.runId ?? undefined); if (event.runId) openFlow(event.runId, { kind: "event-type", handlerId: event.handlerId, name: event.name }); }}
              onInvestigate={() => handleInvestigate(event)}
              onFilterRun={setRunId}
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
