import { useState, useMemo } from "react";
import { useQuery } from "@apollo/client";
import { ADMIN_SCOUT_RUN_EVENTS } from "@/graphql/queries";
import { EVENT_COLORS, eventDetail, truncate, type ScoutRunEvent } from "@/lib/event-colors";

type TreeNode = ScoutRunEvent & { children: TreeNode[] };

function buildTree(events: ScoutRunEvent[]): TreeNode[] {
  const byId = new Map<string, TreeNode>();
  for (const e of events) {
    byId.set(e.id, { ...e, children: [] });
  }
  const roots: TreeNode[] = [];
  for (const node of byId.values()) {
    if (node.parentId && byId.has(node.parentId)) {
      byId.get(node.parentId)!.children.push(node);
    } else {
      roots.push(node);
    }
  }
  return roots;
}

function summarizeChildren(children: TreeNode[]): string {
  const counts: Record<string, number> = {};
  for (const c of children) {
    const key = c.type.replace("signal_", "");
    counts[key] = (counts[key] || 0) + 1;
  }
  return Object.entries(counts)
    .map(([k, v]) => `${v} ${k}`)
    .join(", ");
}

/** Extract a displayable URL from any event (checks url, sourceUrl, newSourceUrl). */
function eventUrl(e: ScoutRunEvent): string | undefined {
  return e.url || e.sourceUrl || e.newSourceUrl || undefined;
}

/** Check if an event or any of its descendants match the text filter. */
function treeMatchesText(node: TreeNode, filter: string): boolean {
  const lower = filter.toLowerCase();
  const url = eventUrl(node);
  if (url && url.toLowerCase().includes(lower)) return true;
  if (node.query && node.query.toLowerCase().includes(lower)) return true;
  if (node.title && node.title.toLowerCase().includes(lower)) return true;
  if (node.canonicalKey && node.canonicalKey.toLowerCase().includes(lower)) return true;
  return node.children.some((c) => treeMatchesText(c, filter));
}

/** Check if a node or any of its descendants match the type filter. */
function treeMatchesType(node: TreeNode, typeFilter: string): boolean {
  if (node.type === typeFilter) return true;
  return node.children.some((c) => treeMatchesType(c, typeFilter));
}

function filterTree(roots: TreeNode[], filter: string, typeFilter?: string): TreeNode[] {
  let result = roots;
  if (filter) result = result.filter((r) => treeMatchesText(r, filter));
  if (typeFilter) result = result.filter((r) => treeMatchesType(r, typeFilter));
  return result;
}

/** Collect unique event types with counts. */
function collectEventTypes(events: ScoutRunEvent[]): { type: string; count: number }[] {
  const counts: Record<string, number> = {};
  for (const e of events) {
    counts[e.type] = (counts[e.type] || 0) + 1;
  }
  return Object.entries(counts)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([type, count]) => ({ type, count }));
}

type Stats = {
  urlsScraped: number;
  signalsCreated: number;
  signalsDeduped: number;
  signalsCorroborated: number;
  signalsRejected: number;
  signalsDropped: number;
  expansionQueries: number;
};

function computeStats(events: ScoutRunEvent[]): Stats {
  const stats: Stats = {
    urlsScraped: 0,
    signalsCreated: 0,
    signalsDeduped: 0,
    signalsCorroborated: 0,
    signalsRejected: 0,
    signalsDropped: 0,
    expansionQueries: 0,
  };
  for (const e of events) {
    switch (e.type) {
      case "scrape_url":
        if (e.success) stats.urlsScraped++;
        break;
      case "signal_created":
        stats.signalsCreated++;
        break;
      case "signal_deduplicated":
        stats.signalsDeduped++;
        break;
      case "signal_corroborated":
        stats.signalsCorroborated++;
        break;
      case "signal_rejected":
        stats.signalsRejected++;
        break;
      case "signal_dropped_no_date":
        stats.signalsDropped++;
        break;
      case "expansion_query_collected":
        stats.expansionQueries++;
        break;
    }
  }
  return stats;
}

/** Collect unique source URLs across all events for the filter dropdown. */
function collectSources(events: ScoutRunEvent[]): string[] {
  const urls = new Set<string>();
  for (const e of events) {
    // scrape_url and scrape_feed use `url`
    if (e.type === "scrape_url" && e.url) urls.add(e.url);
    if (e.type === "scrape_feed" && e.url) urls.add(e.url);
    // social uses platform:identifier
    if (e.type === "social_scrape" && e.platform && e.identifier)
      urls.add(`${e.platform}:${e.identifier}`);
  }
  return [...urls].sort();
}

