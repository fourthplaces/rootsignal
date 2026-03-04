import { useState } from "react";
import { Link, useSearchParams } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import {
  ADMIN_SCOUT_TASKS,
  SUPERVISOR_FINDINGS,
  SUPERVISOR_SUMMARY,
} from "@/graphql/queries";
import {
  RUN_SCOUT,
  CREATE_SCOUT_TASK,
  CANCEL_SCOUT_TASK,
  DISMISS_FINDING,
  STOP_SCOUT,
} from "@/graphql/mutations";

type Tab = "tasks" | "findings";
const TABS: { key: Tab; label: string }[] = [
  { key: "tasks", label: "Tasks" },
  { key: "findings", label: "Findings" },
];

type ScoutTask = {
  id: string;
  centerLat: number;
  centerLng: number;
  radiusKm: number;
  context: string;
  geoTerms: string[];
  priority: number;
  source: string;
  status: string;
  phaseStatus: string;
  createdAt: string;
  completedAt: string | null;
};

type ScoutFinding = {
  id: string;
  issueType: string;
  severity: string;
  targetId: string;
  targetLabel: string;
  description: string;
  suggestedAction: string;
  status: string;
  createdAt: string;
  resolvedAt: string | null;
};

const SEVERITY_COLORS: Record<string, string> = {
  error: "bg-red-500/10 text-red-400 border-red-500/20",
  warning: "bg-amber-500/10 text-amber-400 border-amber-500/20",
  info: "bg-blue-500/10 text-blue-400 border-blue-500/20",
};

/** Human-readable label for a phase status string. */
function phaseStatusLabel(status: string): string {
  const labels: Record<string, string> = {
    idle: "Idle",
    running_bootstrap: "Bootstrap",
    bootstrap_complete: "Bootstrap Done",
    running_scrape: "Scrape",
    scrape_complete: "Scrape Done",
    running_synthesis: "Synthesis",
    synthesis_complete: "Synthesis Done",
    running_situation_weaver: "Situation Weaver",
    situation_weaver_complete: "Situation Weaver Done",
    running_supervisor: "Supervisor",
    complete: "Complete",
  };
  return labels[status] || status;
}

const PHASE_STEPS = [
  { key: "bootstrap", label: "Bootstrap" },
  { key: "scrape", label: "Scrape" },
  { key: "situation_weaver", label: "Weaving" },
  { key: "supervisor", label: "Supervisor" },
] as const;

/** Map a phaseStatus string to the index of the active step (0-based), or -1 if not running. */
function activeStepIndex(phaseStatus: string): number {
  if (phaseStatus === "running_bootstrap") return 0;
  if (phaseStatus === "running_scrape" || phaseStatus === "running_synthesis") return 1;
  if (phaseStatus === "running_situation_weaver") return 2;
  if (phaseStatus === "running_supervisor") return 3;
  return -1;
}

function PhaseProgress({ phaseStatus }: { phaseStatus: string }) {
  const idx = activeStepIndex(phaseStatus);
  if (idx < 0) return null;

  return (
    <div className="flex items-center gap-1 text-xs">
      {PHASE_STEPS.map((step, i) => (
        <span key={step.key} className="flex items-center gap-1">
          {i > 0 && <span className="text-muted-foreground/40">&rarr;</span>}
          <span
            className={
              i < idx
                ? "text-muted-foreground line-through"
                : i === idx
                  ? "text-green-400 font-medium"
                  : "text-muted-foreground/40"
            }
          >
            {step.label}
          </span>
        </span>
      ))}
    </div>
  );
}

const formatDate = (d: string) =>
  new Date(d).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type MutationFn = (options?: any) => Promise<any>;

