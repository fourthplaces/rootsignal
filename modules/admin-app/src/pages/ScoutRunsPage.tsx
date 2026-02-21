import { useState } from "react";
import { useQuery } from "@apollo/client";
import { Link } from "react-router";
import { ADMIN_SCOUT_RUNS, ADMIN_REGIONS } from "@/graphql/queries";

type ScoutRunStats = {
  urlsScraped: number;
  urlsUnchanged: number;
  urlsFailed: number;
  signalsExtracted: number;
  signalsDeduplicated: number;
  signalsStored: number;
  socialMediaPosts: number;
  expansionQueriesCollected: number;
  expansionSourcesCreated: number;
};

type ScoutRun = {
  runId: string;
  region: string;
  startedAt: string;
  finishedAt: string;
  stats: ScoutRunStats;
};

export function ScoutRunsPage() {
  const [region, setRegion] = useState("twincities");
  const { data: regionsData } = useQuery(ADMIN_REGIONS);
  const { data, loading } = useQuery(ADMIN_SCOUT_RUNS, {
    variables: { region, limit: 50 },
  });

  const runs: ScoutRun[] = data?.adminScoutRuns ?? [];

  const formatDate = (d: string) =>
    new Date(d).toLocaleDateString("en-US", {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });

  const duration = (start: string, end: string) => {
    const ms = new Date(end).getTime() - new Date(start).getTime();
    const secs = Math.round(ms / 1000);
    if (secs < 60) return `${secs}s`;
    const mins = Math.floor(secs / 60);
    const remainSecs = secs % 60;
    return `${mins}m ${remainSecs}s`;
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Scout Runs</h1>
        <select
          value={region}
          onChange={(e) => setRegion(e.target.value)}
          className="px-3 py-1.5 rounded-md border border-input bg-background text-sm"
        >
          {regionsData?.adminRegions?.map(
            (r: { slug: string; name: string }) => (
              <option key={r.slug} value={r.slug}>
                {r.name}
              </option>
            ),
          )}
        </select>
      </div>

      {loading ? (
        <p className="text-muted-foreground">Loading runs...</p>
      ) : runs.length === 0 ? (
        <p className="text-muted-foreground">
          No scout runs found for this region.
        </p>
      ) : (
        <div className="rounded-lg border border-border overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border bg-muted/50">
                <th className="text-left px-4 py-2 font-medium">Date</th>
                <th className="text-left px-4 py-2 font-medium">Run ID</th>
                <th className="text-left px-4 py-2 font-medium">Duration</th>
                <th className="text-right px-4 py-2 font-medium">URLs</th>
                <th className="text-right px-4 py-2 font-medium">Extracted</th>
                <th className="text-right px-4 py-2 font-medium">Stored</th>
                <th className="text-right px-4 py-2 font-medium">Deduped</th>
                <th className="text-right px-4 py-2 font-medium">Social</th>
              </tr>
            </thead>
            <tbody>
              {runs.map((run) => (
                <tr
                  key={run.runId}
                  className="border-b border-border last:border-0 hover:bg-muted/30"
                >
                  <td className="px-4 py-2 text-muted-foreground whitespace-nowrap">
                    {formatDate(run.startedAt)}
                  </td>
                  <td className="px-4 py-2">
                    <Link
                      to={`/scout-runs/${run.runId}`}
                      className="text-blue-400 hover:underline font-mono text-xs"
                    >
                      {run.runId.slice(0, 8)}
                    </Link>
                  </td>
                  <td className="px-4 py-2 text-muted-foreground">
                    {duration(run.startedAt, run.finishedAt)}
                  </td>
                  <td className="px-4 py-2 text-right tabular-nums">
                    {run.stats.urlsScraped}
                  </td>
                  <td className="px-4 py-2 text-right tabular-nums">
                    {run.stats.signalsExtracted}
                  </td>
                  <td className="px-4 py-2 text-right tabular-nums font-medium">
                    {run.stats.signalsStored}
                  </td>
                  <td className="px-4 py-2 text-right tabular-nums text-muted-foreground">
                    {run.stats.signalsDeduplicated}
                  </td>
                  <td className="px-4 py-2 text-right tabular-nums text-muted-foreground">
                    {run.stats.socialMediaPosts}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
