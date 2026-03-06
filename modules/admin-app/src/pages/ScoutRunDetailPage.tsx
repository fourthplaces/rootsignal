import { useState } from "react";
import { useParams, Link } from "react-router";
import { useQuery } from "@apollo/client";
import { ADMIN_SCOUT_RUN, ADMIN_SCOUT_RUN_OUTCOMES } from "@/graphql/queries";
import { InvestigateDrawer } from "@/components/InvestigateDrawer";

function ExternalLink({ href, children }: { href: string; children: React.ReactNode }) {
  return (
    <a href={href} target="_blank" rel="noopener noreferrer" className="text-blue-400 hover:underline break-all">
      {children}
    </a>
  );
}

function StatCard({ label, value, warn }: { label: string; value: string | number; warn?: boolean }) {
  return (
    <div className={`rounded-lg border p-4 ${warn ? "border-yellow-500/50 bg-yellow-500/5" : "border-border"}`}>
      <p className="text-xs text-muted-foreground">{label}</p>
      <p className={`text-lg font-semibold mt-1 tabular-nums ${warn ? "text-yellow-400" : ""}`}>{value}</p>
    </div>
  );
}

function SectionHeader({ title, total }: { title: string; total: number }) {
  return (
    <h2 className="text-sm font-semibold flex items-center gap-2">
      {title}
      <span className="text-xs font-normal text-muted-foreground bg-muted px-2 py-0.5 rounded-full tabular-nums">
        {total}
      </span>
    </h2>
  );
}

function EmptySection() {
  return <p className="text-xs text-muted-foreground py-2">None</p>;
}

