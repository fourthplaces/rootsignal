import { useState, useMemo } from "react";
import { useQuery } from "@apollo/client";
import { useEventsPaneContext } from "../EventsPaneContext";
import { ADMIN_HANDLER_LOGS, ADMIN_HANDLER_LOGS_BY_RUN } from "@/graphql/queries";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

type HandlerLogEntry = {
  eventId: string;
  handlerId: string;
  level: string;
  message: string;
  data: string | null;
  loggedAt: string;
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const LOG_LEVEL_COLORS: Record<string, string> = {
  debug: "bg-zinc-600/30 text-zinc-400",
  info: "bg-blue-500/20 text-blue-400",
  warn: "bg-amber-500/20 text-amber-400",
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

// ---------------------------------------------------------------------------
// LogRow
// ---------------------------------------------------------------------------

function LogRow({ log, showHandler }: { log: HandlerLogEntry; showHandler: boolean }) {
  const [expanded, setExpanded] = useState(false);
  const levelColor = LOG_LEVEL_COLORS[log.level] ?? "bg-zinc-600/30 text-zinc-400";

  return (
    <div className="px-2 py-1 hover:bg-accent/20 rounded">
      <div className="flex items-center gap-1.5 min-w-0">
        <span className={`px-1 py-0.5 rounded text-[10px] font-semibold uppercase shrink-0 ${levelColor}`}>
          {log.level}
        </span>
        <span className="text-[10px] text-muted-foreground shrink-0">
          {formatTs(log.loggedAt)}
        </span>
        {showHandler && (
          <span className="text-[10px] font-mono text-zinc-500 shrink-0">
            {log.handlerId}
          </span>
        )}
        <span className="text-[11px] text-zinc-200 truncate">{log.message}</span>
        {log.data && (
          <button
            onClick={() => setExpanded((v) => !v)}
            className="ml-auto text-[10px] px-1 py-0.5 rounded hover:bg-accent shrink-0 text-muted-foreground hover:text-foreground"
          >
            {expanded ? "hide" : "data"}
          </button>
        )}
      </div>
      {expanded && log.data && (
        <pre className="mt-1 ml-4 text-[10px] font-mono text-zinc-400 bg-zinc-900/50 rounded p-2 max-h-32 overflow-auto whitespace-pre-wrap">
          {typeof log.data === "string" ? log.data : JSON.stringify(log.data, null, 2)}
        </pre>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// LogsPane
// ---------------------------------------------------------------------------

export function LogsPane() {
  const { logsFilter } = useEventsPaneContext();
  const [scope, setScope] = useState<"handler" | "run">("handler");
  const [levelFilter, setLevelFilter] = useState<Set<string>>(new Set(["debug", "info", "warn"]));
  const [searchText, setSearchText] = useState("");

  const isRunScope = scope === "run" && logsFilter?.runId != null;

  // Handler-scoped query
  const { data: handlerData, loading: handlerLoading } = useQuery<{
    adminHandlerLogs: HandlerLogEntry[];
  }>(ADMIN_HANDLER_LOGS, {
    variables: logsFilter ? { eventId: logsFilter.eventId, handlerId: logsFilter.handlerId } : undefined,
    skip: !logsFilter || isRunScope,
  });

  // Run-scoped query
  const { data: runData, loading: runLoading } = useQuery<{
    adminHandlerLogsByRun: HandlerLogEntry[];
  }>(ADMIN_HANDLER_LOGS_BY_RUN, {
    variables: logsFilter?.runId ? { runId: logsFilter.runId } : undefined,
    skip: !logsFilter || !isRunScope,
  });

  const loading = isRunScope ? runLoading : handlerLoading;
  const rawLogs = isRunScope
    ? (runData?.adminHandlerLogsByRun ?? [])
    : (handlerData?.adminHandlerLogs ?? []);

  // Client-side filtering
  const logs = useMemo(() => {
    let filtered = rawLogs.filter((l) => levelFilter.has(l.level));
    if (searchText) {
      const lower = searchText.toLowerCase();
      filtered = filtered.filter(
        (l) =>
          l.message.toLowerCase().includes(lower) ||
          l.handlerId.toLowerCase().includes(lower) ||
          (l.data && l.data.toLowerCase().includes(lower)),
      );
    }
    return filtered;
  }, [rawLogs, levelFilter, searchText]);

  if (!logsFilter) {
    return (
      <div className="flex items-center justify-center h-full text-sm text-muted-foreground">
        Click "logs" on a handler node to view logs
      </div>
    );
  }

  const toggleLevel = (level: string) => {
    setLevelFilter((prev) => {
      const next = new Set(prev);
      if (next.has(level)) next.delete(level);
      else next.add(level);
      return next;
    });
  };

  return (
    <div className="h-full flex flex-col">
      {/* Toolbar */}
      <div className="px-3 py-2 border-b border-border flex items-center gap-3 flex-wrap">
        {/* Scope toggle */}
        <div className="flex items-center gap-1 text-[11px]">
          <button
            onClick={() => setScope("handler")}
            className={`px-2 py-0.5 rounded ${scope === "handler" ? "bg-accent text-foreground" : "text-muted-foreground hover:text-foreground"}`}
          >
            This handler
          </button>
          {logsFilter.runId && (
            <button
              onClick={() => setScope("run")}
              className={`px-2 py-0.5 rounded ${scope === "run" ? "bg-accent text-foreground" : "text-muted-foreground hover:text-foreground"}`}
            >
              This run
            </button>
          )}
        </div>

        {/* Level filters */}
        <div className="flex items-center gap-1 text-[10px]">
          {["debug", "info", "warn"].map((level) => (
            <button
              key={level}
              onClick={() => toggleLevel(level)}
              className={`px-1.5 py-0.5 rounded uppercase font-semibold ${
                levelFilter.has(level)
                  ? LOG_LEVEL_COLORS[level]
                  : "text-zinc-600 line-through"
              }`}
            >
              {level}
            </button>
          ))}
        </div>

        {/* Search */}
        <input
          type="text"
          placeholder="Search logs..."
          value={searchText}
          onChange={(e) => setSearchText(e.target.value)}
          className="ml-auto text-[11px] bg-transparent border border-border rounded px-2 py-0.5 w-40 focus:outline-none focus:ring-1 focus:ring-blue-500/50 text-foreground placeholder:text-muted-foreground"
        />
      </div>

      {/* Header */}
      <div className="px-3 py-1.5 text-[10px] text-muted-foreground">
        <span className="font-mono">{logsFilter.handlerId}</span>
        {isRunScope && <span className="ml-1">(all handlers in run)</span>}
        {!loading && <span className="ml-2">{logs.length} logs</span>}
      </div>

      {/* Log list */}
      <div className="flex-1 overflow-y-auto px-1">
        {loading && (
          <div className="p-3 text-[11px] text-muted-foreground animate-pulse">Loading logs...</div>
        )}
        {!loading && logs.length === 0 && (
          <div className="p-3 text-[11px] text-muted-foreground">No logs match filters</div>
        )}
        {logs.map((log, i) => (
          <LogRow key={i} log={log} showHandler={isRunScope} />
        ))}
      </div>
    </div>
  );
}
