import { useState } from "react";
import { useParams, Link } from "react-router";
import { useQuery } from "@apollo/client";
import { ADMIN_SCOUT_RUN, ADMIN_SCOUT_RUN_OUTCOMES, ADMIN_COALESCE_RUN_OUTCOMES } from "@/graphql/queries";
import { InvestigateDrawer } from "@/components/InvestigateDrawer";

const SIGNAL_TYPE_COLORS: Record<string, string> = {
  Gathering: "bg-blue-500/10 text-blue-400 border-blue-500/20",
  Resource: "bg-green-500/10 text-green-400 border-green-500/20",
  HelpRequest: "bg-amber-500/10 text-amber-400 border-amber-500/20",
  Announcement: "bg-purple-500/10 text-purple-400 border-purple-500/20",
  Concern: "bg-red-500/10 text-red-400 border-red-500/20",
  Condition: "bg-orange-500/10 text-orange-400 border-orange-500/20",
};

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

// ─── Scout outcomes ─────────────────────────────────────────────────

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function ScoutOutcomes({ runId, stats }: { runId: string; stats: any }) {
  const { data: outcomesData, loading } = useQuery(ADMIN_SCOUT_RUN_OUTCOMES, {
    variables: { runId },
  });
  const outcomes = outcomesData?.adminScoutRunOutcomes;

  return (
    <>
      <div className="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-4">
        <StatCard label="URLs Scraped" value={stats.urlsScraped} />
        <StatCard label="Signals Extracted" value={stats.signalsExtracted} />
        <StatCard label="Signals Stored" value={stats.signalsStored} />
        <StatCard label="Deduplicated" value={stats.signalsDeduplicated} />
        <StatCard label="Sources Discovered" value={stats.expansionSourcesCreated} />
        <StatCard label="Expansion Queries" value={stats.expansionQueriesCollected} />
        {stats.handlerFailures > 0 && (
          <StatCard label="Failures" value={stats.handlerFailures} warn />
        )}
      </div>

      {loading ? (
        <p className="text-muted-foreground text-sm">Loading outcomes...</p>
      ) : outcomes ? (
        <div className="space-y-6">
          <div className="space-y-2">
            <SectionHeader title="Sources Scraped" total={outcomes.sourcesScraped.total} />
            <OutcomeTable
              columns={[
                { key: "canonicalKey", label: "Source", render: (v: string, row: { sourceId: string }) =>
                  row.sourceId
                    ? <Link to={`/sources/${row.sourceId}`} className="text-blue-400 hover:underline font-mono">{truncate(v, 60)}</Link>
                    : <span className="font-mono">{truncate(v, 60)}</span>
                },
                { key: "url", label: "URL", render: (v: string) => v ? <ExternalLink href={v}>{truncate(v, 50)}</ExternalLink> : null },
                { key: "signalsProduced", label: "Signals" },
              ]}
              rows={outcomes.sourcesScraped.items}
            />
            <ShowingCount shown={outcomes.sourcesScraped.items.length} total={outcomes.sourcesScraped.total} />
          </div>

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
                { key: "url", label: "Source", render: (v: string) => v ? <ExternalLink href={v}>{truncate(v, 40)}</ExternalLink> : null },
              ]}
              rows={outcomes.signalsCreated.items}
            />
            <ShowingCount shown={outcomes.signalsCreated.items.length} total={outcomes.signalsCreated.total} />
          </div>

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
    </>
  );
}

// ─── Coalesce outcomes ──────────────────────────────────────────────

