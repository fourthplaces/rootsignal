import { useState, useMemo } from "react";
import { useParams, Link } from "react-router";
import { useQuery } from "@apollo/client";
import { ADMIN_SCOUT_TASKS, SIGNALS_NEAR, STORIES_IN_BOUNDS } from "@/graphql/queries";
import { RegionMap } from "@/pages/MapPage";

type Tab = "map" | "signals" | "stories" | "actors";
const TABS: { key: Tab; label: string }[] = [
  { key: "map", label: "Map" },
  { key: "signals", label: "Signals" },
  { key: "stories", label: "Stories" },
  { key: "actors", label: "Actors" },
];

type Signal = {
  __typename: string;
  id: string;
  title: string;
  summary: string;
  confidence: number;
  extractedAt: string;
  locationName: string | null;
  sourceUrl: string | null;
  causeHeat: number | null;
  actors: { id: string; name: string; actorType: string }[];
};

type Story = {
  id: string;
  headline: string;
  summary: string;
  arc: string;
  category: string;
  energy: number;
  signalCount: number;
  firstSeen: string;
  status: string;
};

type Actor = {
  id: string;
  name: string;
  actorType: string;
  signalCount: number;
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

const signalTypeName = (typename: string) =>
  typename.replace("Gql", "").replace("Signal", "");

const typeColor: Record<string, string> = {
  Gathering: "bg-blue-500/10 text-blue-400",
  Aid: "bg-green-500/10 text-green-400",
  Need: "bg-orange-500/10 text-orange-400",
  Notice: "bg-purple-500/10 text-purple-400",
  Tension: "bg-red-500/10 text-red-400",
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
  const [tab, setTab] = useState<Tab>("map");

  // Fetch all tasks, find the one matching our ID
  const { data: tasksData, loading: taskLoading } = useQuery(ADMIN_SCOUT_TASKS, {
    variables: { limit: 200 },
  });
  const task: ScoutTask | undefined = (tasksData?.adminScoutTasks ?? []).find(
    (t: ScoutTask) => t.id === id,
  );

  // Fetch signals near the task's center
  const { data: signalsData, loading: signalsLoading } = useQuery(SIGNALS_NEAR, {
    variables: task
      ? { lat: task.centerLat, lng: task.centerLng, radiusKm: task.radiusKm }
      : undefined,
    skip: !task,
  });
  const signals: Signal[] = signalsData?.signalsNear ?? [];

  // Fetch stories in the task's bounding box
  const bounds = task ? toBounds(task.centerLat, task.centerLng, task.radiusKm) : null;
  const { data: storiesData, loading: storiesLoading } = useQuery(STORIES_IN_BOUNDS, {
    variables: bounds ? { ...bounds, limit: 50 } : undefined,
    skip: !bounds,
  });
  const stories: Story[] = storiesData?.storiesInBounds ?? [];

  // Deduplicate actors from signals
  const actors: Actor[] = useMemo(() => {
    const map = new Map<string, Actor>();
    for (const sig of signals) {
      for (const a of sig.actors ?? []) {
        const existing = map.get(a.id);
        if (existing) {
          existing.signalCount += 1;
        } else {
          map.set(a.id, { ...a, signalCount: 1 });
        }
      }
    }
    return Array.from(map.values()).sort((a, b) => b.signalCount - a.signalCount);
  }, [signals]);

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
          <span className="text-muted-foreground">Stories:</span>{" "}
          <span className="font-medium">{stories.length}</span>
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

      {/* Stories tab */}
      {tab === "stories" && (
        storiesLoading ? (
          <p className="text-muted-foreground">Loading stories...</p>
        ) : stories.length === 0 ? (
          <p className="text-muted-foreground">No stories found in this area.</p>
        ) : (
          <div className="rounded-lg border border-border overflow-hidden">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border bg-muted/50">
                  <th className="text-left px-4 py-2 font-medium">Headline</th>
                  <th className="text-left px-4 py-2 font-medium">Arc</th>
                  <th className="text-left px-4 py-2 font-medium">Category</th>
                  <th className="text-right px-4 py-2 font-medium">Energy</th>
                  <th className="text-right px-4 py-2 font-medium">Signals</th>
                  <th className="text-left px-4 py-2 font-medium">Status</th>
                  <th className="text-left px-4 py-2 font-medium">First Seen</th>
                </tr>
              </thead>
              <tbody>
                {stories.map((st) => (
                  <tr key={st.id} className="border-b border-border last:border-0 hover:bg-muted/30">
                    <td className="px-4 py-2 max-w-[300px]">
                      <Link
                        to={`/stories/${st.id}`}
                        className="text-blue-400 hover:underline"
                      >
                        {st.headline}
                      </Link>
                    </td>
                    <td className="px-4 py-2">
                      {st.arc && (
                        <span className="text-xs px-2 py-0.5 rounded-full bg-secondary">
                          {st.arc}
                        </span>
                      )}
                    </td>
                    <td className="px-4 py-2 text-muted-foreground">{st.category || "—"}</td>
                    <td className="px-4 py-2 text-right tabular-nums">{st.energy.toFixed(1)}</td>
                    <td className="px-4 py-2 text-right tabular-nums">{st.signalCount}</td>
                    <td className="px-4 py-2">
                      <span className={`text-xs px-2 py-0.5 rounded-full ${
                        st.status === "emerging"
                          ? "bg-amber-500/10 text-amber-400"
                          : "bg-secondary text-muted-foreground"
                      }`}>
                        {st.status}
                      </span>
                    </td>
                    <td className="px-4 py-2 text-muted-foreground whitespace-nowrap">
                      {formatDate(st.firstSeen)}
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
        signalsLoading ? (
          <p className="text-muted-foreground">Loading actors...</p>
        ) : actors.length === 0 ? (
          <p className="text-muted-foreground">No actors found in signals for this area.</p>
        ) : (
          <div className="rounded-lg border border-border overflow-hidden">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border bg-muted/50">
                  <th className="text-left px-4 py-2 font-medium">Name</th>
                  <th className="text-left px-4 py-2 font-medium">Type</th>
                  <th className="text-right px-4 py-2 font-medium">Mentions</th>
                </tr>
              </thead>
              <tbody>
                {actors.map((a) => (
                  <tr key={a.id} className="border-b border-border last:border-0 hover:bg-muted/30">
                    <td className="px-4 py-2">{a.name}</td>
                    <td className="px-4 py-2 text-muted-foreground">{a.actorType}</td>
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
