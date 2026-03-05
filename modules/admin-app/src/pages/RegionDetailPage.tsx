import { useState } from "react";
import { useParams, Link } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import {
  ADMIN_REGION,
  SIGNALS_NEAR,
  SITUATIONS_IN_BOUNDS,
  ACTORS_IN_BOUNDS,
  ADMIN_SCOUT_RUNS,
  ADMIN_REGION_SOURCES_BY_REGION,
} from "@/graphql/queries";
import {
  RUN_BOOTSTRAP,
  RUN_SCRAPE,
  RUN_WEAVE,
  RUN_SCOUT_SOURCE,
  CANCEL_RUN,
} from "@/graphql/mutations";

type Tab = "signals" | "situations" | "actors" | "sources" | "runs";
const TABS: { key: Tab; label: string }[] = [
  { key: "signals", label: "Signals" },
  { key: "situations", label: "Situations" },
  { key: "actors", label: "Actors" },
  { key: "sources", label: "Sources" },
  { key: "runs", label: "Runs" },
];

const formatDate = (d: string) =>
  new Date(d).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });

const typeColor: Record<string, string> = {
  Gathering: "bg-blue-500/10 text-blue-400",
  Resource: "bg-green-500/10 text-green-400",
  HelpRequest: "bg-red-500/10 text-red-400",
  Announcement: "bg-purple-500/10 text-purple-400",
  Concern: "bg-amber-500/10 text-amber-400",
  Condition: "bg-teal-500/10 text-teal-400",
};

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type MutationFn = (options?: any) => Promise<any>;

