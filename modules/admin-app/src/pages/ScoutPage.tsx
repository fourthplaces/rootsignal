import { useState } from "react";
import { Link, useSearchParams } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import {
  ADMIN_REGIONS,
  SUPERVISOR_FINDINGS,
  SUPERVISOR_SUMMARY,
  ADMIN_SCOUT_RUNS,
} from "@/graphql/queries";
import { useRegion } from "@/contexts/RegionContext";
import {
  CREATE_REGION,
  DELETE_REGION,
  RUN_SCRAPE,
  RUN_BOOTSTRAP,
  RUN_WEAVE,
  CANCEL_RUN,
  DISMISS_FINDING,
} from "@/graphql/mutations";
import { SourcesPage } from "@/pages/SourcesPage";
import { DataTable, type Column } from "@/components/DataTable";

type Tab = "runs" | "regions" | "sources" | "findings";
const TABS: { key: Tab; label: string }[] = [
  { key: "runs", label: "Runs" },
  { key: "regions", label: "Regions" },
  { key: "sources", label: "Sources" },
  { key: "findings", label: "Findings" },
];

type Region = {
  id: string;
  name: string;
  centerLat: number;
  centerLng: number;
  radiusKm: number;
  geoTerms: string[];
  isLeaf: boolean;
  createdAt: string;
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

type ScoutRun = {
  runId: string;
  region: string;
  flowType: string | null;
  startedAt: string;
  finishedAt: string | null;
};

const SEVERITY_COLORS: Record<string, string> = {
  error: "bg-red-500/10 text-red-400 border-red-500/20",
  warning: "bg-amber-500/10 text-amber-400 border-amber-500/20",
  info: "bg-blue-500/10 text-blue-400 border-blue-500/20",
};

const formatDate = (d: string) =>
  new Date(d).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type MutationFn = (options?: any) => Promise<any>;

function RegionActions({ region: r, onDelete, onRefetch }: { region: Region; onDelete: (id: string) => void; onRefetch: () => void }) {
  const [runScrape] = useMutation(RUN_SCRAPE);
  const [runBootstrap] = useMutation(RUN_BOOTSTRAP);
  const [runWeave] = useMutation(RUN_WEAVE);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const runFlow = async (mutation: MutationFn, label: string) => {
    setBusy(label);
    setError(null);
    try {
      await mutation({ variables: { regionId: r.id } });
      onRefetch();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : `Failed to ${label}`);
    } finally {
      setBusy(null);
    }
  };

  return (
    <div>
      <div className="flex gap-1 justify-end items-center flex-wrap">
        <button onClick={() => runFlow(runBootstrap, "bootstrap")} disabled={busy !== null} className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50 disabled:opacity-50">
          {busy === "bootstrap" ? "..." : "Bootstrap"}
        </button>
        <button onClick={() => runFlow(runScrape, "scrape")} disabled={busy !== null} className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50 disabled:opacity-50">
          {busy === "scrape" ? "..." : "Scrape"}
        </button>
        <button onClick={() => runFlow(runWeave, "weave")} disabled={busy !== null} className="text-xs px-2 py-1 rounded border border-blue-500/30 text-blue-400 hover:bg-blue-500/10 disabled:opacity-50">
          {busy === "weave" ? "..." : "Weave"}
        </button>
        <button onClick={() => onDelete(r.id)} className="text-xs px-2 py-1 rounded border border-red-500/30 text-red-400 hover:bg-red-500/10">
          Delete
        </button>
      </div>
      {error && <p className="text-xs text-red-400 mt-1">{error}</p>}
    </div>
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
  const tab: Tab = (rawTab && TABS.some((t) => t.key === rawTab) ? rawTab : "runs") as Tab;
  const setTab = (t: Tab) => setSearchParams({ tab: t }, { replace: false });

  // --- Regions ---
  const { data: regionsData, loading: regionsLoading, refetch: refetchRegions } = useQuery(
    ADMIN_REGIONS,
    { variables: { limit: 100 }, skip: tab !== "regions" },
  );
  const regions: Region[] = regionsData?.adminRegions ?? [];
  const [createRegion] = useMutation(CREATE_REGION);
  const [deleteRegion] = useMutation(DELETE_REGION);

  // Create region form state
  const [showCreate, setShowCreate] = useState(false);
  const [formName, setFormName] = useState("");
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);

  const handleCreate = async (e: React.FormEvent) => {
    e.preventDefault();
    setCreating(true);
    setCreateError(null);
    try {
      await createRegion({
        variables: { name: formName.trim() },
      });
      setFormName("");
      setShowCreate(false);
      refetchRegions();
    } catch (err: unknown) {
      setCreateError(err instanceof Error ? err.message : "Failed to create region");
    } finally {
      setCreating(false);
    }
  };

  const handleDelete = async (id: string) => {
    if (!confirm("Delete this region?")) return;
    await deleteRegion({ variables: { id } });
    refetchRegions();
  };

  // --- Runs ---
  const { data: runsData, loading: runsLoading } = useQuery(ADMIN_SCOUT_RUNS, {
    variables: { limit: 50 },
    skip: tab !== "runs",
  });
  const runs: ScoutRun[] = runsData?.adminScoutRuns ?? [];
  const [cancelRun] = useMutation(CANCEL_RUN);

  // --- Findings ---
  const { regionName } = useRegion();
  const region = regionName || "twincities";
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

  const runColumns: Column<ScoutRun>[] = [
    { key: "runId", label: "Run", render: (r) => (
      <Link to={`/scout-runs/${r.runId}`} className="text-blue-400 hover:underline font-mono text-xs">{r.runId.slice(0, 8)}</Link>
    )},
    { key: "region", label: "Region", render: (r) => <span className="text-muted-foreground">{r.region || "-"}</span> },
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

  const regionColumns: Column<Region>[] = [
    { key: "name", label: "Name", render: (r) => (
      <Link to={`/scout/regions/${r.id}`} className="text-blue-400 hover:underline font-medium">{r.name}</Link>
    )},
    { key: "center", label: "Center", render: (r) => <span className="text-muted-foreground text-xs font-mono">{r.centerLat.toFixed(3)}, {r.centerLng.toFixed(3)}</span> },
    { key: "radius", label: "Radius", align: "right" as const, render: (r) => <span className="tabular-nums">{r.radiusKm}km</span> },
    { key: "type", label: "Type", render: (r) => (
      <span className={`text-xs px-2 py-0.5 rounded-full ${r.isLeaf ? "bg-green-500/10 text-green-400" : "bg-blue-500/10 text-blue-400"}`}>{r.isLeaf ? "Leaf" : "Parent"}</span>
    )},
    { key: "geoTerms", label: "Geo Terms", render: (r) => <span className="text-muted-foreground text-xs">{r.geoTerms.length > 0 ? r.geoTerms.join(", ") : "-"}</span> },
    { key: "createdAt", label: "Created", render: (r) => <span className="text-muted-foreground whitespace-nowrap">{formatDate(r.createdAt)}</span> },
    { key: "actions", label: "Actions", align: "right" as const, render: (r) => (
      <RegionActions region={r} onDelete={handleDelete} onRefetch={refetchRegions} />
    )},
  ];

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
        <DataTable<ScoutRun>
          columns={runColumns}
          data={runs}
          getRowKey={(r) => r.runId}
          loading={runsLoading}
          emptyMessage="No runs yet."
        />
      )}

      {/* Regions tab */}
      {tab === "regions" && (
        <div className="space-y-4">
          <div className="flex gap-2 items-center">
            <button
              onClick={() => setShowCreate(!showCreate)}
              className="px-3 py-1.5 rounded-md bg-primary text-primary-foreground text-sm hover:bg-primary/90"
            >
              {showCreate ? "Cancel" : "Create Region"}
            </button>
          </div>

          {showCreate && (
            <form onSubmit={handleCreate} className="rounded-lg border border-border p-4 space-y-3">
              <div className="flex gap-3 items-center">
                <input
                  type="text"
                  value={formName}
                  onChange={(e) => setFormName(e.target.value)}
                  placeholder="Location (e.g. Minneapolis, Minnesota)"
                  className="px-3 py-1.5 rounded-md border border-input bg-background text-sm flex-1"
                  required
                />
                <button
                  type="submit"
                  disabled={creating}
                  className="px-3 py-1.5 rounded-md bg-primary text-primary-foreground text-sm hover:bg-primary/90 disabled:opacity-50"
                >
                  {creating ? "Creating..." : "Create"}
                </button>
              </div>
              {createError && <span className="text-sm text-red-400">{createError}</span>}
            </form>
          )}

          <DataTable<Region>
            columns={regionColumns}
            data={regions}
            getRowKey={(r) => r.id}
            loading={regionsLoading}
            emptyMessage="No regions configured."
          />

        </div>
      )}

      {/* Sources tab */}
      {tab === "sources" && <SourcesPage />}

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
