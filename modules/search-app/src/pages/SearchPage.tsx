import { useState, useCallback, useMemo } from "react";
import { useQuery } from "@apollo/client";
import { MapView } from "@/components/MapView";
import { SearchInput } from "@/components/SearchInput";
import { TabBar } from "@/components/TabBar";
import { SignalCard } from "@/components/SignalCard";
import { StoryCard } from "@/components/StoryCard";
import { SignalDetail } from "@/components/SignalDetail";
import { StoryDetail } from "@/components/StoryDetail";
import { EmptyState } from "@/components/EmptyState";
import { useDebouncedBounds, type Bounds } from "@/hooks/useDebouncedBounds";
import { useUrlState, type Tab } from "@/hooks/useUrlState";
import {
  SIGNALS_IN_BOUNDS,
  STORIES_IN_BOUNDS,
  SEARCH_SIGNALS_IN_BOUNDS,
  SEARCH_STORIES_IN_BOUNDS,
} from "@/graphql/queries";

export function SearchPage() {
  const url = useUrlState();
  const { bounds, handleBoundsChange } = useDebouncedBounds(300);

  const [query, setQuery] = useState(url.q);
  const [tab, setTab] = useState<Tab>(url.tab);
  const [selectedId, setSelectedId] = useState<string | null>(url.id);
  const [selectedType, setSelectedType] = useState<"signal" | "story">("signal");
  const [flyToTarget, setFlyToTarget] = useState<{ lng: number; lat: number } | null>(null);

  // Determine which query to use based on search state
  const hasQuery = query.trim().length > 0;
  const boundsVars = bounds
    ? {
        minLat: bounds.minLat,
        maxLat: bounds.maxLat,
        minLng: bounds.minLng,
        maxLng: bounds.maxLng,
      }
    : null;

  // Signals query
  const signalsQuery = useQuery(
    hasQuery ? SEARCH_SIGNALS_IN_BOUNDS : SIGNALS_IN_BOUNDS,
    {
      variables: hasQuery
        ? { query, ...boundsVars, limit: 50 }
        : { ...boundsVars, limit: 50 },
      skip: !bounds || tab !== "signals",
    },
  );

  // Stories query
  const storiesQuery = useQuery(
    hasQuery ? SEARCH_STORIES_IN_BOUNDS : STORIES_IN_BOUNDS,
    {
      variables: hasQuery
        ? { query, ...boundsVars, limit: 20 }
        : { ...boundsVars, limit: 20 },
      skip: !bounds || tab !== "stories",
    },
  );

  // Extract signal data (handles both search and browse responses)
  const signals = useMemo(() => {
    if (tab !== "signals") return [];
    const data = signalsQuery.data;
    if (!data) return [];

    if (hasQuery && data.searchSignalsInBounds) {
      return data.searchSignalsInBounds.map(
        (r: { signal: Record<string, unknown>; score: number }) => ({
          ...r.signal,
          _score: r.score,
        }),
      );
    }
    return data.signalsInBounds ?? [];
  }, [signalsQuery.data, tab, hasQuery]);

  // Extract story data
  const stories = useMemo(() => {
    if (tab !== "stories") return [];
    const data = storiesQuery.data;
    if (!data) return [];

    if (hasQuery && data.searchStoriesInBounds) {
      return data.searchStoriesInBounds.map(
        (r: {
          story: Record<string, unknown>;
          score: number;
          topMatchingSignalTitle: string | null;
        }) => ({
          ...r.story,
          _score: r.score,
          _topMatch: r.topMatchingSignalTitle,
        }),
      );
    }
    return data.storiesInBounds ?? [];
  }, [storiesQuery.data, tab, hasQuery]);

  // Map signals (always show signal points on map regardless of tab)
  const mapSignals = useMemo(() => {
    // If on signals tab, use signal results. If on stories tab, use signal results from a parallel query.
    // For simplicity, show signals from the active tab's data.
    if (tab === "signals") return signals;
    // For stories tab, we could show story centroids as markers, but for now show nothing
    // (stories don't have individual signal data in the list response)
    return stories.map((s: Record<string, unknown>) => ({
      id: s.id as string,
      title: s.headline as string,
      location:
        s.centroidLat && s.centroidLng
          ? { lat: s.centroidLat as number, lng: s.centroidLng as number }
          : null,
      __typename: "GqlStoryMarker",
    }));
  }, [signals, stories, tab]);

  const loading =
    tab === "signals" ? signalsQuery.loading : storiesQuery.loading;

  // URL sync
  const handleBoundsChangeWithUrl = useCallback(
    (newBounds: Bounds) => {
      handleBoundsChange(newBounds);
      const center = {
        lat: (newBounds.minLat + newBounds.maxLat) / 2,
        lng: (newBounds.minLng + newBounds.maxLng) / 2,
      };
      url.updateUrl({ lat: center.lat, lng: center.lng }, { replace: true });
    },
    [handleBoundsChange, url],
  );

  const handleSearch = useCallback(
    (q: string) => {
      setQuery(q);
      setSelectedId(null);
      url.updateUrl({ q: q || undefined }, { replace: false });
    },
    [url],
  );

  const handleTabChange = useCallback(
    (t: Tab) => {
      setTab(t);
      setSelectedId(null);
      url.updateUrl({ tab: t, id: undefined }, { replace: false });
    },
    [url],
  );

  const handleSignalSelect = useCallback(
    (signal: Record<string, unknown>) => {
      const id = signal.id as string;
      setSelectedId(id);
      setSelectedType("signal");
      url.updateUrl({ id }, { replace: true });

      const loc = signal.location as { lat: number; lng: number } | null;
      if (loc) {
        setFlyToTarget({ lng: loc.lng, lat: loc.lat });
      }
    },
    [url],
  );

  const handleStorySelect = useCallback(
    (story: Record<string, unknown>) => {
      const id = story.id as string;
      setSelectedId(id);
      setSelectedType("story");
      url.updateUrl({ id }, { replace: true });

      const lat = story.centroidLat as number | undefined;
      const lng = story.centroidLng as number | undefined;
      if (lat && lng) {
        setFlyToTarget({ lng, lat });
      }
    },
    [url],
  );

  const handleMapSignalClick = useCallback(
    (id: string, lng: number, lat: number) => {
      setSelectedId(id);
      setSelectedType(tab === "stories" ? "story" : "signal");
      setFlyToTarget({ lng, lat });
      url.updateUrl({ id }, { replace: true });
    },
    [url, tab],
  );

  const handleBack = useCallback(() => {
    setSelectedId(null);
    url.updateUrl({ id: undefined }, { replace: true });
  }, [url]);

  // Initial map position from URL
  const initialCenter: [number, number] | undefined =
    url.lng != null && url.lat != null ? [url.lng, url.lat] : undefined;
  const initialZoom = url.z ?? undefined;

  return (
    <div className="flex h-screen">
      {/* Left Pane */}
      <aside className="flex w-[400px] min-w-[400px] flex-col border-r border-border">
        <div className="p-3 border-b border-border">
          <SearchInput
            initialValue={query}
            onSearch={handleSearch}
            loading={loading}
          />
        </div>

        <TabBar
          activeTab={tab}
          onTabChange={handleTabChange}
          signalCount={tab === "signals" ? signals.length : undefined}
          storyCount={tab === "stories" ? stories.length : undefined}
        />

        {/* Content area: detail or list */}
        <div className="flex-1 overflow-y-auto">
          {selectedId ? (
            selectedType === "signal" ? (
              <SignalDetail signalId={selectedId} onBack={handleBack} />
            ) : (
              <StoryDetail storyId={selectedId} onBack={handleBack} />
            )
          ) : tab === "signals" ? (
            signals.length === 0 && !loading ? (
              <EmptyState hasQuery={hasQuery} />
            ) : (
              signals.map((signal: Record<string, unknown>) => (
                <SignalCard
                  key={signal.id as string}
                  signal={signal}
                  score={
                    hasQuery ? (signal._score as number | undefined) : undefined
                  }
                  isSelected={selectedId === signal.id}
                  onClick={() => handleSignalSelect(signal)}
                />
              ))
            )
          ) : stories.length === 0 && !loading ? (
            <EmptyState hasQuery={hasQuery} />
          ) : (
            stories.map((story: Record<string, unknown>) => (
              <StoryCard
                key={story.id as string}
                story={story}
                score={
                  hasQuery ? (story._score as number | undefined) : undefined
                }
                topMatchingSignalTitle={
                  hasQuery
                    ? (story._topMatch as string | undefined)
                    : undefined
                }
                isSelected={selectedId === story.id}
                onClick={() => handleStorySelect(story)}
              />
            ))
          )}
        </div>
      </aside>

      {/* Right Pane: Map */}
      <main className="flex-1">
        <MapView
          signals={mapSignals}
          onBoundsChange={handleBoundsChangeWithUrl}
          onSignalClick={handleMapSignalClick}
          flyToTarget={flyToTarget}
          initialCenter={initialCenter}
          initialZoom={initialZoom}
        />
      </main>
    </div>
  );
}
