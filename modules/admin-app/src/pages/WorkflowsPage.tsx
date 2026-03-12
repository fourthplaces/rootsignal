import { useSearchParams } from "react-router";
import { Link } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import { ADMIN_SCOUT_RUNS, ADMIN_SCHEDULED_SCRAPES } from "@/graphql/queries";
import { CANCEL_RUN } from "@/graphql/mutations";
import { DataTable, type Column } from "@/components/DataTable";

type Tab = "runs" | "scheduled";
const TABS: { key: Tab; label: string }[] = [
  { key: "runs", label: "Runs" },
  { key: "scheduled", label: "Scheduled" },
];

type ScoutRun = {
  runId: string;
  region: string;
  regionId: string | null;
  flowType: string | null;
  sources: { id: string; label: string }[];
  startedAt: string;
  finishedAt: string | null;
};

type ScheduledScrape = {
  id: string;
  scopeType: string;
  scopeData: string;
  runAfter: string;
  reason: string;
  createdAt: string;
  completedAt: string | null;
};

const formatDate = (d: string) =>
  new Date(d).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });

export function WorkflowsPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const rawTab = searchParams.get("tab");
  const tab: Tab = (rawTab && TABS.some((t) => t.key === rawTab) ? rawTab : "runs") as Tab;
  const setTab = (t: Tab) => setSearchParams({ tab: t }, { replace: false });

  const { data: runsData, loading: runsLoading } = useQuery(ADMIN_SCOUT_RUNS, {
    variables: { limit: 50 },
    skip: tab !== "runs",
  });
  const runs: ScoutRun[] = runsData?.adminRuns ?? [];
  const [cancelRun] = useMutation(CANCEL_RUN);

  const { data: scheduledData, loading: scheduledLoading } = useQuery(
    ADMIN_SCHEDULED_SCRAPES,
    { variables: { limit: 50 }, skip: tab !== "scheduled" },
  );
  const scheduled: ScheduledScrape[] = scheduledData?.adminScheduledScrapes ?? [];

  const runColumns: Column<ScoutRun>[] = [
    { key: "runId", label: "Run", render: (r) => (
      <Link to={`/workflows/${r.runId}`} className="text-blue-400 hover:underline font-mono text-xs">{r.runId.slice(0, 8)}</Link>
    )},
    { key: "area", label: "Area", render: (r) => {
      if (r.sources.length > 0) {
        return (
          <span className="flex flex-wrap gap-1.5">
            {r.sources.map((s) => (
              <Link key={s.id} to={`/sources/${s.id}`} className="text-blue-400 hover:underline text-xs">
                {s.label}
              </Link>
            ))}
          </span>
        );
      }
      if (r.regionId) {
        return <Link to={`/regions/${r.regionId}`} className="text-blue-400 hover:underline">{r.region}</Link>;
      }
      return <span className="text-muted-foreground">{r.region || "-"}</span>;
    }},
    { key: "flowType", label: "Flow", render: (r) => r.flowType ? (
      <span className="text-xs px-2 py-0.5 rounded-full bg-blue-500/10 text-blue-400">{r.flowType}</span>
    ) : null },
    { key: "startedAt", label: "Started", render: (r) => <span className="text-muted-foreground whitespace-nowrap">{formatDate(r.startedAt)}</span> },
    { key: "status", label: "Status", render: (r) => (
      <span className={`text-xs ${r.finishedAt ? "text-green-400" : "text-amber-400"}`}>{r.finishedAt ? "Completed" : "Running"}</span>
    )},
    { key: "actions", label: "", align: "right" as const, render: (r) => !r.finishedAt ? (
      <button
        onClick={() => cancelRun({ variables: { runId: r.runId } })}
        className="text-xs px-2 py-1 rounded border border-red-500/30 text-red-400 hover:bg-red-500/10"
      >
        Cancel
      </button>
    ) : null },
  ];

  const scheduledColumns: Column<ScheduledScrape>[] = [
    { key: "scopeType", label: "Scope", render: (s) => (
      <span className="text-xs px-2 py-0.5 rounded-full bg-blue-500/10 text-blue-400">{s.scopeType}</span>
    )},
    { key: "scopeData", label: "Target", render: (s) => {
      try {
        const data = JSON.parse(s.scopeData);
        if (Array.isArray(data)) return <span className="font-mono text-xs">{data.map((id: string) => id.slice(0, 8)).join(", ")}</span>;
        return <span className="text-xs">{String(data)}</span>;
      } catch {
        return <span className="text-xs text-muted-foreground">{s.scopeData}</span>;
      }
    }},
    { key: "reason", label: "Reason", render: (s) => <span className="text-muted-foreground">{s.reason}</span> },
    { key: "runAfter", label: "Run After", render: (s) => <span className="text-muted-foreground whitespace-nowrap">{formatDate(s.runAfter)}</span> },
    { key: "createdAt", label: "Created", render: (s) => <span className="text-muted-foreground whitespace-nowrap">{formatDate(s.createdAt)}</span> },
    { key: "status", label: "Status", render: (s) => (
      <span className={`text-xs ${s.completedAt ? "text-green-400" : new Date(s.runAfter) <= new Date() ? "text-amber-400" : "text-muted-foreground"}`}>
        {s.completedAt ? "Completed" : new Date(s.runAfter) <= new Date() ? "Due" : "Pending"}
      </span>
    )},
  ];

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-xl font-semibold">Workflows</h1>
      </div>

      <div className="flex gap-1 border-b border-border">
        {TABS.map((t) => (
          <button
            key={t.key}
            onClick={() => setTab(t.key)}
            className={`px-3 py-2 text-sm -mb-px transition-colors ${
              tab === t.key
                ? "border-b-2 border-foreground text-foreground"
                : "text-muted-foreground hover:text-foreground"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>

      {tab === "runs" && (
        <DataTable<ScoutRun>
          columns={runColumns}
          data={runs}
          getRowKey={(r) => r.runId}
          loading={runsLoading}
          emptyMessage="No runs yet."
        />
      )}

      {tab === "scheduled" && (
        <DataTable<ScheduledScrape>
          columns={scheduledColumns}
          data={scheduled}
          getRowKey={(s) => s.id}
          loading={scheduledLoading}
          emptyMessage="No scheduled workflows."
        />
      )}
    </div>
  );
}
