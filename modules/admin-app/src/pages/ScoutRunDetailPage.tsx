import { useState } from "react";
import { useParams, Link } from "react-router";
import { useQuery } from "@apollo/client";
import { ADMIN_SCOUT_RUN, ADMIN_SCOUT_RUN_EVENTS, SIGNAL_BRIEF } from "@/graphql/queries";
import { EVENT_COLORS, eventDetail, truncate, formatBytes, type ScoutRunEvent } from "@/lib/event-colors";

function ExternalLink({ href, children }: { href: string; children: React.ReactNode }) {
  return (
    <a href={href} target="_blank" rel="noopener noreferrer" className="text-blue-400 hover:underline break-all">
      {children}
    </a>
  );
}

function KV({ label, children }: { label: string; children: React.ReactNode }) {
  if (children == null || children === "") return null;
  return (
    <div className="flex gap-2 text-xs">
      <span className="text-muted-foreground shrink-0">{label}:</span>
      <span className="text-foreground break-all">{children}</span>
    </div>
  );
}

function SignalBriefCard({ signalId, label }: { signalId: string; label: string }) {
  const { data, loading } = useQuery(SIGNAL_BRIEF, {
    variables: { id: signalId },
    skip: !signalId,
  });

  const signal = data?.signal;

  return (
    <div className="flex-1 rounded border border-border p-3 space-y-2 min-w-0">
      <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide">{label}</p>
      {loading && <p className="text-xs text-muted-foreground">Loading...</p>}
      {signal && (
        <>
          <p className="text-sm font-medium">
            <Link to={`/signals/${signal.id}`} className="text-blue-400 hover:underline">
              {signal.title}
            </Link>
          </p>
          {signal.summary && <p className="text-xs text-muted-foreground">{signal.summary}</p>}
          <div className="flex flex-wrap gap-3 text-xs text-muted-foreground">
            {signal.confidence != null && <span>confidence: {signal.confidence.toFixed(2)}</span>}
            {signal.contentDate && <span>date: {signal.contentDate}</span>}
            {signal.locationName && <span>{signal.locationName}</span>}
          </div>
          {signal.sourceUrl && (
            <ExternalLink href={signal.sourceUrl}>{truncate(signal.sourceUrl, 60)}</ExternalLink>
          )}
        </>
      )}
      {!loading && !signal && <p className="text-xs text-muted-foreground">Signal not found</p>}
    </div>
  );
}

function DedupPanel({ e }: { e: ScoutRunEvent }) {
  const matchId = e.matchedId ?? e.existingId;
  return (
    <div className="flex gap-4">
      <div className="flex-1 rounded border border-border p-3 space-y-2 min-w-0">
        <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide">Incoming Signal</p>
        <p className="text-sm font-medium">{e.title}</p>
        {e.summary && <p className="text-xs text-muted-foreground">{e.summary}</p>}
        <div className="flex flex-wrap gap-3 text-xs text-muted-foreground">
          {e.signalType && <span>{e.signalType}</span>}
          {e.confidence != null && <span>confidence: {e.confidence.toFixed(2)}</span>}
        </div>
        {(e.sourceUrl ?? e.newSourceUrl) && (
          <ExternalLink href={(e.sourceUrl ?? e.newSourceUrl)!}>
            {truncate((e.sourceUrl ?? e.newSourceUrl)!, 60)}
          </ExternalLink>
        )}
      </div>
      <div className="flex flex-col items-center justify-center px-2">
        <span className="text-xs font-mono text-muted-foreground">sim</span>
        <span className="text-lg font-bold tabular-nums">
          {(e.similarity ?? 0).toFixed(3)}
        </span>
        {e.action && <span className="text-xs text-muted-foreground mt-1">{e.action}</span>}
      </div>
      {matchId ? (
        <SignalBriefCard signalId={matchId} label="Existing Signal" />
      ) : (
        <div className="flex-1 rounded border border-border p-3">
          <p className="text-xs text-muted-foreground">No matched signal ID</p>
        </div>
      )}
    </div>
  );
}

