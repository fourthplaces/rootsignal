import { useState } from "react";
import { useParams, Link } from "react-router";
import { useQuery } from "@apollo/client";
import { ADMIN_SCOUT_RUN } from "@/graphql/queries";

const EVENT_COLORS: Record<string, string> = {
  reap_expired: "bg-gray-500/10 text-gray-400 border-gray-500/20",
  bootstrap: "bg-purple-500/10 text-purple-400 border-purple-500/20",
  search_query: "bg-blue-500/10 text-blue-400 border-blue-500/20",
  scrape_url: "bg-cyan-500/10 text-cyan-400 border-cyan-500/20",
  scrape_feed: "bg-cyan-500/10 text-cyan-400 border-cyan-500/20",
  social_scrape: "bg-pink-500/10 text-pink-400 border-pink-500/20",
  social_topic_search: "bg-pink-500/10 text-pink-400 border-pink-500/20",
  llm_extraction: "bg-amber-500/10 text-amber-400 border-amber-500/20",
  signal_created: "bg-green-500/10 text-green-400 border-green-500/20",
  signal_deduplicated: "bg-orange-500/10 text-orange-400 border-orange-500/20",
  signal_corroborated:
    "bg-emerald-500/10 text-emerald-400 border-emerald-500/20",
  expansion_query_collected:
    "bg-violet-500/10 text-violet-400 border-violet-500/20",
  expansion_source_created:
    "bg-violet-500/10 text-violet-400 border-violet-500/20",
  budget_checkpoint: "bg-gray-500/10 text-gray-400 border-gray-500/20",
};

type ScoutRunEvent = {
  seq: number;
  ts: string;
  type: string;
  query?: string;
  url?: string;
  provider?: string;
  platform?: string;
  identifier?: string;
  signalType?: string;
  title?: string;
  resultCount?: number;
  postCount?: number;
  items?: number;
  contentBytes?: number;
  contentChars?: number;
  signalsExtracted?: number;
  impliedQueries?: number;
  similarity?: number;
  confidence?: number;
  success?: boolean;
  action?: string;
  nodeId?: string;
  matchedId?: string;
  existingId?: string;
  sourceUrl?: string;
  newSourceUrl?: string;
  canonicalKey?: string;
  gatherings?: number;
  needs?: number;
  stale?: number;
  sourcesCreated?: number;
  spentCents?: number;
  remainingCents?: number;
  topics?: string[];
  postsFound?: number;
};

/** Build a human-readable detail string for an event. */
function eventDetail(e: ScoutRunEvent): string {
  switch (e.type) {
    case "reap_expired":
      return `gatherings=${e.gatherings} needs=${e.needs} stale=${e.stale}`;
    case "bootstrap":
      return `${e.sourcesCreated} sources created`;
    case "search_query":
      return `"${e.query}" → ${e.resultCount} results (${e.provider})`;
    case "scrape_url":
      return `${truncate(e.url ?? "", 60)} ${e.success ? `(${formatBytes(e.contentBytes ?? 0)})` : "(failed)"}`;
    case "scrape_feed":
      return `${truncate(e.url ?? "", 50)} → ${e.items} items`;
    case "social_scrape":
      return `${e.platform}: ${truncate(e.identifier ?? "", 40)} → ${e.postCount} posts`;
    case "social_topic_search":
      return `${e.platform}: ${e.topics?.join(", ")} → ${e.postsFound} posts`;
    case "llm_extraction":
      return `${truncate(e.sourceUrl ?? "", 40)} → ${e.signalsExtracted} signals, ${e.impliedQueries ?? 0} queries`;
    case "signal_created":
      return `${e.signalType}: "${truncate(e.title ?? "", 40)}" (${(e.confidence ?? 0).toFixed(2)})`;
    case "signal_deduplicated":
      return `${e.signalType}: "${truncate(e.title ?? "", 30)}" → ${e.action} (sim=${(e.similarity ?? 0).toFixed(3)})`;
    case "signal_corroborated":
      return `${e.signalType}: ${e.existingId?.slice(0, 8)} ← ${truncate(e.newSourceUrl ?? "", 30)} (sim=${(e.similarity ?? 0).toFixed(3)})`;
    case "expansion_query_collected":
      return `"${e.query}"`;
    case "expansion_source_created":
      return `"${e.query}" → ${e.canonicalKey}`;
    case "budget_checkpoint":
      return `spent=${e.spentCents}¢ remaining=${e.remainingCents === 18446744073709551615 ? "∞" : `${e.remainingCents}¢`}`;
    default:
      return "";
  }
}

function truncate(s: string, max: number): string {
  return s.length <= max ? s : s.slice(0, max - 1) + "…";
}

function formatBytes(b: number): string {
  if (b < 1024) return `${b}B`;
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)}KB`;
  return `${(b / (1024 * 1024)).toFixed(1)}MB`;
}

export function ScoutRunDetailPage() {
  const { runId } = useParams<{ runId: string }>();
  const [typeFilter, setTypeFilter] = useState<string | undefined>(undefined);

  const { data, loading } = useQuery(ADMIN_SCOUT_RUN, {
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

  const events: ScoutRunEvent[] = run.events ?? [];
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
          { label: "Events", value: events.length },
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
            {filtered.map((e: ScoutRunEvent) => (
              <tr
                key={e.seq}
                className="border-b border-border last:border-0 hover:bg-muted/30"
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
                  {eventDetail(e)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
