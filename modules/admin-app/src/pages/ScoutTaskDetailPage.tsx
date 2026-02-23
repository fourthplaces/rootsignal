import { useState } from "react";
import { useParams, Link, useSearchParams } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import { ADMIN_SCOUT_TASKS, SIGNALS_NEAR, SITUATIONS_IN_BOUNDS, ACTORS_IN_BOUNDS } from "@/graphql/queries";
import { RUN_SCOUT, RUN_SCOUT_PHASE } from "@/graphql/mutations";
import { RegionMap, type MapSignal } from "@/pages/MapPage";

type Tab = "map" | "signals" | "situations" | "actors";
const TABS: { key: Tab; label: string }[] = [
  { key: "map", label: "Map" },
  { key: "signals", label: "Signals" },
  { key: "situations", label: "Situations" },
  { key: "actors", label: "Actors" },
];

type Signal = {
  __typename: string;
  id: string;
  title: string;
  summary: string;
  confidence: number;
  extractedAt: string;
  contentDate: string | null;
  locationName: string | null;
  sourceUrl: string | null;
  causeHeat: number | null;
  location: { lat: number; lng: number } | null;
  actors: { id: string; name: string; actorType: string }[];
};

type Situation = {
  id: string;
  headline: string;
  arc: string;
  temperature: number;
  signalCount: number;
  locationName: string | null;
  firstSeen: string;
  lastUpdated: string;
};

