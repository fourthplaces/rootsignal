import { useState, useCallback } from "react";
import { Link } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import {
  ADMIN_SCOUT_RUNS,
  ADMIN_REGION_SOURCES,
  ADMIN_SCOUT_TASKS,
} from "@/graphql/queries";
import {
  ADD_SOURCE,
  RUN_SCOUT,
  STOP_SCOUT,
  RESET_SCOUT_LOCK,
  CREATE_SCOUT_TASK,
  CANCEL_SCOUT_TASK,
} from "@/graphql/mutations";
import { RegionMap } from "./MapPage";

const MAPBOX_TOKEN = import.meta.env.VITE_MAPBOX_TOKEN ?? "";
const DEFAULT_SCOPE = { centerLat: 44.9778, centerLng: -93.265, radiusKm: 30 };

type Tab = "runs" | "sources" | "map" | "tasks" | "controls";
const TABS: { key: Tab; label: string }[] = [
  { key: "runs", label: "Runs" },
  { key: "sources", label: "Sources" },
  { key: "map", label: "Map" },
  { key: "tasks", label: "Tasks" },
  { key: "controls", label: "Controls" },
];

type GeocodedPlace = {
  name: string;
  lat: number;
  lng: number;
  geoTerms: string[];
};

async function geocodeCity(query: string): Promise<GeocodedPlace | null> {
  const url = `https://api.mapbox.com/geocoding/v5/mapbox.places/${encodeURIComponent(query)}.json?types=place,locality,region&limit=1&access_token=${MAPBOX_TOKEN}`;
  const res = await fetch(url);
  if (!res.ok) return null;
  const data = await res.json();
  const feature = data.features?.[0];
  if (!feature) return null;
  const [lng, lat] = feature.center;
  const terms: string[] = [];
  if (feature.text) terms.push(feature.text);
  if (feature.context) {
    for (const c of feature.context) {
      if (c.id?.startsWith("region") || c.id?.startsWith("place")) {
        terms.push(c.text);
      }
    }
  }
  return { name: feature.place_name ?? query, lat, lng, geoTerms: terms };
}

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

const duration = (start: string, end: string) => {
  const ms = new Date(end).getTime() - new Date(start).getTime();
  const secs = Math.round(ms / 1000);
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  return `${mins}m ${secs % 60}s`;
};