function CoalesceOutcomes({ runId }: { runId: string }) {
  const { data, loading } = useQuery(ADMIN_COALESCE_RUN_OUTCOMES, {
    variables: { runId },
  });
  const outcomes = data?.adminCoalesceRunOutcomes;

  if (loading) return <p className="text-muted-foreground text-sm">Loading outcomes...</p>;
  if (!outcomes) return null;

  return (
    <div className="space-y-6">
      {/* Summary stats */}
      <div className="grid grid-cols-2 md:grid-cols-3 gap-4">
        <StatCard label="Groups Created" value={outcomes.groupsCreated.total} />
        <StatCard label="Signals Grouped" value={outcomes.signalsGrouped.total} />
        <StatCard label="Groups Refined" value={outcomes.groupsRefined.total} />
      </div>

      {/* Groups Created */}
      {outcomes.groupsCreated.total > 0 && (
        <div className="space-y-2">
          <SectionHeader title="Groups Created" total={outcomes.groupsCreated.total} />
          <div className="space-y-3">
            {outcomes.groupsCreated.items.map((g: { groupId: string; label: string; queries: string[]; seedSignalId: string | null; memberCount: number }) => (
              <Link key={g.groupId} to={`/clusters/${g.groupId}`} className="block rounded-lg border border-border p-4 space-y-2 hover:border-blue-500/40 transition-colors">
                <div className="flex items-center justify-between">
                  <h3 className="text-sm font-medium">{g.label}</h3>
                  <span className="text-xs text-muted-foreground tabular-nums">{g.memberCount} signals</span>
                </div>
                {g.seedSignalId && (
                  <p className="text-xs text-muted-foreground">
                    Seed: <Link to={`/signals/${g.seedSignalId}`} className="text-blue-400 hover:underline font-mono" onClick={(e) => e.stopPropagation()}>{g.seedSignalId.slice(0, 8)}</Link>
                  </p>
                )}
                <div className="flex flex-wrap gap-1.5">
                  {g.queries.map((q, i) => (
                    <span key={i} className="text-xs px-2 py-0.5 rounded-full bg-muted text-muted-foreground">{q}</span>
                  ))}
                </div>
              </Link>
            ))}
          </div>
          <ShowingCount shown={outcomes.groupsCreated.items.length} total={outcomes.groupsCreated.total} />
        </div>
      )}

      {/* Signals Grouped */}
      {outcomes.signalsGrouped.total > 0 && (
        <div className="space-y-2">
          <SectionHeader title="Signals Grouped" total={outcomes.signalsGrouped.total} />
          <OutcomeTable
            columns={[
              { key: "signalTitle", label: "Signal", render: (v: string, row: { signalId: string }) => (
                <Link to={`/signals/${row.signalId}`} className="text-blue-400 hover:underline">{v ?? "untitled"}</Link>
              )},
              { key: "signalType", label: "Type", render: (v: string) => v ? (
                <span className={`px-2 py-0.5 rounded-full text-xs border ${SIGNAL_TYPE_COLORS[v] ?? "bg-muted text-muted-foreground border-border"}`}>{v}</span>
              ) : null },
              { key: "sourceUrl", label: "Source", render: (v: string) => v ? (
                <ExternalLink href={v}>{truncate(v, 40)}</ExternalLink>
              ) : null },
              { key: "groupLabel", label: "Group" },
              { key: "confidence", label: "Confidence", render: (v: number) => `${(v * 100).toFixed(0)}%` },
            ]}
            rows={outcomes.signalsGrouped.items}
          />
          <ShowingCount shown={outcomes.signalsGrouped.items.length} total={outcomes.signalsGrouped.total} />
        </div>
      )}

      {/* Groups Refined */}
      {outcomes.groupsRefined.total > 0 && (
        <div className="space-y-2">
          <SectionHeader title="Groups Refined" total={outcomes.groupsRefined.total} />
          <div className="space-y-3">
            {outcomes.groupsRefined.items.map((g: { groupId: string; queries: string[]; groupLabel: string | null }) => (
              <Link key={g.groupId} to={`/clusters/${g.groupId}`} className="block rounded-lg border border-border p-4 space-y-2 hover:border-blue-500/40 transition-colors">
                <h3 className="text-sm font-medium">{g.groupLabel ?? g.groupId.slice(0, 8)}</h3>
                <div className="flex flex-wrap gap-1.5">
                  {g.queries.map((q, i) => (
                    <span key={i} className="text-xs px-2 py-0.5 rounded-full bg-blue-500/10 text-blue-400">{q}</span>
                  ))}
                </div>
              </Link>
            ))}
          </div>
          <ShowingCount shown={outcomes.groupsRefined.items.length} total={outcomes.groupsRefined.total} />
        </div>
      )}
    </div>
  );
}