type Actor = {
  id: string;
  name: string;
  actorType: string;
  signalCount: number;
  locationName: string | null;
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
  | "SCRAPE"
  | "SYNTHESIS"
  | "SITUATION_WEAVER"
  | "SUPERVISOR";

const PHASES: { value: ScoutPhaseValue; label: string }[] = [
  { value: "FULL_RUN", label: "Full Run" },
  { value: "BOOTSTRAP", label: "Bootstrap" },
  { value: "SCRAPE", label: "Scrape" },
  { value: "SYNTHESIS", label: "Synthesis" },
  { value: "SITUATION_WEAVER", label: "Situation Weaver" },
  { value: "SUPERVISOR", label: "Supervisor" },
];

function phaseEnabled(phase: ScoutPhaseValue, status: string): boolean {
  if (status.startsWith("running_")) return false;
  if (phase === "FULL_RUN") return true;

  switch (phase) {
    case "BOOTSTRAP":
      return true;
    case "SCRAPE":
      return [
        "bootstrap_complete", "scrape_complete", "synthesis_complete",
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

const formatDate = (d: string) =>
  new Date(d).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });

const signalTypeName = (typename: string) =>
  typename.replace("Gql", "").replace("Signal", "");

const typeColor: Record<string, string> = {
  Gathering: "bg-blue-500/10 text-blue-400",
  Aid: "bg-green-500/10 text-green-400",
  Need: "bg-orange-500/10 text-orange-400",
  Notice: "bg-purple-500/10 text-purple-400",
  Tension: "bg-red-500/10 text-red-400",
};

const arcColor: Record<string, string> = {
  EMERGING: "bg-blue-500/20 text-blue-300",
  DEVELOPING: "bg-green-500/20 text-green-300",
  ACTIVE: "bg-orange-500/20 text-orange-300",
  COOLING: "bg-gray-500/20 text-gray-300",
  COLD: "bg-gray-500/20 text-gray-500",
};

/** Convert center + radius to a bounding box. */
function toBounds(lat: number, lng: number, radiusKm: number) {
  const latDelta = radiusKm / 111.0;
  const lngDelta = radiusKm / (111.0 * Math.cos((lat * Math.PI) / 180));
  return {
    minLat: lat - latDelta,
    maxLat: lat + latDelta,
    minLng: lng - lngDelta,
    maxLng: lng + lngDelta,
  };
}

export function ScoutTaskDetailPage() {
  const { id } = useParams<{ id: string }>();
  const [searchParams, setSearchParams] = useSearchParams();
  const rawTab = searchParams.get("tab");
  const tab: Tab = (rawTab && TABS.some((t) => t.key === rawTab) ? rawTab : "map") as Tab;
  const setTab = (t: Tab) => setSearchParams({ tab: t }, { replace: false });
  const [selectedPhase, setSelectedPhase] = useState<ScoutPhaseValue>("FULL_RUN");

  // Fetch all tasks, find the one matching our ID
  const { data: tasksData, loading: taskLoading, refetch: refetchTasks } = useQuery(ADMIN_SCOUT_TASKS, {
    variables: { limit: 200 },
  });
  const task: ScoutTask | undefined = (tasksData?.adminScoutTasks ?? []).find(
    (t: ScoutTask) => t.id === id,
  );

  const [runScout] = useMutation(RUN_SCOUT);
  const [runScoutPhase] = useMutation(RUN_SCOUT_PHASE);
  const [runLoading, setRunLoading] = useState(false);
  const [runError, setRunError] = useState<string | null>(null);

  const handleRunPhase = async () => {
    if (!task) return;
    setRunLoading(true);
    setRunError(null);
    try {
      if (selectedPhase === "FULL_RUN") {
        await runScout({ variables: { taskId: task.id } });
      } else {
        await runScoutPhase({ variables: { phase: selectedPhase, taskId: task.id } });
      }
      refetchTasks();
    } catch (err: unknown) {
      setRunError(err instanceof Error ? err.message : "Failed to run phase");
    } finally {
      setRunLoading(false);
    }
  };

  // Fetch signals near the task's center
  const { data: signalsData, loading: signalsLoading } = useQuery(SIGNALS_NEAR, {
    variables: task
      ? { lat: task.centerLat, lng: task.centerLng, radiusKm: task.radiusKm }
      : undefined,
    skip: !task,
  });
  const signals: Signal[] = signalsData?.signalsNear ?? [];

  // Fetch situations in the task's bounding box
  const bounds = task ? toBounds(task.centerLat, task.centerLng, task.radiusKm) : null;
  const { data: situationsData, loading: situationsLoading } = useQuery(SITUATIONS_IN_BOUNDS, {
    variables: bounds ? { ...bounds, limit: 50 } : undefined,
    skip: !bounds,
  });
  const situations: Situation[] = situationsData?.situationsInBounds ?? [];

  // Fetch actors in the task's bounding box
  const { data: actorsData, loading: actorsLoading } = useQuery(ACTORS_IN_BOUNDS, {
    variables: bounds ? { ...bounds, limit: 100 } : undefined,
    skip: !bounds,
  });
  const actors: Actor[] = actorsData?.actorsInBounds ?? [];

  if (taskLoading) return <p className="text-muted-foreground">Loading...</p>;
  if (!task) return <p className="text-muted-foreground">Task not found</p>;

  const statusColor =
    task.status === "pending"
      ? "bg-amber-500/10 text-amber-400"
      : task.status === "running"
        ? "bg-green-900 text-green-300"
        : task.status === "completed"
          ? "bg-secondary text-muted-foreground"
          : "bg-red-500/10 text-red-400";

  return (
    <div className="space-y-6">
      {/* Header */}
      <div>
        <p className="text-sm text-muted-foreground mb-1">
          <Link to="/scout" className="hover:text-foreground">Scout</Link>
          {" / Tasks / "}
        </p>
        <div className="flex items-center gap-3">
          <h1 className="text-xl font-semibold">{task.context}</h1>
          <span className={`text-xs px-2 py-0.5 rounded-full ${statusColor}`}>
            {task.status}
          </span>
          {task.status !== "completed" && (
            <div className="flex gap-1 items-center ml-auto">
              <select
                value={selectedPhase}
                onChange={(e) => setSelectedPhase(e.target.value as ScoutPhaseValue)}
                className="text-xs px-1 py-1 rounded border border-border bg-background text-muted-foreground"
              >
                {PHASES.map((p) => (
                  <option
                    key={p.value}
                    value={p.value}
                    disabled={!phaseEnabled(p.value, task.phaseStatus)}
                  >
                    {p.label}
                  </option>
                ))}
              </select>
              <button
                onClick={handleRunPhase}
                disabled={runLoading}
                className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50 disabled:opacity-50"
              >
                {runLoading ? "Running..." : "Run"}
              </button>
            </div>
          )}
          {runError && (
            <p className="text-xs text-red-400 mt-1">{runError}</p>
          )}
        </div>
      </div>

      {/* Task metadata */}
      <div className="grid grid-cols-2 sm:grid-cols-4 gap-4 text-sm">
        <div>
          <p className="text-muted-foreground">Center</p>
          <p className="font-mono text-xs">
            {task.centerLat.toFixed(4)}, {task.centerLng.toFixed(4)}
          </p>
        </div>
        <div>
          <p className="text-muted-foreground">Radius</p>
          <p>{task.radiusKm} km</p>
        </div>
        <div>
          <p className="text-muted-foreground">Source</p>
          <p>{task.source}</p>
        </div>
        <div>
          <p className="text-muted-foreground">Priority</p>
          <p>{task.priority}</p>
        </div>
        <div>
          <p className="text-muted-foreground">Created</p>
          <p>{formatDate(task.createdAt)}</p>
        </div>
        {task.completedAt && (
          <div>
            <p className="text-muted-foreground">Completed</p>
            <p>{formatDate(task.completedAt)}</p>
          </div>
        )}
        {task.geoTerms.length > 0 && (
          <div className="col-span-2">
            <p className="text-muted-foreground">Geo Terms</p>
            <div className="flex flex-wrap gap-1 mt-1">
              {task.geoTerms.map((t) => (
                <span key={t} className="text-xs px-2 py-0.5 rounded bg-secondary">
                  {t}
                </span>
              ))}
            </div>
          </div>
        )}
      </div>

      {/* Summary counts */}
      <div className="flex gap-6 text-sm">
        <span>
          <span className="text-muted-foreground">Signals:</span>{" "}
          <span className="font-medium">{signals.length}</span>
        </span>
        <span>
          <span className="text-muted-foreground">Situations:</span>{" "}
          <span className="font-medium">{situations.length}</span>
        </span>
        <span>
          <span className="text-muted-foreground">Actors:</span>{" "}
          <span className="font-medium">{actors.length}</span>
        </span>
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

      {/* Map tab */}
      {tab === "map" && (
        <RegionMap
          region={{
            centerLat: task.centerLat,
            centerLng: task.centerLng,
            radiusKm: task.radiusKm,
          }}
          signals={signals as MapSignal[]}
        />
      )}

      {/* Signals tab */}
      {tab === "signals" && (
        signalsLoading ? (
          <p className="text-muted-foreground">Loading signals...</p>
        ) : signals.length === 0 ? (
          <p className="text-muted-foreground">No signals found in this area.</p>
        ) : (
          <div className="rounded-lg border border-border overflow-hidden">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border bg-muted/50">
                  <th className="text-left px-4 py-2 font-medium">Type</th>
                  <th className="text-left px-4 py-2 font-medium">Title</th>
                  <th className="text-left px-4 py-2 font-medium">Location</th>
                  <th className="text-right px-4 py-2 font-medium">Confidence</th>
                  <th className="text-right px-4 py-2 font-medium">Heat</th>
                  <th className="text-left px-4 py-2 font-medium">Published</th>
                  <th className="text-left px-4 py-2 font-medium">Extracted</th>
                </tr>
              </thead>
              <tbody>
                {signals.map((s) => {
                  const typeName = signalTypeName(s.__typename);
                  return (
                    <tr key={s.id} className="border-b border-border last:border-0 hover:bg-muted/30">
                      <td className="px-4 py-2">
                        <span className={`text-xs px-2 py-0.5 rounded-full ${typeColor[typeName] ?? "bg-secondary"}`}>
                          {typeName}
                        </span>
                      </td>
                      <td className="px-4 py-2 max-w-[300px]">
                        <Link
                          to={`/signals/${s.id}`}
                          className="text-blue-400 hover:underline"
                        >
                          {s.title}
                        </Link>
                      </td>
                      <td className="px-4 py-2 text-muted-foreground truncate max-w-[150px]">
                        {s.locationName ?? "—"}
                      </td>
                      <td className="px-4 py-2 text-right tabular-nums">
                        {(s.confidence * 100).toFixed(0)}%
                      </td>
                      <td className="px-4 py-2 text-right tabular-nums">
                        {s.causeHeat != null ? s.causeHeat.toFixed(1) : "—"}
                      </td>
                      <td className="px-4 py-2 text-muted-foreground whitespace-nowrap">
                        {s.contentDate ? formatDate(s.contentDate) : "—"}
                      </td>
                      <td className="px-4 py-2 text-muted-foreground whitespace-nowrap">
                        {formatDate(s.extractedAt)}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )
      )}

      {/* Situations tab */}
      {tab === "situations" && (
        situationsLoading ? (
          <p className="text-muted-foreground">Loading situations...</p>
        ) : situations.length === 0 ? (
          <p className="text-muted-foreground">No situations found in this area.</p>
        ) : (
          <div className="rounded-lg border border-border overflow-hidden">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border bg-muted/50">
                  <th className="text-left px-4 py-2 font-medium">Headline</th>
                  <th className="text-left px-4 py-2 font-medium">Arc</th>
                  <th className="text-left px-4 py-2 font-medium">Location</th>
                  <th className="text-right px-4 py-2 font-medium">Temp</th>
                  <th className="text-right px-4 py-2 font-medium">Signals</th>
                  <th className="text-left px-4 py-2 font-medium">Last Updated</th>
                </tr>
              </thead>
              <tbody>
                {situations.map((s) => (
                  <tr key={s.id} className="border-b border-border last:border-0 hover:bg-muted/30">
                    <td className="px-4 py-2 max-w-[300px]">
                      <Link
                        to={`/situations/${s.id}`}
                        className="text-blue-400 hover:underline"
                      >
                        {s.headline}
                      </Link>
                    </td>
                    <td className="px-4 py-2">
                      {s.arc && (
                        <span className={`text-xs px-2 py-0.5 rounded-full ${arcColor[s.arc] ?? "bg-secondary"}`}>
                          {s.arc}
                        </span>
                      )}
                    </td>
                    <td className="px-4 py-2 text-muted-foreground truncate max-w-[150px]">
                      {s.locationName ?? "—"}
                    </td>
                    <td className="px-4 py-2 text-right tabular-nums">{s.temperature.toFixed(1)}</td>
                    <td className="px-4 py-2 text-right tabular-nums">{s.signalCount}</td>
                    <td className="px-4 py-2 text-muted-foreground whitespace-nowrap">
                      {formatDate(s.lastUpdated)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )
      )}

      {/* Actors tab */}
      {tab === "actors" && (
        actorsLoading ? (
          <p className="text-muted-foreground">Loading actors...</p>
        ) : actors.length === 0 ? (
          <p className="text-muted-foreground">No actors found in this area.</p>
        ) : (
          <div className="rounded-lg border border-border overflow-hidden">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border bg-muted/50">
                  <th className="text-left px-4 py-2 font-medium">Name</th>
                  <th className="text-left px-4 py-2 font-medium">Type</th>
                  <th className="text-left px-4 py-2 font-medium">Location</th>
                  <th className="text-right px-4 py-2 font-medium">Signals</th>
                </tr>
              </thead>
              <tbody>
                {actors.map((a) => (
                  <tr key={a.id} className="border-b border-border last:border-0 hover:bg-muted/30">
                    <td className="px-4 py-2">{a.name}</td>
                    <td className="px-4 py-2 text-muted-foreground">{a.actorType}</td>
                    <td className="px-4 py-2 text-muted-foreground">{a.locationName ?? "—"}</td>
                    <td className="px-4 py-2 text-right tabular-nums">{a.signalCount}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )
      )}
    </div>
  );
}