function TaskRow({
  task: t,
  runScout,
  stopScout,
  onCancel,
  onRefetch,
}: {
  task: ScoutTask;
  runScout: MutationFn;
  stopScout: MutationFn;
  onCancel: (id: string) => void;
  onRefetch: () => void;
}) {
  const [running, setRunning] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isRunning = t.phaseStatus.startsWith("running_");

  const handleRun = async () => {
    setRunning(true);
    setError(null);
    try {
      await runScout({ variables: { taskId: t.id } });
      onRefetch();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to run");
    } finally {
      setRunning(false);
    }
  };

  const handleCancelRun = async () => {
    setCancelling(true);
    setError(null);
    try {
      await stopScout({ variables: { taskId: t.id } });
      onRefetch();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to cancel");
    } finally {
      setCancelling(false);
    }
  };

  return (
    <tr className="border-b border-border last:border-0 hover:bg-muted/30">
      <td className="px-4 py-2 max-w-[200px] truncate">
        <Link to={`/scout/tasks/${t.id}`} className="text-blue-400 hover:underline">
          {t.context}
        </Link>
      </td>
      <td className="px-4 py-2 text-muted-foreground text-xs font-mono">
        {t.centerLat.toFixed(3)}, {t.centerLng.toFixed(3)}
      </td>
      <td className="px-4 py-2 text-right tabular-nums">{t.radiusKm}km</td>
      <td className="px-4 py-2 text-right tabular-nums">{t.priority}</td>
      <td className="px-4 py-2 text-muted-foreground">{t.source}</td>
      <td className="px-4 py-2">
        {isRunning ? (
          <PhaseProgress phaseStatus={t.phaseStatus} />
        ) : (
          <span
            className={`text-xs px-2 py-0.5 rounded-full w-fit ${
              t.status === "pending"
                ? "bg-amber-500/10 text-amber-400"
                : t.phaseStatus === "complete"
                  ? "bg-green-500/10 text-green-400"
                  : t.status === "cancelled"
                    ? "bg-red-500/10 text-red-400"
                    : "bg-secondary text-muted-foreground"
            }`}
          >
            {t.phaseStatus === "complete" ? "Complete" : phaseStatusLabel(t.phaseStatus) || t.status}
          </span>
        )}
      </td>
      <td className="px-4 py-2 text-muted-foreground whitespace-nowrap">
        {formatDate(t.createdAt)}
      </td>
      <td className="px-4 py-2 text-right">
        <div className="flex gap-1 justify-end items-center">
          {t.status === "pending" && !isRunning && (
            <>
              <button
                onClick={handleRun}
                disabled={running}
                className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50 disabled:opacity-50"
              >
                {running ? "Starting..." : "Run"}
              </button>
              <button
                onClick={() => onCancel(t.id)}
                className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50"
              >
                Cancel
              </button>
            </>
          )}
          {isRunning && (
            <button
              onClick={handleCancelRun}
              disabled={cancelling}
              className="text-xs px-2 py-1 rounded border border-red-500/30 text-red-400 hover:text-red-300 hover:bg-red-500/10 disabled:opacity-50"
            >
              {cancelling ? "Cancelling..." : "Cancel"}
            </button>
          )}
        </div>
        {error && <p className="text-xs text-red-400 mt-1">{error}</p>}
      </td>
    </tr>
  );
}

