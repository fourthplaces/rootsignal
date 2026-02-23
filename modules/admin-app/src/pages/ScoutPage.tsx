import { useState } from "react";
import { Link, useSearchParams } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import {
  ADMIN_SCOUT_RUNS,
  ADMIN_REGION_SOURCES,
  ADMIN_SCOUT_TASKS,
  SUPERVISOR_FINDINGS,
  SUPERVISOR_SUMMARY,
} from "@/graphql/queries";
import {
  ADD_SOURCE,
  RUN_SCOUT,
  RUN_SCOUT_PHASE,
  RESET_SCOUT_STATUS,
  CREATE_SCOUT_TASK,
  CANCEL_SCOUT_TASK,
  DISMISS_FINDING,
} from "@/graphql/mutations";

type Tab = "tasks" | "runs" | "sources" | "findings";
const TABS: { key: Tab; label: string }[] = [
  { key: "tasks", label: "Tasks" },
  { key: "runs", label: "Runs" },
  { key: "sources", label: "Sources" },
  { key: "findings", label: "Findings" },
];

type ScoutRunStats = {
  urlsScraped: number;
  signalsExtracted: number;
  signalsDeduplicated: number;
  signalsStored: number;
  socialMediaPosts: number;
};

type ScoutRun = {
  runId: string;
  region: string;
  startedAt: string;
  finishedAt: string;
  stats: ScoutRunStats;
};

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

type ScoutPhaseValue =
  | "FULL_RUN"
  | "BOOTSTRAP"
  | "ACTOR_DISCOVERY"
  | "SCRAPE"
  | "SYNTHESIS"
  | "SITUATION_WEAVER"
  | "SUPERVISOR";

const PHASES: { value: ScoutPhaseValue; label: string }[] = [
  { value: "FULL_RUN", label: "Full Run" },
  { value: "BOOTSTRAP", label: "Bootstrap" },
  { value: "ACTOR_DISCOVERY", label: "Actor Discovery" },
  { value: "SCRAPE", label: "Scrape" },
  { value: "SYNTHESIS", label: "Synthesis" },
  { value: "SITUATION_WEAVER", label: "Situation Weaver" },
  { value: "SUPERVISOR", label: "Supervisor" },
];

/** Check if a phase can run given the current region status. Full Run is always allowed (unless running). */
function phaseEnabled(phase: ScoutPhaseValue, status: string): boolean {
  if (status.startsWith("running_")) return false;
  if (phase === "FULL_RUN") return true;

  switch (phase) {
    case "BOOTSTRAP":
      return true; // Always runnable when not running
    case "ACTOR_DISCOVERY":
      return [
        "bootstrap_complete", "actor_discovery_complete", "scrape_complete",
        "synthesis_complete", "situation_weaver_complete", "complete",
      ].includes(status);
    case "SCRAPE":
      return [
        "actor_discovery_complete", "scrape_complete", "synthesis_complete",
        "situation_weaver_complete", "complete",
      ].includes(status);
    case "SYNTHESIS":
      return [
        "scrape_complete", "synthesis_complete",
        "situation_weaver_complete", "complete",
      ].includes(status);
    case "SITUATION_WEAVER":
      return [
        "synthesis_complete", "situation_weaver_complete", "complete",
      ].includes(status);
    case "SUPERVISOR":
      return [
        "situation_weaver_complete", "complete",
      ].includes(status);
    default:
      return false;
  }
}

/** Human-readable label for a phase status string. */
function phaseStatusLabel(status: string): string {
  const labels: Record<string, string> = {
    idle: "Idle",
    running_bootstrap: "Running Bootstrap",
    bootstrap_complete: "Bootstrap Done",
    running_actor_discovery: "Running Actor Discovery",
    actor_discovery_complete: "Actor Discovery Done",
    running_scrape: "Running Scrape",
    scrape_complete: "Scrape Done",
    running_synthesis: "Running Synthesis",
    synthesis_complete: "Synthesis Done",
    running_situation_weaver: "Running Situation Weaver",
    situation_weaver_complete: "Situation Weaver Done",
    running_supervisor: "Running Supervisor",
    complete: "Complete",
  };
  return labels[status] || status;
}

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
  return `${mins}m ${secs % 60}s`;
};

