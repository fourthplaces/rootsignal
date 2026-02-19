import { useState, useEffect } from "react";
import { useParams } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import { ADMIN_CITY, ADMIN_CITY_SOURCES, ADMIN_SCOUT_STATUS } from "@/graphql/queries";
import { ADD_SOURCE, RUN_SCOUT, STOP_SCOUT, RESET_SCOUT_LOCK } from "@/graphql/mutations";

export function CityDetailPage() {
  const { slug } = useParams<{ slug: string }>();
  const { data: cityData, loading } = useQuery(ADMIN_CITY, { variables: { slug } });
  const { data: sourcesData, refetch: refetchSources } = useQuery(ADMIN_CITY_SOURCES, {
    variables: { citySlug: slug },
  });
  const { data: scoutData, refetch: refetchScout } = useQuery(ADMIN_SCOUT_STATUS, {
    variables: { citySlug: slug },
  });

  const [addSource] = useMutation(ADD_SOURCE);
  const [runScout] = useMutation(RUN_SCOUT);
  const [stopScout] = useMutation(STOP_SCOUT);
  const [resetScoutLock] = useMutation(RESET_SCOUT_LOCK);

  const [showAddSource, setShowAddSource] = useState(false);
  const [sourceUrl, setSourceUrl] = useState("");
  const [sourceReason, setSourceReason] = useState("");

  const isRunning = scoutData?.adminScoutStatus?.running;

  // Poll scout status every 5s when running
  useEffect(() => {
    if (!isRunning) return;
    const interval = setInterval(() => refetchScout(), 5000);
    return () => clearInterval(interval);
  }, [isRunning, refetchScout]);

  if (loading) return <p className="text-muted-foreground">Loading...</p>;

  const city = cityData?.adminCity;
  if (!city) return <p className="text-muted-foreground">City not found</p>;

  const sources = sourcesData?.adminCitySources ?? [];
  const scout = scoutData?.adminScoutStatus;

  const handleAddSource = async (e: React.FormEvent) => {
    e.preventDefault();
    await addSource({
      variables: { citySlug: slug, url: sourceUrl, reason: sourceReason || undefined },
    });
    setSourceUrl("");
    setSourceReason("");
    setShowAddSource(false);
    refetchSources();
  };

  const handleRunScout = async () => {
    await runScout({ variables: { citySlug: slug } });
    refetchScout();
  };

  const handleStopScout = async () => {
    await stopScout({ variables: { citySlug: slug } });
    refetchScout();
  };

  const handleResetLock = async () => {
    await resetScoutLock({ variables: { citySlug: slug } });
    refetchScout();
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold">{city.name}</h1>
          <p className="text-sm text-muted-foreground">
            {city.centerLat.toFixed(4)}, {city.centerLng.toFixed(4)} &middot; {city.radiusKm}km
            radius
          </p>
        </div>
      </div>

      {/* Scout controls */}
      <div className="rounded-lg border border-border p-4">
        <div className="flex items-center justify-between mb-2">
          <h2 className="text-sm font-medium">Scout</h2>
          <span
            className={`text-xs px-2 py-0.5 rounded-full ${
              scout?.running
                ? "bg-green-900 text-green-300"
                : "bg-secondary text-muted-foreground"
            }`}
          >
            {scout?.running ? "Running" : "Idle"}
          </span>
        </div>
        <p className="text-sm text-muted-foreground mb-3">
          {scout?.sourcesDue ?? 0} sources due
          {scout?.lastScouted && (
            <>
              {" "}
              &middot; Last run {new Date(scout.lastScouted).toLocaleString()}
            </>
          )}
        </p>
        <div className="flex gap-2">
          {!scout?.running ? (
            <button
              onClick={handleRunScout}
              className="px-3 py-1.5 rounded-md bg-primary text-primary-foreground text-sm hover:bg-primary/90"
            >
              Run Scout
            </button>
          ) : (
            <button
              onClick={handleStopScout}
              className="px-3 py-1.5 rounded-md bg-destructive text-destructive-foreground text-sm hover:bg-destructive/90"
            >
              Stop Scout
            </button>
          )}
          <button
            onClick={handleResetLock}
            className="px-3 py-1.5 rounded-md border border-input text-sm hover:bg-accent"
          >
            Reset Lock
          </button>
        </div>
      </div>

      {/* Sources */}
      <div className="rounded-lg border border-border p-4">
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
                  sourceType: string;
                  effectiveWeight: number;
                  signalsProduced: number;
                  cadenceHours: number;
                  lastScraped: string | null;
                }) => (
                  <tr key={s.id} className="border-b border-border/50">
                    <td className="py-2 truncate max-w-[200px]">{s.canonicalValue}</td>
                    <td className="py-2">{s.sourceType}</td>
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
    </div>
  );
}