function TreeNodeRow({
  node,
  depth,
}: {
  node: TreeNode;
  depth: number;
}) {
  const [expanded, setExpanded] = useState(false);
  const hasChildren = node.children.length > 0;

  const formatTime = (d: string) =>
    new Date(d).toLocaleTimeString("en-US", {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });

  return (
    <>
      <div
        className="flex items-start gap-2 py-1.5 px-3 hover:bg-muted/30 border-b border-border/50"
        style={{ paddingLeft: `${depth * 24 + 12}px` }}
      >
        {/* Expand/collapse */}
        <button
          onClick={() => setExpanded(!expanded)}
          className="w-4 h-4 flex items-center justify-center text-muted-foreground shrink-0 mt-0.5"
          disabled={!hasChildren}
        >
          {hasChildren ? (
            <svg
              className={`w-3 h-3 transition-transform ${expanded ? "rotate-90" : ""}`}
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              strokeWidth={2}
            >
              <path strokeLinecap="round" strokeLinejoin="round" d="M9 5l7 7-7 7" />
            </svg>
          ) : (
            <span className="w-3" />
          )}
        </button>

        {/* Event type badge */}
        <span
          className={`inline-block px-2 py-0.5 rounded text-xs border shrink-0 ${
            EVENT_COLORS[node.type] ?? "bg-muted text-muted-foreground"
          }`}
        >
          {node.type}
        </span>

        {/* Details */}
        <span className="text-xs text-muted-foreground font-mono truncate flex-1">
          {eventDetail(node)}
        </span>

        {/* Collapsed summary badge */}
        {hasChildren && !expanded && (
          <span className="text-xs text-muted-foreground bg-muted/50 px-2 py-0.5 rounded shrink-0">
            {summarizeChildren(node.children)}
          </span>
        )}

        {/* Timestamp */}
        <span className="text-xs text-muted-foreground tabular-nums whitespace-nowrap shrink-0">
          {formatTime(node.ts)}
        </span>
      </div>

      {/* Children */}
      {expanded &&
        node.children.map((child) => (
          <TreeNodeRow key={child.id} node={child} depth={depth + 1} />
        ))}
    </>
  );
}

export function SourceTrace({ runId }: { runId: string }) {
  const { data, loading } = useQuery(ADMIN_SCOUT_RUN_EVENTS, {
    variables: { runId },
  });
  const [filter, setFilter] = useState("");
  const [typeFilter, setTypeFilter] = useState<string | undefined>(undefined);

  const events: ScoutRunEvent[] = data?.adminScoutRunEvents ?? [];
  const stats = useMemo(() => computeStats(events), [events]);
  const sources = useMemo(() => collectSources(events), [events]);
  const eventTypes = useMemo(() => collectEventTypes(events), [events]);
  const tree = useMemo(() => buildTree(events), [events]);
  const filtered = useMemo(() => filterTree(tree, filter, typeFilter), [tree, filter, typeFilter]);

  if (loading) {
    return <p className="text-muted-foreground">Loading events...</p>;
  }

  if (events.length === 0) {
    return <p className="text-muted-foreground">No events recorded for this run.</p>;
  }

  return (
    <div className="space-y-3">
      {/* Stats bar */}
      <div className="flex flex-wrap gap-x-5 gap-y-1 text-xs">
        <Stat label="URLs scraped" value={stats.urlsScraped} />
        <Stat label="Signals created" value={stats.signalsCreated} color="text-green-400" />
        <Stat label="Deduped" value={stats.signalsDeduped} color="text-orange-400" />
        <Stat label="Corroborated" value={stats.signalsCorroborated} color="text-emerald-400" />
        <Stat label="Rejected" value={stats.signalsRejected} color="text-red-400" />
        <Stat label="Dropped (no date)" value={stats.signalsDropped} color="text-red-300" />
        <Stat label="Expansion queries" value={stats.expansionQueries} color="text-violet-400" />
      </div>

      {/* Filter */}
      <div className="flex gap-2 items-center">
        <input
          type="text"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="Filter by URL, query, or title..."
          className="flex-1 px-3 py-1.5 rounded-md border border-input bg-background text-sm placeholder:text-muted-foreground"
        />
        <select
          value={typeFilter ?? ""}
          onChange={(e) => setTypeFilter(e.target.value || undefined)}
          className="px-2 py-1.5 rounded-md border border-input bg-background text-sm text-muted-foreground"
        >
          <option value="">All types</option>
          {eventTypes.map((t) => (
            <option key={t.type} value={t.type}>
              {t.type} ({t.count})
            </option>
          ))}
        </select>
        {sources.length > 0 && (
          <select
            value=""
            onChange={(e) => { if (e.target.value) setFilter(e.target.value); }}
            className="px-2 py-1.5 rounded-md border border-input bg-background text-sm text-muted-foreground max-w-[250px]"
          >
            <option value="">Sources ({sources.length})</option>
            {sources.map((s) => (
              <option key={s} value={s}>
                {truncate(s, 60)}
              </option>
            ))}
          </select>
        )}
        {(filter || typeFilter) && (
          <button
            onClick={() => { setFilter(""); setTypeFilter(undefined); }}
            className="text-xs text-muted-foreground hover:text-foreground px-2 py-1.5 rounded border border-input"
          >
            Clear
          </button>
        )}
      </div>

      {/* Event tree */}
      <div className="rounded-lg border border-border overflow-hidden">
        <div className="bg-muted/50 px-3 py-2 text-xs text-muted-foreground border-b border-border flex justify-between">
          <span>
            {filter
              ? `${filtered.length} of ${tree.length} root events`
              : `${events.length} events`}
          </span>
          <span>
            {[...new Set(events.map((e) => e.type))].length} types
          </span>
        </div>
        <div className="max-h-[600px] overflow-y-auto">
          {filtered.length === 0 ? (
            <p className="text-muted-foreground text-sm px-3 py-4">
              No events match "{filter}"
            </p>
          ) : (
            filtered.map((node) => (
              <TreeNodeRow key={node.id} node={node} depth={0} />
            ))
          )}
        </div>
      </div>
    </div>
  );
}

function Stat({
  label,
  value,
  color,
}: {
  label: string;
  value: number;
  color?: string;
}) {
  if (value === 0) return null;
  return (
    <span>
      <span className="text-muted-foreground">{label}:</span>{" "}
      <span className={`font-medium tabular-nums ${color ?? "text-foreground"}`}>
        {value}
      </span>
    </span>
  );
}