export function ScoutPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const rawTab = searchParams.get("tab");
  const tab: Tab = (rawTab && TABS.some((t) => t.key === rawTab) ? rawTab : "tasks") as Tab;
  const setTab = (t: Tab) => setSearchParams({ tab: t }, { replace: false });

  // --- Runs ---
  const { data: runsData, loading: runsLoading } = useQuery(ADMIN_SCOUT_RUNS, {
    variables: { region: "", limit: 50 },
    skip: tab !== "runs",
  });
  const runs: ScoutRun[] = runsData?.adminScoutRuns ?? [];

  // --- Sources ---
  const { data: sourcesData, refetch: refetchSources } = useQuery(ADMIN_REGION_SOURCES, {
    variables: { regionSlug: "" },
    skip: tab !== "sources",
  });
  const sources = sourcesData?.adminRegionSources ?? [];
  const [addSource] = useMutation(ADD_SOURCE);
  const [showAddSource, setShowAddSource] = useState(false);
  const [sourceUrl, setSourceUrl] = useState("");
  const [sourceReason, setSourceReason] = useState("");

  const handleAddSource = async (e: React.FormEvent) => {
    e.preventDefault();
    await addSource({
      variables: { url: sourceUrl, reason: sourceReason || undefined },
    });
    setSourceUrl("");
    setSourceReason("");
    setShowAddSource(false);
    refetchSources();
  };

  // --- Tasks ---
  const { data: tasksData, loading: tasksLoading, refetch: refetchTasks } = useQuery(
    ADMIN_SCOUT_TASKS,
    { variables: { limit: 50 }, skip: tab !== "tasks" },
  );
  const tasks: ScoutTask[] = tasksData?.adminScoutTasks ?? [];
  const [createTask] = useMutation(CREATE_SCOUT_TASK);
  const [cancelTask] = useMutation(CANCEL_SCOUT_TASK);
  const [showCreateTask, setShowCreateTask] = useState(false);
  const [taskLocation, setTaskLocation] = useState("");
  const [taskCreating, setTaskCreating] = useState(false);
  const [taskError, setTaskError] = useState<string | null>(null);

  // Phase selection state per task
  const [selectedPhase, setSelectedPhase] = useState<Record<string, ScoutPhaseValue>>({});

  const handleCreateTask = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!taskLocation.trim()) return;
    setTaskCreating(true);
    setTaskError(null);
    try {
      await createTask({
        variables: { location: taskLocation.trim() },
      });
      setShowCreateTask(false);
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
  const [runScoutPhase] = useMutation(RUN_SCOUT_PHASE);
  const [resetStatus] = useMutation(RESET_SCOUT_STATUS);

  const handleRunPhase = async (context: string, phase: ScoutPhaseValue) => {
    if (phase === "FULL_RUN") {
      await runScout({ variables: { query: context } });
    } else {
      await runScoutPhase({ variables: { phase, query: context } });
    }
    refetchTasks();
  };

  const handleResetStatus = async (context: string) => {
    await resetStatus({ variables: { query: context } });
    refetchTasks();
  };

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

  type Finding = {
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

  const findingsSummary = findingsSummaryData?.supervisorSummary as
    | { totalOpen: number; totalResolved: number; totalDismissed: number }
    | undefined;
  const findings: Finding[] = findingsData?.supervisorFindings ?? [];

  const filteredFindings = findings.filter((f) => {
    if (findingsSeverityFilter && f.severity !== findingsSeverityFilter) return false;
    if (findingsTypeFilter && f.issueType !== findingsTypeFilter) return false;
    return true;
  });

  const findingsIssueTypes = [...new Set(findings.map((f) => f.issueType))].sort();

  const handleDismissFinding = async (id: string) => {
    await dismissFinding({ variables: { id } });
    refetchFindings();
    refetchFindingsSummary();
  };

  const SEVERITY_COLORS: Record<string, string> = {
    error: "bg-red-500/10 text-red-400 border-red-500/20",
    warning: "bg-amber-500/10 text-amber-400 border-amber-500/20",
    info: "bg-blue-500/10 text-blue-400 border-blue-500/20",
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

      {/* Runs tab */}
      {tab === "runs" && (
        runsLoading ? (
          <p className="text-muted-foreground">Loading runs...</p>
        ) : runs.length === 0 ? (
          <p className="text-muted-foreground">No scout runs found.</p>
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
                  <tr key={run.runId} className="border-b border-border last:border-0 hover:bg-muted/30">
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
                    <td className="px-4 py-2 text-right tabular-nums">{run.stats.urlsScraped}</td>
                    <td className="px-4 py-2 text-right tabular-nums">{run.stats.signalsExtracted}</td>
                    <td className="px-4 py-2 text-right tabular-nums font-medium">{run.stats.signalsStored}</td>
                    <td className="px-4 py-2 text-right tabular-nums text-muted-foreground">{run.stats.signalsDeduplicated}</td>
                    <td className="px-4 py-2 text-right tabular-nums text-muted-foreground">{run.stats.socialMediaPosts}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )
      )}

      {/* Sources tab */}
      {tab === "sources" && (
        <div>
          <div className="flex items-center justify-between mb-4">
            <h2 className="text-sm font-medium">Sources ({sources.length})</h2>
            <button
              onClick={() => setShowAddSource(!showAddSource)}
              className="px-3 py-1.5 rounded-md bg-primary text-primary-foreground text-sm hover:bg-primary/90"
            >
              Add Source
            </button>
          </div>

          {showAddSource && (
            <form onSubmit={handleAddSource} className="mb-4 space-y-2">
              <input
                type="url"
                value={sourceUrl}
                onChange={(e) => setSourceUrl(e.target.value)}
                placeholder="https://..."
                className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm"
                required
              />
              <input
                type="text"
                value={sourceReason}
                onChange={(e) => setSourceReason(e.target.value)}
                placeholder="Reason (optional)"
                className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm"
              />
              <button
                type="submit"
                className="px-4 py-2 rounded-md bg-primary text-primary-foreground text-sm hover:bg-primary/90"
              >
                Add
              </button>
            </form>
          )}

          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border text-left text-muted-foreground">
                  <th className="pb-2 font-medium">Source</th>
                  <th className="pb-2 font-medium">Type</th>
                  <th className="pb-2 font-medium">Weight</th>
                  <th className="pb-2 font-medium">Signals</th>
                  <th className="pb-2 font-medium">Cadence</th>
                  <th className="pb-2 font-medium">Last Scraped</th>
                </tr>
              </thead>
              <tbody>
                {sources.map(
                  (s: {
                    id: string;
                    canonicalValue: string;
                    sourceLabel: string;
                    effectiveWeight: number;
                    signalsProduced: number;
                    cadenceHours: number;
                    lastScraped: string | null;
                  }) => (
                    <tr key={s.id} className="border-b border-border/50">
                      <td className="py-2 truncate max-w-[200px]">{s.canonicalValue}</td>
                      <td className="py-2">{s.sourceLabel}</td>
                      <td className="py-2">{s.effectiveWeight.toFixed(2)}</td>
                      <td className="py-2">{s.signalsProduced}</td>
                      <td className="py-2">{s.cadenceHours}h</td>
                      <td className="py-2 text-muted-foreground">
                        {s.lastScraped ? new Date(s.lastScraped).toLocaleDateString() : "Never"}
                      </td>
                    </tr>
                  ),
                )}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* Tasks tab */}
      {tab === "tasks" && (
        <div>
          <div className="flex items-center justify-between mb-4">
            <h2 className="text-sm font-medium">Scout Tasks</h2>
            <button
              onClick={() => setShowCreateTask(!showCreateTask)}
              className="px-3 py-1.5 rounded-md bg-primary text-primary-foreground text-sm hover:bg-primary/90"
            >
              Create Task
            </button>
          </div>

          {showCreateTask && (
            <form onSubmit={handleCreateTask} className="mb-4 flex gap-2 max-w-lg">
              <input
                type="text"
                value={taskLocation}
                onChange={(e) => { setTaskLocation(e.target.value); setTaskError(null); }}
                placeholder="Location (e.g. Austin, TX)"
                className="flex-1 px-3 py-2 rounded-md border border-input bg-background text-sm"
                required
              />
              <button
                type="submit"
                disabled={taskCreating || !taskLocation.trim()}
                className="px-4 py-2 rounded-md bg-primary text-primary-foreground text-sm hover:bg-primary/90 disabled:opacity-50"
              >
                {taskCreating ? "Creating..." : "Create"}
              </button>
            </form>
          )}
          {taskError && (
            <p className="text-sm text-red-400 mb-4">{taskError}</p>
          )}

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
                    <tr key={t.id} className="border-b border-border last:border-0 hover:bg-muted/30">
                      <td className="px-4 py-2 max-w-[200px] truncate">
                        <Link
                          to={`/scout/tasks/${t.id}`}
                          className="text-blue-400 hover:underline"
                        >
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
                        <div className="flex flex-col gap-1">
                          <span
                            className={`text-xs px-2 py-0.5 rounded-full w-fit ${
                              t.status === "pending"
                                ? "bg-amber-500/10 text-amber-400"
                                : t.status === "running"
                                  ? "bg-green-900 text-green-300"
                                  : t.status === "completed"
                                    ? "bg-secondary text-muted-foreground"
                                    : "bg-red-500/10 text-red-400"
                            }`}
                          >
                            {t.status}
                          </span>
                          {t.phaseStatus !== "idle" && (
                            <span
                              className={`text-xs px-2 py-0.5 rounded-full w-fit ${
                                t.phaseStatus.startsWith("running_")
                                  ? "bg-green-900 text-green-300"
                                  : t.phaseStatus === "complete"
                                    ? "bg-secondary text-muted-foreground"
                                    : "bg-blue-500/10 text-blue-400"
                              }`}
                            >
                              {phaseStatusLabel(t.phaseStatus)}
                            </span>
                          )}
                        </div>
                      </td>
                      <td className="px-4 py-2 text-muted-foreground whitespace-nowrap">
                        {formatDate(t.createdAt)}
                      </td>
                      <td className="px-4 py-2 text-right">
                        <div className="flex gap-1 justify-end items-center">
                          {t.status === "pending" && (
                            <>
                              <select
                                value={selectedPhase[t.id] || "FULL_RUN"}
                                onChange={(e) =>
                                  setSelectedPhase({
                                    ...selectedPhase,
                                    [t.id]: e.target.value as ScoutPhaseValue,
                                  })
                                }
                                className="text-xs px-1 py-1 rounded border border-border bg-background text-muted-foreground"
                              >
                                {PHASES.map((p) => (
                                  <option
                                    key={p.value}
                                    value={p.value}
                                    disabled={!phaseEnabled(p.value, t.phaseStatus)}
                                  >
                                    {p.label}
                                  </option>
                                ))}
                              </select>
                              <button
                                onClick={() =>
                                  handleRunPhase(
                                    t.context,
                                    selectedPhase[t.id] || "FULL_RUN",
                                  )
                                }
                                className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50"
                              >
                                Run
                              </button>
                            </>
                          )}
                          {(t.status === "pending" || t.status === "running") && (
                            <>
                              <button
                                onClick={() => handleResetStatus(t.context)}
                                className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50"
                              >
                                Reset Status
                              </button>
                              <button
                                onClick={() => handleCancelTask(t.id)}
                                className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50"
                              >
                                Cancel
                              </button>
                            </>
                          )}
                        </div>
                      </td>
                    </tr>
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
                    <tr key={f.id} className="border-b border-border last:border-0 hover:bg-muted/30">
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
                            onClick={() => handleDismissFinding(f.id)}
                            className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50 transition-colors"
                          >
                            Dismiss
                          </button>
                        )}
                      </td>
                    </tr>
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