function EventDetailPanel({ e }: { e: ScoutRunEvent }) {
  switch (e.type) {
    case "signal_deduplicated":
    case "signal_corroborated":
      return <DedupPanel e={e} />;

    case "signal_created":
      return (
        <div className="space-y-1">
          <KV label="Title">{e.title}</KV>
          <KV label="Type">{e.signalType}</KV>
          <KV label="Confidence">{e.confidence?.toFixed(2)}</KV>
          {e.nodeId && (
            <KV label="Signal">
              <Link to={`/signals/${e.nodeId}`} className="text-blue-400 hover:underline">{e.nodeId.slice(0, 8)}</Link>
            </KV>
          )}
          {e.sourceUrl && <KV label="Source"><ExternalLink href={e.sourceUrl}>{e.sourceUrl}</ExternalLink></KV>}
        </div>
      );

    case "signal_rejected":
      return (
        <div className="space-y-1">
          <KV label="Title">{e.title}</KV>
          <KV label="Reason"><span className="text-red-400">{e.reason}</span></KV>
          {e.sourceUrl && <KV label="Source"><ExternalLink href={e.sourceUrl}>{e.sourceUrl}</ExternalLink></KV>}
        </div>
      );

    case "signal_dropped_no_date":
      return (
        <div className="space-y-1">
          <KV label="Title">{e.title}</KV>
          {e.sourceUrl && <KV label="Source"><ExternalLink href={e.sourceUrl}>{e.sourceUrl}</ExternalLink></KV>}
        </div>
      );

    case "scrape_url":
      return (
        <div className="space-y-1">
          {e.url && <KV label="URL"><ExternalLink href={e.url}>{e.url}</ExternalLink></KV>}
          <KV label="Strategy">{e.strategy}</KV>
          <KV label="Result">{e.success ? `Success (${formatBytes(e.contentBytes ?? 0)})` : "Failed"}</KV>
        </div>
      );

    case "search_query":
      return (
        <div className="space-y-1">
          <KV label="Query">{e.query}</KV>
          <KV label="Provider">{e.provider}</KV>
          <KV label="Results">{e.resultCount}</KV>
          <KV label="Canonical Key">{e.canonicalKey}</KV>
        </div>
      );

    case "llm_extraction":
      return (
        <div className="space-y-1">
          {e.sourceUrl && <KV label="Source"><ExternalLink href={e.sourceUrl}>{e.sourceUrl}</ExternalLink></KV>}
          <KV label="Content">{e.contentChars?.toLocaleString()} chars</KV>
          <KV label="Signals Extracted">{e.signalsExtracted}</KV>
          <KV label="Implied Queries">{e.impliedQueries}</KV>
        </div>
      );

    case "lint_batch":
      return (
        <div className="space-y-1">
          {e.sourceUrl && <KV label="Source"><ExternalLink href={e.sourceUrl}>{e.sourceUrl}</ExternalLink></KV>}
          <KV label="Total Signals">{e.signalCount}</KV>
          <div className="flex gap-4 text-xs">
            <span className="text-green-400">{e.resultCount} passed</span>
            <span className="text-yellow-400">{e.postCount} corrected</span>
            <span className="text-red-400">{e.items} rejected</span>
          </div>
        </div>
      );

    case "lint_correction":
      return (
        <div className="space-y-1">
          {e.nodeId && (
            <KV label="Signal">
              <Link to={`/signals/${e.nodeId}`} className="text-blue-400 hover:underline">
                {e.title ?? e.nodeId.slice(0, 8)}
              </Link>
              {e.signalType && <span className="text-muted-foreground ml-2">({e.signalType})</span>}
            </KV>
          )}
          <KV label="Field">{e.field}</KV>
          <div className="flex items-center gap-2 text-xs">
            <span className="rounded bg-red-500/10 border border-red-500/20 px-2 py-0.5 line-through">{e.oldValue}</span>
            <span className="text-muted-foreground">→</span>
            <span className="rounded bg-green-500/10 border border-green-500/20 px-2 py-0.5">{e.newValue}</span>
          </div>
          <KV label="Reason">{e.reason}</KV>
        </div>
      );

    case "lint_rejection":
      return (
        <div className="space-y-1">
          {e.nodeId && (
            <KV label="Signal">
              <Link to={`/signals/${e.nodeId}`} className="text-blue-400 hover:underline">
                {e.title ?? e.nodeId.slice(0, 8)}
              </Link>
              {e.signalType && <span className="text-muted-foreground ml-2">({e.signalType})</span>}
            </KV>
          )}
          <KV label="Reason"><span className="text-red-400">{e.reason}</span></KV>
        </div>
      );

    case "expansion_query_collected":
      return (
        <div className="space-y-1">
          <KV label="Query">{e.query}</KV>
          {e.sourceUrl && <KV label="Source"><ExternalLink href={e.sourceUrl}>{e.sourceUrl}</ExternalLink></KV>}
        </div>
      );

    case "expansion_source_created":
      return (
        <div className="space-y-1">
          <KV label="Canonical Key">{e.canonicalKey}</KV>
          <KV label="Query">{e.query}</KV>
          {e.sourceUrl && <KV label="Source"><ExternalLink href={e.sourceUrl}>{e.sourceUrl}</ExternalLink></KV>}
        </div>
      );

    default: {
      // Fallback: show all non-null fields as key-value pairs
      const skip = new Set(["id", "parentId", "seq", "ts", "type"]);
      const entries = Object.entries(e).filter(
        ([k, v]) => !skip.has(k) && v != null && v !== "",
      );
      if (entries.length === 0) return <p className="text-xs text-muted-foreground">No additional details</p>;
      return (
        <div className="space-y-1">
          {entries.map(([k, v]) => (
            <KV key={k} label={k}>{typeof v === "object" ? JSON.stringify(v) : String(v)}</KV>
          ))}
        </div>
      );
    }
  }
}