export function ScoutPage() {
  const [tab, setTab] = useState<Tab>("runs");

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
  const [cityInput, setCityInput] = useState("");
  const [geocoded, setGeocoded] = useState<GeocodedPlace | null>(null);
  const [geocoding, setGeocoding] = useState(false);
  const [geocodeError, setGeocodeError] = useState<string | null>(null);
  const [taskRadiusKm, setTaskRadiusKm] = useState("30");
  const [taskPriority, setTaskPriority] = useState("1.0");

  const handleGeocode = useCallback(async () => {
    if (!cityInput.trim()) return;
    setGeocoding(true);
    setGeocodeError(null);
    const result = await geocodeCity(cityInput.trim());
    setGeocoding(false);
    if (result) {
      setGeocoded(result);
    } else {
      setGeocodeError("Could not find that location.");
      setGeocoded(null);
    }
  }, [cityInput]);

  const handleCreateTask = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!geocoded) return;
    await createTask({
      variables: {
        centerLat: geocoded.lat,
        centerLng: geocoded.lng,
        radiusKm: parseFloat(taskRadiusKm),
        context: geocoded.name,
        geoTerms: geocoded.geoTerms.length > 0 ? geocoded.geoTerms : undefined,
        priority: parseFloat(taskPriority),
      },
    });
    setShowCreateTask(false);
    setCityInput("");
    setGeocoded(null);
    setTaskRadiusKm("30");
    setTaskPriority("1.0");
    refetchTasks();
  };

  const handleCancelTask = async (id: string) => {
    await cancelTask({ variables: { id } });
    refetchTasks();
  };

  // --- Controls ---
  const [runScout] = useMutation(RUN_SCOUT);
  const [stopScout] = useMutation(STOP_SCOUT);
  const [resetLock] = useMutation(RESET_SCOUT_LOCK);
  const [scoutQuery, setScoutQuery] = useState("");
  const [controlMsg, setControlMsg] = useState<string | null>(null);

  const handleControl = async (action: "run" | "stop" | "reset") => {
    setControlMsg(null);
    let result;
    if (action === "run") result = await runScout({ variables: { query: scoutQuery } });
    else if (action === "stop") result = await stopScout();
    else result = await resetLock({ variables: { query: scoutQuery } });
    const msg =
      result.data?.runScout?.message ??
      result.data?.stopScout?.message ??
      result.data?.resetScoutLock?.message ??
      "Done";
    setControlMsg(msg);
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

      {/* Map tab */}
      {tab === "map" && <RegionMap region={DEFAULT_SCOPE} />}

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
            <form onSubmit={handleCreateTask} className="mb-4 space-y-3 max-w-lg">
              <div className="flex gap-2">
                <input
                  type="text"
                  value={cityInput}
                  onChange={(e) => { setCityInput(e.target.value); setGeocoded(null); setGeocodeError(null); }}
                  onKeyDown={(e) => { if (e.key === "Enter" && !geocoded) { e.preventDefault(); handleGeocode(); } }}
                  placeholder="City (e.g. Austin, TX)"
                  className="flex-1 px-3 py-2 rounded-md border border-input bg-background text-sm"
                  required
                />
                <button
                  type="button"
                  onClick={handleGeocode}
                  disabled={geocoding || !cityInput.trim()}
                  className="px-3 py-2 rounded-md border border-border text-sm hover:bg-accent disabled:opacity-50"
                >
                  {geocoding ? "Looking up..." : "Look up"}
                </button>
              </div>

              {geocodeError && (
                <p className="text-sm text-red-400">{geocodeError}</p>
              )}

              {geocoded && (
                <>
                  <div className="rounded-md border border-border bg-muted/30 px-3 py-2 text-sm">
                    <p className="font-medium">{geocoded.name}</p>
                    <p className="text-muted-foreground text-xs">
                      {geocoded.lat.toFixed(4)}, {geocoded.lng.toFixed(4)}
                      {geocoded.geoTerms.length > 0 && (
                        <> &middot; {geocoded.geoTerms.join(", ")}</>
                      )}
                    </p>
                  </div>
                  <div className="grid grid-cols-2 gap-2">
                    <div>
                      <label className="text-xs text-muted-foreground">Radius (km)</label>
                      <input
                        type="number"
                        step="any"
                        value={taskRadiusKm}
                        onChange={(e) => setTaskRadiusKm(e.target.value)}
                        className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm"
                      />
                    </div>
                    <div>
                      <label className="text-xs text-muted-foreground">Priority</label>
                      <input
                        type="number"
                        step="0.1"
                        value={taskPriority}
                        onChange={(e) => setTaskPriority(e.target.value)}
                        className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm"
                      />
                    </div>
                  </div>
                  <button
                    type="submit"
                    className="px-4 py-2 rounded-md bg-primary text-primary-foreground text-sm hover:bg-primary/90"
                  >
                    Create Task
                  </button>
                </>
              )}
            </form>
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
                      <td className="px-4 py-2 max-w-[200px] truncate">{t.context}</td>
                      <td className="px-4 py-2 text-muted-foreground text-xs font-mono">
                        {t.centerLat.toFixed(3)}, {t.centerLng.toFixed(3)}
                      </td>
                      <td className="px-4 py-2 text-right tabular-nums">{t.radiusKm}km</td>
                      <td className="px-4 py-2 text-right tabular-nums">{t.priority}</td>
                      <td className="px-4 py-2 text-muted-foreground">{t.source}</td>
                      <td className="px-4 py-2">
                        <span
                          className={`text-xs px-2 py-0.5 rounded-full ${
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
                      </td>
                      <td className="px-4 py-2 text-muted-foreground whitespace-nowrap">
                        {formatDate(t.createdAt)}
                      </td>
                      <td className="px-4 py-2 text-right">
                        {(t.status === "pending" || t.status === "running") && (
                          <button
                            onClick={() => handleCancelTask(t.id)}
                            className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50"
                          >
                            Cancel
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

      {/* Controls tab */}
      {tab === "controls" && (
        <div className="space-y-4 max-w-md">
          <div>
            <label className="text-xs text-muted-foreground">Where should we scout?</label>
            <input
              type="text"
              value={scoutQuery}
              onChange={(e) => setScoutQuery(e.target.value)}
              placeholder="Austin, TX"
              className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm"
            />
          </div>
          <div className="flex gap-3">
            <button
              onClick={() => handleControl("run")}
              disabled={!scoutQuery.trim()}
              className="px-4 py-2 rounded-md bg-primary text-primary-foreground text-sm hover:bg-primary/90 disabled:opacity-50"
            >
              Run Scout
            </button>
            <button
              onClick={() => handleControl("stop")}
              className="px-4 py-2 rounded-md border border-border text-sm hover:bg-accent"
            >
              Stop Scout
            </button>
            <button
              onClick={() => handleControl("reset")}
              disabled={!scoutQuery.trim()}
              className="px-4 py-2 rounded-md border border-border text-sm text-muted-foreground hover:bg-accent disabled:opacity-50"
            >
              Reset Lock
            </button>
          </div>
          {controlMsg && (
            <p className="text-sm text-muted-foreground">{controlMsg}</p>
          )}
        </div>
      )}
    </div>
  );
}