function truncate(s: string, n: number) {
  return s.length > n ? s.slice(0, n) + "..." : s;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function OutcomeTable({ columns, rows }: { columns: { key: string; label: string; render?: (v: any, row: any) => React.ReactNode }[]; rows: Record<string, unknown>[] }) {
  if (rows.length === 0) return <EmptySection />;
  return (
    <div className="rounded-lg border border-border overflow-hidden">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b border-border bg-muted/50">
            {columns.map((c) => (
              <th key={c.key} className="text-left px-4 py-2 font-medium text-xs">{c.label}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, i) => (
            <tr key={i} className="border-b border-border last:border-0 hover:bg-muted/30">
              {columns.map((c) => (
                <td key={c.key} className="px-4 py-2 text-xs">
                  {c.render ? c.render(row[c.key], row) : (row[c.key] as string) ?? ""}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function ShowingCount({ shown, total }: { shown: number; total: number }) {
  if (total <= shown) return null;
  return <p className="text-xs text-muted-foreground mt-1">Showing {shown} of {total}</p>;
}

export function ScoutRunDetailPage() {
  const { runId } = useParams<{ runId: string }>();
  const [investigating, setInvestigating] = useState(false);

  const { data, loading } = useQuery(ADMIN_SCOUT_RUN, {
    variables: { runId: runId ?? "" },
    skip: !runId,
  });

  const { data: outcomesData, loading: outcomesLoading } = useQuery(ADMIN_SCOUT_RUN_OUTCOMES, {
    variables: { runId: runId ?? "" },
    skip: !runId,
  });

  const run = data?.adminScoutRun;

  if (loading) return <p className="text-muted-foreground">Loading run...</p>;
  if (!run) return <p className="text-muted-foreground">Run not found.</p>;

  const outcomes = outcomesData?.adminScoutRunOutcomes;

  const duration = (() => {
    if (!run.finishedAt) return "running";
    const ms = new Date(run.finishedAt).getTime() - new Date(run.startedAt).getTime();
    const secs = Math.round(ms / 1000);
    if (secs < 60) return `${secs}s`;
    const mins = Math.floor(secs / 60);
    return `${mins}m ${secs % 60}s`;
  })();

  const formatTs = (d: string) =>
    new Date(d).toLocaleString("en-US", {
      month: "short", day: "numeric",
      hour: "2-digit", minute: "2-digit", second: "2-digit",
    });

  return (
    <div className="space-y-6">
      {/* Breadcrumb + header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <Link to="/scout-runs" className="text-muted-foreground hover:text-foreground text-sm">Scout Runs</Link>
          <span className="text-muted-foreground">/</span>
          <h1 className="text-sm font-semibold font-mono">{run.runId.slice(0, 8)}</h1>
          {run.flowType && (
            <span className="text-xs px-2 py-0.5 rounded border border-border bg-muted">{run.flowType}</span>
          )}
          {!run.finishedAt && (
            <span className="text-xs px-2 py-0.5 rounded bg-green-500/10 text-green-400 border border-green-500/30">running</span>
          )}
        </div>
        <div className="flex items-center gap-3">
          <button
            onClick={() => setInvestigating(true)}
            className="text-xs px-3 py-1.5 rounded-md border border-border hover:bg-accent text-foreground transition-colors"
          >
            Investigate
          </button>
          <Link
            to={`/events?runId=${run.runId}`}
            className="text-xs text-blue-400 hover:underline"
          >
            View Events &rarr;
          </Link>
        </div>
      </div>

      {/* Timestamps */}
      <div className="flex gap-6 text-xs text-muted-foreground">
        <span>Started: {formatTs(run.startedAt)}</span>
        {run.finishedAt && <span>Finished: {formatTs(run.finishedAt)}</span>}
        <span>Duration: {duration}</span>
        <span>Region: {run.region}</span>
      </div>

      {/* Stats grid */}
      <div className="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-4">
        <StatCard label="URLs Scraped" value={run.stats.urlsScraped} />
        <StatCard label="Signals Stored" value={run.stats.signalsStored} />
        <StatCard label="Deduplicated" value={run.stats.signalsDeduplicated} />
        <StatCard label="Sources Discovered" value={run.stats.expansionSourcesCreated} />
        <StatCard label="Expansion Queries" value={run.stats.expansionQueriesCollected} />
        {run.stats.handlerFailures > 0 && (
          <StatCard label="Failures" value={run.stats.handlerFailures} warn />
        )}
      </div>

      {/* Outcome sections */}
      {outcomesLoading ? (
        <p className="text-muted-foreground text-sm">Loading outcomes...</p>
      ) : outcomes ? (
        <div className="space-y-6">
          {/* Sources Scraped */}
          <div className="space-y-2">
            <SectionHeader title="Sources Scraped" total={outcomes.sourcesScraped.total} />
            <OutcomeTable
              columns={[
                { key: "canonicalKey", label: "Source", render: (v: string) => <span className="font-mono">{truncate(v, 60)}</span> },
                { key: "url", label: "URL", render: (v: string) => v ? <ExternalLink href={v}>{truncate(v, 50)}</ExternalLink> : null },
                { key: "signalsProduced", label: "Signals" },
              ]}
              rows={outcomes.sourcesScraped.items}
            />
            <ShowingCount shown={outcomes.sourcesScraped.items.length} total={outcomes.sourcesScraped.total} />
          </div>

          {/* Signals Created */}
          <div className="space-y-2">
            <SectionHeader title="Signals Created" total={outcomes.signalsCreated.total} />
            <OutcomeTable
              columns={[
                { key: "nodeType", label: "Type", render: (v: string) => (
                  <span className="px-2 py-0.5 rounded text-xs border border-border bg-muted">{v}</span>
                )},
                { key: "title", label: "Title", render: (v: string, row: { nodeId: string }) => (
                  <Link to={`/signals/${row.nodeId}`} className="text-blue-400 hover:underline">{v ?? "untitled"}</Link>
                )},
                { key: "confidence", label: "Confidence", render: (v: number | null) => v != null ? `${(v * 100).toFixed(0)}%` : "" },
                { key: "sourceUrl", label: "Source", render: (v: string) => v ? <ExternalLink href={v}>{truncate(v, 40)}</ExternalLink> : null },
              ]}
              rows={outcomes.signalsCreated.items}
            />
            <ShowingCount shown={outcomes.signalsCreated.items.length} total={outcomes.signalsCreated.total} />
          </div>

          {/* Dedup Matches */}
          {outcomes.dedupMatches.total > 0 && (
            <div className="space-y-2">
              <SectionHeader title="Dedup Matches" total={outcomes.dedupMatches.total} />
              <OutcomeTable
                columns={[
                  { key: "nodeType", label: "Type" },
                  { key: "title", label: "Title" },
                  { key: "similarity", label: "Similarity", render: (v: number | null) => v != null ? `${(v * 100).toFixed(0)}%` : "" },
                  { key: "existingId", label: "Existing Signal", render: (v: string) => v ? (
                    <Link to={`/signals/${v}`} className="text-blue-400 hover:underline font-mono text-xs">{v.slice(0, 8)}</Link>
                  ) : null },
                ]}
                rows={outcomes.dedupMatches.items}
              />
              <ShowingCount shown={outcomes.dedupMatches.items.length} total={outcomes.dedupMatches.total} />
            </div>
          )}

          {/* Rejections */}
          {outcomes.rejections.total > 0 && (
            <div className="space-y-2">
              <SectionHeader title="Rejections" total={outcomes.rejections.total} />
              <OutcomeTable
                columns={[
                  { key: "title", label: "Title" },
                  { key: "reason", label: "Reason", render: (v: string) => <span className="text-red-400">{v}</span> },
                ]}
                rows={outcomes.rejections.items}
              />
              <ShowingCount shown={outcomes.rejections.items.length} total={outcomes.rejections.total} />
            </div>
          )}

          {/* Sources Discovered */}
          {outcomes.sourcesDiscovered.total > 0 && (
            <div className="space-y-2">
              <SectionHeader title="Sources Discovered" total={outcomes.sourcesDiscovered.total} />
              <OutcomeTable
                columns={[
                  { key: "canonicalKey", label: "Source", render: (v: string, row: { sourceId: string }) =>
                    row.sourceId
                      ? <Link to={`/sources/${row.sourceId}`} className="text-blue-400 hover:underline font-mono">{truncate(v, 50)}</Link>
                      : <span className="font-mono">{truncate(v, 50)}</span>
                  },
                  { key: "url", label: "URL", render: (v: string) => v ? <ExternalLink href={v}>{truncate(v, 40)}</ExternalLink> : null },
                  { key: "discoveryMethod", label: "Method" },
                  { key: "gapContext", label: "Gap Context", render: (v: string) => v ? <span className="text-muted-foreground">{truncate(v, 40)}</span> : null },
                ]}
                rows={outcomes.sourcesDiscovered.items}
              />
              <ShowingCount shown={outcomes.sourcesDiscovered.items.length} total={outcomes.sourcesDiscovered.total} />
            </div>
          )}

          {/* Expansion Queries */}
          {outcomes.expansionQueries.total > 0 && (
            <div className="space-y-2">
              <SectionHeader title="Expansion Queries" total={outcomes.expansionQueries.total} />
              <OutcomeTable
                columns={[
                  { key: "query", label: "Query" },
                  { key: "sourceUrl", label: "Source", render: (v: string) => v ? <ExternalLink href={v}>{truncate(v, 40)}</ExternalLink> : null },
                ]}
                rows={outcomes.expansionQueries.items}
              />
              <ShowingCount shown={outcomes.expansionQueries.items.length} total={outcomes.expansionQueries.total} />
            </div>
          )}

          {/* Failures */}
          {outcomes.failures.total > 0 && (
            <div className="space-y-2">
              <SectionHeader title="Failures" total={outcomes.failures.total} />
              <OutcomeTable
                columns={[
                  { key: "variant", label: "Type", render: (v: string) => (
                    <span className="px-2 py-0.5 rounded text-xs border border-red-500/30 bg-red-500/10 text-red-400">{v}</span>
                  )},
                  { key: "handlerId", label: "Handler", render: (v: string) => v ? <span className="font-mono">{v}</span> : null },
                  { key: "url", label: "URL", render: (v: string) => v ? <ExternalLink href={v}>{truncate(v, 40)}</ExternalLink> : null },
                  { key: "error", label: "Error", render: (v: string) => <span className="text-red-400">{truncate(v, 80)}</span> },
                ]}
                rows={outcomes.failures.items}
              />
              <ShowingCount shown={outcomes.failures.items.length} total={outcomes.failures.total} />
            </div>
          )}
        </div>
      ) : null}

      {/* Investigation drawer */}
      {investigating && runId && (
        <div className="fixed inset-0 z-50 flex">
          <div
            className="flex-1 bg-black/40"
            onClick={() => setInvestigating(false)}
          />
          <div className="w-[520px] bg-card border-l border-border flex flex-col">
            <InvestigateDrawer
              key={runId}
              investigation={{
                mode: "scout_run",
                runId,
                runLabel: runId.slice(0, 8),
              }}
              onClose={() => setInvestigating(false)}
            />
          </div>
        </div>
      )}
    </div>
  );
}
