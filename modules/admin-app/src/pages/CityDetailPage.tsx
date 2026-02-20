import { useState, useEffect } from "react";
import { useParams, Link } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import { ADMIN_CITY, ADMIN_CITY_SOURCES, ADMIN_SCOUT_STATUS, SIGNALS_NEAR_GEO_JSON, STORIES_IN_BOUNDS } from "@/graphql/queries";
import { ADD_SOURCE, RUN_SCOUT, RESET_SCOUT_LOCK } from "@/graphql/mutations";
import { CityMap } from "./MapPage";
import type { FeatureCollection } from "geojson";

type Tab = "stories" | "signals" | "map" | "sources";
const TABS: { key: Tab; label: string }[] = [
  { key: "stories", label: "Stories" },
  { key: "signals", label: "Signals" },
  { key: "map", label: "Map" },
  { key: "sources", label: "Sources" },
];

export function CityDetailPage() {
  const { slug } = useParams<{ slug: string }>();
  const [tab, setTab] = useState<Tab>("stories");
  const [menuOpen, setMenuOpen] = useState(false);

  const { data: cityData, loading } = useQuery(ADMIN_CITY, { variables: { slug } });
  const { data: sourcesData, refetch: refetchSources } = useQuery(ADMIN_CITY_SOURCES, {
    variables: { citySlug: slug },
    skip: tab !== "sources",
  });
  const { data: scoutData, refetch: refetchScout } = useQuery(ADMIN_SCOUT_STATUS, {
    variables: { citySlug: slug },
  });

  const city = cityData?.adminCity;

  const { data: geoData } = useQuery(SIGNALS_NEAR_GEO_JSON, {
    variables: city
      ? { lat: city.centerLat, lng: city.centerLng, radiusKm: city.radiusKm }
      : undefined,
    skip: !city || tab !== "signals",
  });

  const { data: storiesData } = useQuery(STORIES_IN_BOUNDS, {
    variables: city
      ? (() => {
          const latDelta = city.radiusKm / 111.0;
          const lngDelta = city.radiusKm / (111.0 * Math.cos((city.centerLat * Math.PI) / 180));
          return {
            minLat: city.centerLat - latDelta,
            maxLat: city.centerLat + latDelta,
            minLng: city.centerLng - lngDelta,
            maxLng: city.centerLng + lngDelta,
            limit: 50,
          };
        })()
      : undefined,
    skip: !city || tab !== "stories",
  });

  const [addSource] = useMutation(ADD_SOURCE);
  const [runScout] = useMutation(RUN_SCOUT);
  const [resetScoutLock] = useMutation(RESET_SCOUT_LOCK);

  const [showAddSource, setShowAddSource] = useState(false);
  const [sourceUrl, setSourceUrl] = useState("");
  const [sourceReason, setSourceReason] = useState("");

  const isRunning = scoutData?.adminScoutStatus?.running;

  useEffect(() => {
    if (!isRunning) return;
    const interval = setInterval(() => refetchScout(), 5000);
    return () => clearInterval(interval);
  }, [isRunning, refetchScout]);

  // Close menu on outside click
  useEffect(() => {
    if (!menuOpen) return;
    const close = () => setMenuOpen(false);
    document.addEventListener("click", close);
    return () => document.removeEventListener("click", close);
  }, [menuOpen]);

  if (loading) return <p className="text-muted-foreground">Loading...</p>;
  if (!city) return <p className="text-muted-foreground">City not found</p>;

  const sources = sourcesData?.adminCitySources ?? [];
  const stories = storiesData?.storiesInBounds ?? [];

  const signalFeatures: { id: string; title: string; type: string; lat: number; lng: number }[] = [];
  if (geoData?.signalsNearGeoJson) {
    const fc: FeatureCollection = JSON.parse(geoData.signalsNearGeoJson);
    for (const f of fc.features) {
      if (f.geometry.type === "Point" && f.properties) {
        signalFeatures.push({
          id: f.properties.id ?? f.id ?? "",
          title: f.properties.title ?? "",
          type: f.properties.node_type ?? "",
          lat: f.geometry.coordinates[1]!,
          lng: f.geometry.coordinates[0]!,
        });
      }
    }
  }

  const handleRunScout = async () => {
    await runScout({ variables: { citySlug: slug } });
    refetchScout();
    setMenuOpen(false);
  };

  const handleResetLock = async () => {
    await resetScoutLock({ variables: { citySlug: slug } });
    refetchScout();
    setMenuOpen(false);
  };

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

  return (
    <div className="space-y-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold">{city.name}</h1>
          <p className="text-sm text-muted-foreground">
            {city.centerLat.toFixed(4)}, {city.centerLng.toFixed(4)} &middot; {city.radiusKm}km
            radius
          </p>
        </div>
        <div className="relative">
          <button
            onClick={(e) => {
              e.stopPropagation();
              setMenuOpen(!menuOpen);
            }}
            className="p-2 rounded-md hover:bg-accent text-muted-foreground hover:text-foreground"
          >
            &hellip;
          </button>
          {menuOpen && (
            <div className="absolute right-0 top-full mt-1 w-40 rounded-md border border-border bg-card shadow-lg py-1 z-50">
              <button
                onClick={handleRunScout}
                className="w-full text-left px-3 py-2 text-sm hover:bg-accent"
              >
                Run Scout
              </button>
              <button
                onClick={handleResetLock}
                className="w-full text-left px-3 py-2 text-sm hover:bg-accent"
              >
                Reset Lock
              </button>
            </div>
          )}
        </div>
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

      {/* Tab content */}
      {tab === "stories" && (
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border text-left text-muted-foreground">
                <th className="pb-2 font-medium">Headline</th>
                <th className="pb-2 font-medium">Arc</th>
                <th className="pb-2 font-medium">Category</th>
                <th className="pb-2 font-medium">Energy</th>
                <th className="pb-2 font-medium">Signals</th>
              </tr>
            </thead>
            <tbody>
              {stories.map(
                (s: {
                  id: string;
                  headline: string;
                  arc: string | null;
                  category: string | null;
                  energy: number;
                  signalCount: number;
                }) => (
                  <tr key={s.id} className="border-b border-border/50 hover:bg-accent/30">
                    <td className="py-2 max-w-md">
                      <Link to={`/stories/${s.id}`} className="hover:underline line-clamp-1">
                        {s.headline}
                      </Link>
                    </td>
                    <td className="py-2">
                      {s.arc && (
                        <span className="px-2 py-0.5 rounded-full text-xs bg-secondary">
                          {s.arc}
                        </span>
                      )}
                    </td>
                    <td className="py-2 text-muted-foreground">{s.category}</td>
                    <td className="py-2">{s.energy.toFixed(1)}</td>
                    <td className="py-2">{s.signalCount}</td>
                  </tr>
                ),
              )}
            </tbody>
          </table>
        </div>
      )}

      {tab === "signals" && (
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border text-left text-muted-foreground">
                <th className="pb-2 font-medium">Title</th>
                <th className="pb-2 font-medium">Type</th>
              </tr>
            </thead>
            <tbody>
              {signalFeatures.map((s) => (
                <tr key={s.id} className="border-b border-border/50 hover:bg-accent/30">
                  <td className="py-2">
                    <Link to={`/signals/${s.id}`} className="hover:underline line-clamp-1">
                      {s.title}
                    </Link>
                  </td>
                  <td className="py-2">
                    <span className="px-2 py-0.5 rounded-full text-xs bg-secondary">{s.type}</span>
                  </td>
                </tr>
              ))}
              {signalFeatures.length === 0 && (
                <tr>
                  <td colSpan={2} className="py-4 text-muted-foreground text-center">
                    No signals found
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      )}

      {tab === "map" && <CityMap city={city} />}

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
      )}
    </div>
  );
}
