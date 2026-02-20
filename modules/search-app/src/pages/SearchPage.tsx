import { useState, useCallback, useMemo } from "react";
import { useQuery } from "@apollo/client";
import { MapView } from "@/components/MapView";
import { SearchInput, parseQuery } from "@/components/SearchInput";
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
  TAGS,
} from "@/graphql/queries";

// Maps type filter keys to GraphQL __typename values
const TYPE_TO_TYPENAME: Record<string, string> = {
  gathering: "GqlGatheringSignal",
  aid: "GqlAidSignal",
  need: "GqlNeedSignal",
  notice: "GqlNoticeSignal",
  tension: "GqlTensionSignal",
};

export function SearchPage() {
  const url = useUrlState();
  const { bounds, handleBoundsChange } = useDebouncedBounds(300);

  const [rawQuery, setRawQuery] = useState(url.q);
  const [tab, setTab] = useState<Tab>(url.tab);
  const [selectedId, setSelectedId] = useState<string | null>(url.id);
  const [selectedType, setSelectedType] = useState<"signal" | "story">("signal");
  const [flyToTarget, setFlyToTarget] = useState<{ lng: number; lat: number } | null>(null);

  const parsed = useMemo(() => parseQuery(rawQuery), [rawQuery]);
  const hasTextQuery = parsed.text.length > 0;
  const hasTypeFilter = parsed.types.length > 0;
  const hasTagFilter = parsed.tags.length > 0;

  // Fetch available tags for the pill bar
  const { data: tagsData } = useQuery(TAGS, { variables: { limit: 20 } });
  const availableTags = tagsData?.tags ?? [];

  const boundsVars = bounds
    ? {
        minLat: bounds.minLat,
        maxLat: bounds.maxLat,
        minLng: bounds.minLng,
        maxLng: bounds.maxLng,
      }
    : null;

  // Signals query — semantic search only when there's free text
  const signalsQuery = useQuery(
    hasTextQuery ? SEARCH_SIGNALS_IN_BOUNDS : SIGNALS_IN_BOUNDS,
    {
      variables: hasTextQuery
        ? { query: parsed.text, ...boundsVars, limit: 50 }
        : { ...boundsVars, limit: 50 },
      skip: !bounds || tab !== "signals",
    },
  );

  // Stories query — pass first tag filter to backend
  const storiesQuery = useQuery(
    hasTextQuery ? SEARCH_STORIES_IN_BOUNDS : STORIES_IN_BOUNDS,
    {
      variables: hasTextQuery
        ? { query: parsed.text, ...boundsVars, limit: 20 }
        : { ...boundsVars, tag: hasTagFilter ? parsed.tags[0] : null, limit: 20 },
      skip: !bounds || tab !== "stories",
    },
  );

  // Extract signal data, then apply client-side type filter
  const signals = useMemo(() => {
    if (tab !== "signals") return [];
    const data = signalsQuery.data;
    if (!data) return [];

    let items: Record<string, unknown>[];
    if (hasTextQuery && data.searchSignalsInBounds) {
      items = data.searchSignalsInBounds.map(
        (r: { signal: Record<string, unknown>; score: number }) => ({
          ...r.signal,
          _score: r.score,
        }),
      );
    } else {
      items = data.signalsInBounds ?? [];
    }

    // Client-side type filtering
    if (hasTypeFilter) {
      const allowedTypenames = new Set(
        parsed.types.map((t) => TYPE_TO_TYPENAME[t]).filter(Boolean),
      );
      items = items.filter((s) => allowedTypenames.has(s.__typename as string));
    }

    return items;
  }, [signalsQuery.data, tab, hasTextQuery, hasTypeFilter, parsed.types]);

  // Extract story data, client-side type filter on dominantType
  const stories = useMemo(() => {
    if (tab !== "stories") return [];
    const data = storiesQuery.data;
    if (!data) return [];

    let items: Record<string, unknown>[];
    if (hasTextQuery && data.searchStoriesInBounds) {
      items = data.searchStoriesInBounds.map(
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
    } else {
      items = data.storiesInBounds ?? [];
    }

    // Client-side type filtering for stories (by dominantType)
    if (hasTypeFilter) {
      const allowedTypes = new Set(parsed.types.map((t) => t.charAt(0).toUpperCase() + t.slice(1)));
      items = items.filter((s) => allowedTypes.has(s.dominantType as string));
    }

    return items;
  }, [storiesQuery.data, tab, hasTextQuery, hasTypeFilter, parsed.types]);

  // Map signals for the map view
  const mapSignals = useMemo(() => {
    if (tab === "signals") return signals as { id: string; title: string; location?: { lat: number; lng: number } | null; __typename?: string }[];
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
      setRawQuery(q);
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
            initialValue={rawQuery}
            onSearch={handleSearch}
            loading={loading}
            availableTags={availableTags}
            activeTab={tab}
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
              <EmptyState hasQuery={hasTextQuery || hasTypeFilter} />
            ) : (
              signals.map((signal: Record<string, unknown>) => (
                <SignalCard
                  key={signal.id as string}
                  signal={signal}
                  score={
                    hasTextQuery ? (signal._score as number | undefined) : undefined
                  }
                  isSelected={selectedId === signal.id}
                  onClick={() => handleSignalSelect(signal)}
                />
              ))
            )
          ) : stories.length === 0 && !loading ? (
            <EmptyState hasQuery={hasTextQuery || hasTypeFilter || hasTagFilter} />
          ) : (
            stories.map((story: Record<string, unknown>) => (
              <StoryCard
                key={story.id as string}
                story={story}
                score={
                  hasTextQuery ? (story._score as number | undefined) : undefined
                }
                topMatchingSignalTitle={
                  hasTextQuery
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