export function RegionDetailPage() {
  const { id } = useParams<{ id: string }>();
  const [tab, setTab] = useState<Tab>("signals");

  const { data: regionData, loading: regionLoading } = useQuery(ADMIN_REGION, {
    variables: { id },
    skip: !id,
  });
  const region = regionData?.adminRegion;

  // Spatial queries based on region bounds
  const { data: signalsData, loading: signalsLoading } = useQuery(SIGNALS_NEAR, {
    variables: {
      lat: region?.centerLat,
      lng: region?.centerLng,
      radiusKm: region?.radiusKm,
    },
    skip: !region || tab !== "signals",
  });

  const bounds = region ? {
    minLat: region.centerLat - region.radiusKm / 111,
    maxLat: region.centerLat + region.radiusKm / 111,
    minLng: region.centerLng - region.radiusKm / (111 * Math.cos(region.centerLat * Math.PI / 180)),
    maxLng: region.centerLng + region.radiusKm / (111 * Math.cos(region.centerLat * Math.PI / 180)),
  } : null;

  const { data: situationsData, loading: situationsLoading } = useQuery(SITUATIONS_IN_BOUNDS, {
    variables: { ...bounds, limit: 100 },
    skip: !bounds || tab !== "situations",
  });

  const { data: actorsData, loading: actorsLoading } = useQuery(ACTORS_IN_BOUNDS, {
    variables: { ...bounds, limit: 100 },
    skip: !bounds || tab !== "actors",
  });

  const { data: runsData } = useQuery(ADMIN_SCOUT_RUNS, {
    variables: { region: region?.name, limit: 20 },
    skip: !region || tab !== "runs",
  });

  const { data: sourcesData, loading: sourcesLoading } = useQuery(ADMIN_REGION_SOURCES_BY_REGION, {
    variables: { regionId: id },
    skip: !id || tab !== "sources",
  });

  // Flow mutations
  const [runBootstrap] = useMutation(RUN_BOOTSTRAP);
  const [runScrape] = useMutation(RUN_SCRAPE);
  const [runWeave] = useMutation(RUN_WEAVE);
  const [runScoutSource] = useMutation(RUN_SCOUT_SOURCE);
  const [cancelRun] = useMutation(CANCEL_RUN);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const runFlow = async (mutation: MutationFn, label: string) => {
    setBusy(label);
    setError(null);
    try {
      await mutation({ variables: { regionId: id } });
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : `Failed to ${label}`);
    } finally {
      setBusy(null);
    }
  };

  if (regionLoading) return <p className="text-muted-foreground p-4">Loading region...</p>;
  if (!region) return <p className="text-muted-foreground p-4">Region not found.</p>;

  const signals = signalsData?.signalsNear ?? [];
  const situations = situationsData?.situationsInBounds ?? [];
  const actors = actorsData?.actorsInBounds ?? [];
  const regionSources = sourcesData?.adminRegionSourcesByRegion ?? [];
  const runs = runsData?.adminScoutRuns ?? [];

  return (
    <div className="space-y-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <Link to="/scout" className="text-xs text-muted-foreground hover:text-foreground">
            &larr; Back to Scout
          </Link>
          <h1 className="text-xl font-semibold mt-1">{region.name}</h1>
        </div>
        <div className="flex gap-2">
          <button
            onClick={() => runFlow(runBootstrap, "bootstrap")}
            disabled={busy !== null}
            className="text-xs px-3 py-1.5 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50 disabled:opacity-50"
          >
            {busy === "bootstrap" ? "Starting..." : "Bootstrap"}
          </button>
          <button
            onClick={() => runFlow(runScrape, "scrape")}
            disabled={busy !== null}
            className="text-xs px-3 py-1.5 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50 disabled:opacity-50"
          >
            {busy === "scrape" ? "Starting..." : "Scrape"}
          </button>
          <button
            onClick={() => runFlow(runWeave, "weave")}
            disabled={busy !== null}
            className="text-xs px-3 py-1.5 rounded border border-blue-500/30 text-blue-400 hover:bg-blue-500/10 disabled:opacity-50"
          >
            {busy === "weave" ? "Starting..." : "Weave"}
          </button>
        </div>
      </div>

      {error && <p className="text-sm text-red-400">{error}</p>}

      {/* Metadata */}
      <div className="grid grid-cols-2 md:grid-cols-5 gap-4">
        {[
          { label: "Center", value: `${region.centerLat.toFixed(4)}, ${region.centerLng.toFixed(4)}` },
          { label: "Radius", value: `${region.radiusKm} km` },
          { label: "Type", value: region.isLeaf ? "Leaf" : "Parent" },
          { label: "Geo Terms", value: region.geoTerms.join(", ") || "-" },
          { label: "Created", value: formatDate(region.createdAt) },
        ].map((item) => (
          <div key={item.label} className="rounded-lg border border-border p-3">
            <p className="text-xs text-muted-foreground">{item.label}</p>
            <p className="text-sm font-medium mt-0.5">{item.value}</p>
          </div>
        ))}
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
            {t.key === "signals" && signals.length > 0 && (
              <span className="ml-1 text-xs text-muted-foreground">({signals.length})</span>
            )}
            {t.key === "situations" && situations.length > 0 && (
              <span className="ml-1 text-xs text-muted-foreground">({situations.length})</span>
            )}
            {t.key === "actors" && actors.length > 0 && (
              <span className="ml-1 text-xs text-muted-foreground">({actors.length})</span>
            )}
            {t.key === "sources" && regionSources.length > 0 && (
              <span className="ml-1 text-xs text-muted-foreground">({regionSources.length})</span>
            )}
          </button>
        ))}
      </div>

      {/* Signals tab */}
      {tab === "signals" && (
        signalsLoading ? (
          <p className="text-muted-foreground">Loading signals...</p>
        ) : signals.length === 0 ? (
          <p className="text-muted-foreground">No signals in this region.</p>
        ) : (
          <div className="rounded-lg border border-border overflow-hidden">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border bg-muted/50">
                  <th className="text-left px-4 py-2 font-medium">Type</th>
                  <th className="text-left px-4 py-2 font-medium">Title</th>
                  <th className="text-left px-4 py-2 font-medium">Location</th>
                  <th className="text-right px-4 py-2 font-medium">Confidence</th>
                  <th className="text-left px-4 py-2 font-medium">Updated</th>
                </tr>
              </thead>
              <tbody>
                {signals.map((s: Record<string, string | number>) => (
                  <tr key={s.id} className="border-b border-border last:border-0 hover:bg-muted/30">
                    <td className="px-4 py-2">
                      <span className={`text-xs px-2 py-0.5 rounded-full ${typeColor[s.signalType as string] ?? "bg-muted text-muted-foreground"}`}>
                        {s.signalType}
                      </span>
                    </td>
                    <td className="px-4 py-2 max-w-[300px] truncate">
                      <Link to={`/signals/${s.id}`} className="text-blue-400 hover:underline">
                        {s.title}
                      </Link>
                    </td>
                    <td className="px-4 py-2 text-muted-foreground">{s.locationName || "-"}</td>
                    <td className="px-4 py-2 text-right tabular-nums">{s.confidence ? `${(Number(s.confidence) * 100).toFixed(0)}%` : "-"}</td>
                    <td className="px-4 py-2 text-muted-foreground whitespace-nowrap">{s.updatedAt ? formatDate(s.updatedAt as string) : "-"}</td>
                  </tr>
                ))}
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
          <p className="text-muted-foreground">No situations in this region.</p>
        ) : (
          <div className="rounded-lg border border-border overflow-hidden">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border bg-muted/50">
                  <th className="text-left px-4 py-2 font-medium">Headline</th>
                  <th className="text-left px-4 py-2 font-medium">Arc</th>
                  <th className="text-left px-4 py-2 font-medium">Location</th>
                  <th className="text-right px-4 py-2 font-medium">Signals</th>
                  <th className="text-left px-4 py-2 font-medium">Updated</th>
                </tr>
              </thead>
              <tbody>
                {situations.map((s: Record<string, string | number>) => (
                  <tr key={s.id} className="border-b border-border last:border-0 hover:bg-muted/30">
                    <td className="px-4 py-2 max-w-[300px] truncate font-medium">{s.headline}</td>
                    <td className="px-4 py-2">
                      <span className="text-xs px-2 py-0.5 rounded-full bg-purple-500/10 text-purple-400">{s.arc}</span>
                    </td>
                    <td className="px-4 py-2 text-muted-foreground">{s.locationName || "-"}</td>
                    <td className="px-4 py-2 text-right tabular-nums">{s.signalCount ?? "-"}</td>
                    <td className="px-4 py-2 text-muted-foreground whitespace-nowrap">{s.updatedAt ? formatDate(s.updatedAt as string) : "-"}</td>
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
          <p className="text-muted-foreground">No actors in this region.</p>
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
                {actors.map((a: Record<string, string | number>) => (
                  <tr key={a.id} className="border-b border-border last:border-0 hover:bg-muted/30">
                    <td className="px-4 py-2 font-medium">{a.name}</td>
                    <td className="px-4 py-2 text-muted-foreground">{a.actorType || "-"}</td>
                    <td className="px-4 py-2 text-muted-foreground">{a.locationName || "-"}</td>
                    <td className="px-4 py-2 text-right tabular-nums">{a.signalCount ?? "-"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )
      )}

      {/* Sources tab */}
      {tab === "sources" && (
        sourcesLoading ? (
          <p className="text-muted-foreground">Loading sources...</p>
        ) : regionSources.length === 0 ? (
          <p className="text-muted-foreground">No sources linked to this region.</p>
        ) : (
          <div className="rounded-lg border border-border overflow-hidden">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border bg-muted/50">
                  <th className="text-left px-4 py-2 font-medium">Source</th>
                  <th className="text-left px-4 py-2 font-medium">Type</th>
                  <th className="text-left px-4 py-2 font-medium">Last Scraped</th>
                  <th className="text-right px-4 py-2 font-medium">Signals</th>
                  <th className="text-left px-4 py-2 font-medium">Status</th>
                  <th className="text-right px-4 py-2 font-medium">Actions</th>
                </tr>
              </thead>
              <tbody>
                {regionSources.map((s: Record<string, string | number | boolean | null>) => (
                  <tr key={s.id as string} className="border-b border-border last:border-0 hover:bg-muted/30">
                    <td className="px-4 py-2 max-w-[300px] truncate">
                      <Link to={`/sources/${s.id}`} className="text-blue-400 hover:underline">
                        {(s.canonicalValue as string) || (s.url as string)}
                      </Link>
                    </td>
                    <td className="px-4 py-2 text-muted-foreground text-xs">{s.sourceLabel as string}</td>
                    <td className="px-4 py-2 text-muted-foreground whitespace-nowrap">
                      {s.lastScraped ? formatDate(s.lastScraped as string) : "Never"}
                    </td>
                    <td className="px-4 py-2 text-right tabular-nums">{s.signalsProduced as number}</td>
                    <td className="px-4 py-2">
                      <span className={`text-xs px-2 py-0.5 rounded-full border ${
                        s.active
                          ? "bg-green-900/30 text-green-400 border-green-500/30"
                          : "bg-muted text-muted-foreground border-border"
                      }`}>
                        {s.active ? "Active" : "Inactive"}
                      </span>
                    </td>
                    <td className="px-4 py-2 text-right">
                      <button
                        onClick={async () => {
                          try {
                            await runScoutSource({ variables: { sourceIds: [s.id] } });
                          } catch { /* error shown by Apollo */ }
                        }}
                        className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50"
                      >
                        Scout
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )
      )}

      {/* Runs tab */}
      {tab === "runs" && (
        runs.length === 0 ? (
          <p className="text-muted-foreground">No runs for this region.</p>
        ) : (
          <div className="rounded-lg border border-border overflow-hidden">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border bg-muted/50">
                  <th className="text-left px-4 py-2 font-medium">Run</th>
                  <th className="text-left px-4 py-2 font-medium">Flow</th>
                  <th className="text-left px-4 py-2 font-medium">Started</th>
                  <th className="text-left px-4 py-2 font-medium">Status</th>
                  <th className="text-right px-4 py-2 font-medium"></th>
                </tr>
              </thead>
              <tbody>
                {runs.map((run: Record<string, string | null>) => (
                  <tr key={run.runId} className="border-b border-border last:border-0 hover:bg-muted/30">
                    <td className="px-4 py-2">
                      <Link to={`/scout-runs/${run.runId}`} className="text-blue-400 hover:underline font-mono text-xs">
                        {run.runId?.slice(0, 8)}
                      </Link>
                    </td>
                    <td className="px-4 py-2">
                      {run.flowType && (
                        <span className="text-xs px-2 py-0.5 rounded-full bg-blue-500/10 text-blue-400">
                          {run.flowType}
                        </span>
                      )}
                    </td>
                    <td className="px-4 py-2 text-muted-foreground whitespace-nowrap">
                      {run.startedAt ? formatDate(run.startedAt) : "-"}
                    </td>
                    <td className="px-4 py-2">
                      <span className={`text-xs ${run.finishedAt ? "text-green-400" : "text-amber-400"}`}>
                        {run.finishedAt ? "Completed" : "Running"}
                      </span>
                    </td>
                    <td className="px-4 py-2 text-right">
                      {!run.finishedAt && (
                        <button
                          onClick={async () => {
                            await cancelRun({ variables: { runId: run.runId } });
                          }}
                          className="text-xs px-2 py-1 rounded border border-red-500/30 text-red-400 hover:bg-red-500/10"
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
        )
      )}
    </div>
  );
}
