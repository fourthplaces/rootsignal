import { useState } from "react";
import { useSearchParams } from "react-router";
import { Link } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import { ADMIN_SCOUT_RUNS, ADMIN_SCHEDULED_SCRAPES, ADMIN_SCHEDULES } from "@/graphql/queries";
import { CANCEL_RUN, TOGGLE_SCHEDULE, DELETE_SCHEDULE } from "@/graphql/mutations";
import { DataTable, type Column } from "@/components/DataTable";
import { CreateScheduleDialog } from "@/components/CreateScheduleDialog";
import { formatCadence } from "@/lib/utils";

type Tab = "runs" | "schedules" | "scheduled";
const TABS: { key: Tab; label: string }[] = [
  { key: "runs", label: "Runs" },
  { key: "schedules", label: "Schedules" },
  { key: "scheduled", label: "One-Shot" },
];

type WorkflowRun = {
  runId: string;
  region: string;
  regionId: string | null;
  flowType: string | null;
  sources: { id: string; label: string }[];
  startedAt: string;
  finishedAt: string | null;
  status: string;
  error: string | null;
  cancelledAt: string | null;
  parentRunId: string | null;
  scheduleId: string | null;
};

type Schedule = {
  scheduleId: string;
  flowType: string;
  scope: string;
  cadenceSeconds: number;
  baseCadenceSeconds: number;
  recurring: boolean;
  enabled: boolean;
  lastRunId: string | null;
  nextRunAt: string | null;
  deletedAt: string | null;
  createdAt: string;
  regionId: string | null;
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


const STATUS_COLORS: Record<string, string> = {
  running: "text-amber-400",
  completed: "text-green-400",
  failed: "text-red-400",
  cancelled: "text-muted-foreground",
  scheduled: "text-blue-400",
};

export function WorkflowsPage() {
  const [showCreate, setShowCreate] = useState(false);
  const [searchParams, setSearchParams] = useSearchParams();
  const rawTab = searchParams.get("tab");
  const tab: Tab = (rawTab && TABS.some((t) => t.key === rawTab) ? rawTab : "runs") as Tab;
  const setTab = (t: Tab) => setSearchParams({ tab: t }, { replace: false });

  const { data: runsData, loading: runsLoading } = useQuery(ADMIN_SCOUT_RUNS, {
    variables: { limit: 50 },
    skip: tab !== "runs",
  });
  const runs: WorkflowRun[] = runsData?.adminRuns ?? [];
  const [cancelRun] = useMutation(CANCEL_RUN);

  const { data: schedulesData, loading: schedulesLoading, refetch: refetchSchedules } = useQuery(
    ADMIN_SCHEDULES,
    { variables: { limit: 50 }, skip: tab !== "schedules" },
  );
  const schedules: Schedule[] = schedulesData?.adminSchedules ?? [];
  const [toggleSchedule] = useMutation(TOGGLE_SCHEDULE);
  const [deleteSchedule] = useMutation(DELETE_SCHEDULE);

  const { data: scheduledData, loading: scheduledLoading } = useQuery(
    ADMIN_SCHEDULED_SCRAPES,
    { variables: { limit: 50 }, skip: tab !== "scheduled" },
  );
  const scheduled: ScheduledScrape[] = scheduledData?.adminScheduledScrapes ?? [];

  const runColumns: Column<WorkflowRun>[] = [
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
      <span className={`text-xs ${STATUS_COLORS[r.status] ?? "text-muted-foreground"}`}>
        {r.status.charAt(0).toUpperCase() + r.status.slice(1)}
      </span>
    )},
    { key: "chain", label: "Chain", render: (r) => r.parentRunId ? (
      <Link to={`/workflows/${r.parentRunId}`} className="text-blue-400 hover:underline font-mono text-xs" title="Parent run">
        {r.parentRunId.slice(0, 8)}
      </Link>
    ) : null },
    { key: "actions", label: "", align: "right" as const, render: (r) => r.status === "running" ? (
      <button
        onClick={() => cancelRun({ variables: { runId: r.runId } })}
        className="text-xs px-2 py-1 rounded border border-red-500/30 text-red-400 hover:bg-red-500/10"
      >
        Cancel
      </button>
    ) : null },
  ];

  const scheduleColumns: Column<Schedule>[] = [
    { key: "flowType", label: "Flow", render: (s) => (
      <span className="text-xs px-2 py-0.5 rounded-full bg-blue-500/10 text-blue-400">{s.flowType}</span>
    )},
    { key: "scope", label: "Scope", resizable: true, defaultWidth: 200, render: (s) => {
      try {
        const data = JSON.parse(s.scope);
        if (data.source_ids) {
          return <span className="font-mono text-xs">{data.source_ids.map((id: string) => id.slice(0, 8)).join(", ")}</span>;
        }
        return <span className="text-xs">{JSON.stringify(data)}</span>;
      } catch {
        return <span className="text-xs text-muted-foreground">{s.scope}</span>;
      }
    }},
    { key: "cadenceSeconds", label: "Cadence", render: (s) => (
      <span className="text-xs tabular-nums">{formatCadence(s.cadenceSeconds)}</span>
    )},
    { key: "enabled", label: "Status", render: (s) => (
      <span className={`text-xs ${s.enabled ? "text-green-400" : "text-muted-foreground"}`}>
        {s.enabled ? "Enabled" : "Disabled"}
      </span>
    )},
    { key: "nextRunAt", label: "Next Run", render: (s) => (
      <span className="text-muted-foreground whitespace-nowrap">
        {s.nextRunAt ? formatDate(s.nextRunAt) : "\u2014"}
      </span>
    )},
    { key: "lastRunId", label: "Last Run", render: (s) => s.lastRunId ? (
      <Link to={`/workflows/${s.lastRunId}`} className="text-blue-400 hover:underline font-mono text-xs">
        {s.lastRunId.slice(0, 8)}
      </Link>
    ) : <span className="text-muted-foreground">{"\u2014"}</span> },
    { key: "createdAt", label: "Created", render: (s) => (
      <span className="text-muted-foreground whitespace-nowrap">{formatDate(s.createdAt)}</span>
    )},
    { key: "actions", label: "", align: "right" as const, render: (s) => (
      <span className="flex gap-1">
        <button
          onClick={async () => {
            await toggleSchedule({ variables: { scheduleId: s.scheduleId, enabled: !s.enabled } });
            refetchSchedules();
          }}
          className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-muted"
        >
          {s.enabled ? "Disable" : "Enable"}
        </button>
        <button
          onClick={async () => {
            await deleteSchedule({ variables: { scheduleId: s.scheduleId } });
            refetchSchedules();
          }}
          className="text-xs px-2 py-1 rounded border border-red-500/30 text-red-400 hover:bg-red-500/10"
        >
          Delete
        </button>
      </span>
    )},
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
        <DataTable<WorkflowRun>
          columns={runColumns}
          data={runs}
          getRowKey={(r) => r.runId}
          loading={runsLoading}
          emptyMessage="No runs yet."
        />
      )}

      {tab === "schedules" && (
        <>
          <div className="flex justify-end">
            <button
              onClick={() => setShowCreate(true)}
              className="text-xs px-3 py-1.5 rounded-md bg-primary text-white hover:bg-primary/90"
            >
              New Schedule
            </button>
          </div>
          <DataTable<Schedule>
            columns={scheduleColumns}
            data={schedules}
            getRowKey={(s) => s.scheduleId}
            loading={schedulesLoading}
            emptyMessage="No schedules created yet."
          />
          {showCreate && (
            <CreateScheduleDialog onClose={() => { setShowCreate(false); refetchSchedules(); }} />
          )}
        </>
      )}

      {tab === "scheduled" && (
        <DataTable<ScheduledScrape>
          columns={scheduledColumns}
          data={scheduled}
          getRowKey={(s) => s.id}
          loading={scheduledLoading}
          emptyMessage="No one-shot scheduled workflows."
        />
      )}
    </div>
  );
}