export function ScoutRunDetailPage() {
  const { runId } = useParams<{ runId: string }>();
  const [typeFilter, setTypeFilter] = useState<string | undefined>(undefined);
  const [expandedId, setExpandedId] = useState<string | null>(null);

  const { data, loading } = useQuery(ADMIN_SCOUT_RUN, {
    variables: { runId: runId ?? "" },
    skip: !runId,
  });

  const { data: eventsData, loading: eventsLoading } = useQuery(ADMIN_SCOUT_RUN_EVENTS, {
    variables: { runId: runId ?? "" },
    skip: !runId,
  });

  const run = data?.adminScoutRun;

  if (loading) {
    return <p className="text-muted-foreground">Loading run...</p>;
  }

  if (!run) {
    return <p className="text-muted-foreground">Run not found.</p>;
  }

  const events: ScoutRunEvent[] = eventsData?.adminScoutRunEvents ?? [];
  const eventTypes = [...new Set(events.map((e: ScoutRunEvent) => e.type))].sort();
  const filtered = typeFilter
    ? events.filter((e: ScoutRunEvent) => e.type === typeFilter)
    : events;

  const duration = (() => {
    const ms =
      new Date(run.finishedAt).getTime() - new Date(run.startedAt).getTime();
    const secs = Math.round(ms / 1000);
    if (secs < 60) return `${secs}s`;
    const mins = Math.floor(secs / 60);
    return `${mins}m ${secs % 60}s`;
  })();

  const formatTime = (d: string) =>
    new Date(d).toLocaleTimeString("en-US", {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-3">
        <Link
          to="/scout-runs"
          className="text-muted-foreground hover:text-foreground text-sm"
        >
          Scout Runs
        </Link>
        <span className="text-muted-foreground">/</span>
        <h1 className="text-xl font-semibold font-mono text-sm">
          {run.runId.slice(0, 8)}
        </h1>
      </div>

      {/* Header stats */}
      <div className="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-4">
        {[
          { label: "Region", value: run.region },
          { label: "Duration", value: duration },
          { label: "URLs Scraped", value: run.stats.urlsScraped },
          { label: "Signals Stored", value: run.stats.signalsStored },
          { label: "Deduplicated", value: run.stats.signalsDeduplicated },
          { label: "Events", value: eventsLoading ? "..." : events.length },
        ].map((stat) => (
          <div key={stat.label} className="rounded-lg border border-border p-4">
            <p className="text-xs text-muted-foreground">{stat.label}</p>
            <p className="text-lg font-semibold mt-1">{stat.value}</p>
          </div>
        ))}
      </div>

      {/* Filter */}
      <div className="flex gap-3 items-center">
        <select
          value={typeFilter ?? ""}
          onChange={(e) => setTypeFilter(e.target.value || undefined)}
          className="px-3 py-1.5 rounded-md border border-input bg-background text-sm"
        >
          <option value="">All event types ({events.length})</option>
          {eventTypes.map((t) => (
            <option key={t} value={t}>
              {t} ({events.filter((e: ScoutRunEvent) => e.type === t).length})
            </option>
          ))}
        </select>
        <span className="text-xs text-muted-foreground">
          Showing {filtered.length} of {events.length} events
        </span>
      </div>

      {/* Event timeline */}
      {eventsLoading ? (
        <p className="text-muted-foreground">Loading events...</p>
      ) : (
        <div className="rounded-lg border border-border overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border bg-muted/50">
                <th className="text-left px-4 py-2 font-medium w-12">#</th>
                <th className="text-left px-4 py-2 font-medium w-24">Time</th>
                <th className="text-left px-4 py-2 font-medium w-48">Type</th>
                <th className="text-left px-4 py-2 font-medium">Details</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((e: ScoutRunEvent) => {
                const isExpanded = expandedId === e.id;
                return (
                  <>
                    <tr
                      key={e.id}
                      className={`border-b border-border last:border-0 hover:bg-muted/30 cursor-pointer ${isExpanded ? "bg-muted/40" : ""}`}
                      onClick={() => setExpandedId(isExpanded ? null : e.id)}
                    >
                      <td className="px-4 py-2 text-muted-foreground tabular-nums">
                        {e.seq}
                      </td>
                      <td className="px-4 py-2 text-muted-foreground whitespace-nowrap tabular-nums text-xs">
                        {formatTime(e.ts)}
                      </td>
                      <td className="px-4 py-2">
                        <span
                          className={`inline-block px-2 py-0.5 rounded text-xs border ${EVENT_COLORS[e.type] ?? "bg-muted text-muted-foreground"}`}
                        >
                          {e.type}
                        </span>
                      </td>
                      <td className="px-4 py-2 text-muted-foreground font-mono text-xs truncate max-w-lg">
                        <span className="flex items-center gap-2">
                          <span className={`transition-transform text-xs ${isExpanded ? "rotate-90" : ""}`}>▶</span>
                          {eventDetail(e)}
                        </span>
                      </td>
                    </tr>
                    {isExpanded && (
                      <tr key={`${e.id}-detail`} className="border-b border-border bg-muted/20">
                        <td colSpan={4} className="px-6 py-4">
                          <EventDetailPanel e={e} />
                        </td>
                      </tr>
                    )}
                  </>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