function ScoutFindingRow({
  finding: f,
  dismissFinding,
  onRefetch,
}: {
  finding: ScoutFinding;
  dismissFinding: MutationFn;
  onRefetch: () => void;
}) {
  const [dismissing, setDismissing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleDismiss = async () => {
    setDismissing(true);
    setError(null);
    try {
      await dismissFinding({ variables: { id: f.id } });
      onRefetch();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to dismiss");
    } finally {
      setDismissing(false);
    }
  };

  return (
    <tr className="border-b border-border last:border-0 hover:bg-muted/30">
      <td className="px-4 py-2">
        <span className={`inline-block px-2 py-0.5 rounded text-xs border ${SEVERITY_COLORS[f.severity] ?? "bg-muted text-muted-foreground"}`}>
          {f.severity}
        </span>
      </td>
      <td className="px-4 py-2 text-muted-foreground">{f.issueType}</td>
      <td className="px-4 py-2"><span className="font-medium">{f.targetLabel}</span></td>
      <td className="px-4 py-2 max-w-md truncate text-muted-foreground">{f.description}</td>
      <td className="px-4 py-2 text-muted-foreground whitespace-nowrap">{formatDate(f.createdAt)}</td>
      <td className="px-4 py-2">
        <span className={`text-xs ${
          f.status === "open" ? "text-amber-400"
            : f.status === "resolved" ? "text-green-400"
            : "text-muted-foreground"
        }`}>
          {f.status}
        </span>
      </td>
      <td className="px-4 py-2 text-right">
        {f.status === "open" && (
          <button
            onClick={handleDismiss}
            disabled={dismissing}
            className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50 transition-colors disabled:opacity-50"
          >
            {dismissing ? "Dismissing..." : "Dismiss"}
          </button>
        )}
        {error && <p className="text-xs text-red-400 mt-1">{error}</p>}
      </td>
    </tr>
  );
}

export function ScoutPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const rawTab = searchParams.get("tab");
  const tab: Tab = (rawTab && TABS.some((t) => t.key === rawTab) ? rawTab : "tasks") as Tab;
  const setTab = (t: Tab) => setSearchParams({ tab: t }, { replace: false });

  // --- Tasks ---
  const { data: tasksData, loading: tasksLoading, refetch: refetchTasks } = useQuery(
    ADMIN_SCOUT_TASKS,
    { variables: { limit: 50 }, skip: tab !== "tasks" },
  );
  const tasks: ScoutTask[] = tasksData?.adminScoutTasks ?? [];
  const [createTask] = useMutation(CREATE_SCOUT_TASK);
  const [cancelTask] = useMutation(CANCEL_SCOUT_TASK);
  const [taskLocation, setTaskLocation] = useState("");
  const [taskCreating, setTaskCreating] = useState(false);
  const [taskError, setTaskError] = useState<string | null>(null);

  const handleCreateTask = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!taskLocation.trim()) return;
    setTaskCreating(true);
    setTaskError(null);
    try {
      await createTask({
        variables: { location: taskLocation.trim() },
      });
      setTaskLocation("");
      refetchTasks();
    } catch (err: unknown) {
      setTaskError(err instanceof Error ? err.message : "Failed to create task");
    } finally {
      setTaskCreating(false);
    }
  };

  const handleCancelTask = async (id: string) => {
    await cancelTask({ variables: { id } });
    refetchTasks();
  };

  // --- Task actions ---
  const [runScout] = useMutation(RUN_SCOUT);
  const [stopScout] = useMutation(STOP_SCOUT);

  // --- Findings ---
  const region = "twincities";
  const [findingsStatusFilter, setFindingsStatusFilter] = useState<string | undefined>(undefined);
  const [findingsSeverityFilter, setFindingsSeverityFilter] = useState<string | undefined>(undefined);
  const [findingsTypeFilter, setFindingsTypeFilter] = useState<string | undefined>(undefined);
  const { data: findingsSummaryData, refetch: refetchFindingsSummary } = useQuery(
    SUPERVISOR_SUMMARY,
    { variables: { region }, skip: tab !== "findings" },
  );
  const { data: findingsData, loading: findingsLoading, refetch: refetchFindings } = useQuery(
    SUPERVISOR_FINDINGS,
    { variables: { region, status: findingsStatusFilter, limit: 200 }, skip: tab !== "findings" },
  );
  const [dismissFinding] = useMutation(DISMISS_FINDING);

  const findingsSummary = findingsSummaryData?.supervisorSummary as
    | { totalOpen: number; totalResolved: number; totalDismissed: number }
    | undefined;
  const findings: ScoutFinding[] = findingsData?.supervisorFindings ?? [];

  const filteredFindings = findings.filter((f) => {
    if (findingsSeverityFilter && f.severity !== findingsSeverityFilter) return false;
    if (findingsTypeFilter && f.issueType !== findingsTypeFilter) return false;
    return true;
  });

  const findingsIssueTypes = [...new Set(findings.map((f) => f.issueType))].sort();

  const refetchAllFindings = () => {
    refetchFindings();
    refetchFindingsSummary();
  };

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-xl font-semibold">Scout</h1>
      </div>

      {/* Tabs */}
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

      {/* Tasks tab */}
      {tab === "tasks" && (
        <div>
          <form onSubmit={handleCreateTask} className="mb-4 flex gap-2 items-center">
            <input
              type="text"
              value={taskLocation}
              onChange={(e) => { setTaskLocation(e.target.value); setTaskError(null); }}
              placeholder="Location (e.g. Austin, TX)"
              className="flex-1 max-w-xs px-3 py-1.5 rounded-md border border-input bg-background text-sm"
              required
            />
            <button
              type="submit"
              disabled={taskCreating || !taskLocation.trim()}
              className="px-3 py-1.5 rounded-md bg-primary text-primary-foreground text-sm hover:bg-primary/90 disabled:opacity-50"
            >
              {taskCreating ? "Creating..." : "Create Task"}
            </button>
            {taskError && (
              <span className="text-sm text-red-400">{taskError}</span>
            )}
          </form>

          {tasksLoading ? (
            <p className="text-muted-foreground">Loading tasks...</p>
          ) : tasks.length === 0 ? (
            <p className="text-muted-foreground">No scout tasks.</p>
          ) : (
            <div className="rounded-lg border border-border overflow-hidden">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-border bg-muted/50">
                    <th className="text-left px-4 py-2 font-medium">Context</th>
                    <th className="text-left px-4 py-2 font-medium">Center</th>
                    <th className="text-right px-4 py-2 font-medium">Radius</th>
                    <th className="text-right px-4 py-2 font-medium">Priority</th>
                    <th className="text-left px-4 py-2 font-medium">Source</th>
                    <th className="text-left px-4 py-2 font-medium">Status</th>
                    <th className="text-left px-4 py-2 font-medium">Created</th>
                    <th className="text-right px-4 py-2 font-medium"></th>
                  </tr>
                </thead>
                <tbody>
                  {tasks.map((t) => (
                    <TaskRow
                      key={t.id}
                      task={t}
                      runScout={runScout}
                      stopScout={stopScout}
                      onCancel={handleCancelTask}
                      onRefetch={refetchTasks}
                    />
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      {/* Findings tab */}
      {tab === "findings" && (
        <div className="space-y-4">
          {/* Summary cards */}
          {findingsSummary && (
            <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
              {[
                { label: "Open", value: findingsSummary.totalOpen },
                { label: "Resolved", value: findingsSummary.totalResolved },
                { label: "Dismissed", value: findingsSummary.totalDismissed },
                { label: "Total", value: findingsSummary.totalOpen + findingsSummary.totalResolved + findingsSummary.totalDismissed },
              ].map((stat) => (
                <div key={stat.label} className="rounded-lg border border-border p-4">
                  <p className="text-xs text-muted-foreground">{stat.label}</p>
                  <p className="text-2xl font-semibold mt-1">{stat.value}</p>
                </div>
              ))}
            </div>
          )}

          {/* Filters */}
          <div className="flex gap-3">
            <select
              value={findingsStatusFilter ?? ""}
              onChange={(e) => setFindingsStatusFilter(e.target.value || undefined)}
              className="px-3 py-1.5 rounded-md border border-input bg-background text-sm"
            >
              <option value="">All statuses</option>
              <option value="open">Open</option>
              <option value="resolved">Resolved</option>
              <option value="dismissed">Dismissed</option>
            </select>
            <select
              value={findingsSeverityFilter ?? ""}
              onChange={(e) => setFindingsSeverityFilter(e.target.value || undefined)}
              className="px-3 py-1.5 rounded-md border border-input bg-background text-sm"
            >
              <option value="">All severities</option>
              <option value="error">Error</option>
              <option value="warning">Warning</option>
              <option value="info">Info</option>
            </select>
            <select
              value={findingsTypeFilter ?? ""}
              onChange={(e) => setFindingsTypeFilter(e.target.value || undefined)}
              className="px-3 py-1.5 rounded-md border border-input bg-background text-sm"
            >
              <option value="">All types</option>
              {findingsIssueTypes.map((t) => (
                <option key={t} value={t}>{t}</option>
              ))}
            </select>
          </div>

          {/* Findings table */}
          {findingsLoading ? (
            <p className="text-muted-foreground">Loading findings...</p>
          ) : filteredFindings.length === 0 ? (
            <p className="text-muted-foreground">No findings match the current filters.</p>
          ) : (
            <div className="rounded-lg border border-border overflow-hidden">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-border bg-muted/50">
                    <th className="text-left px-4 py-2 font-medium">Severity</th>
                    <th className="text-left px-4 py-2 font-medium">Type</th>
                    <th className="text-left px-4 py-2 font-medium">Target</th>
                    <th className="text-left px-4 py-2 font-medium">Description</th>
                    <th className="text-left px-4 py-2 font-medium">Created</th>
                    <th className="text-left px-4 py-2 font-medium">Status</th>
                    <th className="text-right px-4 py-2 font-medium">Action</th>
                  </tr>
                </thead>
                <tbody>
                  {filteredFindings.map((f) => (
                    <ScoutFindingRow
                      key={f.id}
                      finding={f}
                      dismissFinding={dismissFinding}
                      onRefetch={refetchAllFindings}
                    />
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

    </div>
  );
}