// ─── Main detail page ───────────────────────────────────────────────

export function ScoutRunDetailPage() {
  const { runId } = useParams<{ runId: string }>();
  const [investigating, setInvestigating] = useState(false);

  const { data, loading } = useQuery(ADMIN_SCOUT_RUN, {
    variables: { runId: runId ?? "" },
    skip: !runId,
  });

  const run = data?.adminScoutRun;

  if (loading) return <p className="text-muted-foreground">Loading run...</p>;
  if (!run) return <p className="text-muted-foreground">Run not found.</p>;

  const isCoalesce = run.flowType === "coalesce";

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
          <Link to="/workflows" className="text-muted-foreground hover:text-foreground text-sm">Workflows</Link>
          <span className="text-muted-foreground">/</span>
          {run.parentRunId && (
            <>
              <Link to={`/workflows/${run.parentRunId}`} className="text-blue-400 hover:underline font-mono text-sm">{run.parentRunId.slice(0, 8)}</Link>
              <span className="text-muted-foreground">/</span>
            </>
          )}
          <h1 className="text-sm font-semibold font-mono">{run.runId.slice(0, 8)}</h1>
          {run.flowType && (
            <span className="text-xs px-2 py-0.5 rounded border border-border bg-muted">{run.flowType}</span>
          )}
          {run.status && (
            <span className={`text-xs px-2 py-0.5 rounded border ${
              run.status === "running" ? "bg-amber-500/10 text-amber-400 border-amber-500/30" :
              run.status === "completed" ? "bg-green-500/10 text-green-400 border-green-500/30" :
              run.status === "failed" ? "bg-red-500/10 text-red-400 border-red-500/30" :
              run.status === "cancelled" ? "bg-muted text-muted-foreground border-border" :
              "bg-blue-500/10 text-blue-400 border-blue-500/30"
            }`}>{run.status}</span>
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

      {/* Timestamps + error */}
      <div className="flex flex-wrap gap-6 text-xs text-muted-foreground">
        <span>Started: {formatTs(run.startedAt)}</span>
        {run.finishedAt && <span>Finished: {formatTs(run.finishedAt)}</span>}
        <span>Duration: {duration}</span>
        <span>Region: {run.region}</span>
        {run.scheduleId && <span>Schedule: <span className="font-mono">{run.scheduleId.slice(0, 8)}</span></span>}
      </div>
      {run.error && (
        <div className="text-xs px-3 py-2 rounded border border-red-500/30 bg-red-500/5 text-red-400">{run.error}</div>
      )}

      {/* Child runs (chain) */}
      {run.childRuns?.length > 0 && (
        <div className="space-y-1">
          <h2 className="text-xs font-semibold text-muted-foreground">Chain</h2>
          <div className="flex gap-2">
            {run.childRuns.map((child: { runId: string; flowType: string; status: string }) => (
              <Link
                key={child.runId}
                to={`/workflows/${child.runId}`}
                className="text-xs px-3 py-1.5 rounded border border-border hover:bg-muted flex items-center gap-2"
              >
                <span className="text-blue-400 font-mono">{child.runId.slice(0, 8)}</span>
                <span className="px-1.5 py-0.5 rounded bg-blue-500/10 text-blue-400">{child.flowType}</span>
                <span className={
                  child.status === "completed" ? "text-green-400" :
                  child.status === "running" ? "text-amber-400" :
                  child.status === "failed" ? "text-red-400" :
                  "text-muted-foreground"
                }>{child.status}</span>
              </Link>
            ))}
          </div>
        </div>
      )}

      {/* Flow-type-specific outcomes */}
      {isCoalesce ? (
        <CoalesceOutcomes runId={run.runId} />
      ) : (
        <ScoutOutcomes runId={run.runId} stats={run.stats} />
      )}

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
