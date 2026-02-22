import { useState, useCallback, useMemo, useRef } from "react";
import { useQuery, useMutation } from "@apollo/client";
import { MapView } from "@/components/MapView";
import { SearchInput, parseQuery, toggleToken } from "@/components/SearchInput";
import { TabBar } from "@/components/TabBar";
import { SignalCard } from "@/components/SignalCard";
import { SituationCard } from "@/components/SituationCard";
import { SignalDetail } from "@/components/SignalDetail";
import { SituationDetail } from "@/components/SituationDetail";
import { EmptyState } from "@/components/EmptyState";
import { BottomSheet, type Snap } from "@/components/BottomSheet";
import { useDebouncedBounds, type Bounds } from "@/hooks/useDebouncedBounds";
import { useUrlState, type Tab } from "@/hooks/useUrlState";
import { useMediaQuery } from "@/hooks/useMediaQuery";
import {
  SIGNALS_IN_BOUNDS,
  SITUATIONS_IN_BOUNDS,
  SEARCH_SIGNALS_IN_BOUNDS,
} from "@/graphql/queries";
import { RECORD_DEMAND } from "@/graphql/mutations";

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
  const [selectedType, setSelectedType] = useState<"signal" | "situation">("signal");
  const [flyToTarget, setFlyToTarget] = useState<{ lng: number; lat: number } | null>(null);
  const [sheetSnap, setSheetSnap] = useState<Snap>("half");
  const isDesktop = useMediaQuery("(min-width: 768px)");

  const [recordDemand] = useMutation(RECORD_DEMAND);
  const lastDemandQuery = useRef<string>("");

  const parsed = useMemo(() => parseQuery(rawQuery), [rawQuery]);
  const hasTextQuery = parsed.text.length > 0;
  const hasTypeFilter = parsed.types.length > 0;

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

  // Situations query
  const situationsQuery = useQuery(SITUATIONS_IN_BOUNDS, {
    variables: { ...boundsVars, limit: 20 },
    skip: !bounds || tab !== "situations",
  });

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

  // Extract situation data
  const situations = useMemo(() => {
    if (tab !== "situations") return [];
    const data = situationsQuery.data;
    if (!data) return [];
    return (data.situationsInBounds ?? []) as Record<string, unknown>[];
  }, [situationsQuery.data, tab]);

  // Map markers for the map view
  const mapSignals = useMemo(() => {
    if (tab === "signals") return signals as { id: string; title: string; location?: { lat: number; lng: number } | null; __typename?: string }[];
    return situations.map((s: Record<string, unknown>) => ({
      id: s.id as string,
      title: s.headline as string,
      location:
        s.centroidLat && s.centroidLng
          ? { lat: s.centroidLat as number, lng: s.centroidLng as number }
          : null,
      __typename: "GqlSituationMarker",
    }));
  }, [signals, situations, tab]);

  const loading =
    tab === "signals" ? signalsQuery.loading : situationsQuery.loading;

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

      // Fire demand signal (fire-and-forget) when we have bounds and a non-empty query
      const text = parseQuery(q).text;
      if (text && bounds && text !== lastDemandQuery.current) {
        lastDemandQuery.current = text;
        const centerLat = (bounds.minLat + bounds.maxLat) / 2;
        const centerLng = (bounds.minLng + bounds.maxLng) / 2;
        const latSpan = bounds.maxLat - bounds.minLat;
        const lngSpan = bounds.maxLng - bounds.minLng;
        const radiusKm = Math.max(
          1,
          Math.min(500, Math.sqrt(latSpan ** 2 + lngSpan ** 2) * 111 / 2),
        );
        recordDemand({
          variables: { query: text, centerLat, centerLng, radiusKm },
        }).catch(() => {}); // fire-and-forget
      }
    },
    [url, bounds, recordDemand],
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
      setSheetSnap("full");
      url.updateUrl({ id }, { replace: true });

      const loc = signal.location as { lat: number; lng: number } | null;
      if (loc) {
        setFlyToTarget({ lng: loc.lng, lat: loc.lat });
      }
    },
    [url],
  );

  const handleSituationSelect = useCallback(
    (situation: Record<string, unknown>) => {
      const id = situation.id as string;
      setSelectedId(id);
      setSelectedType("situation");
      setSheetSnap("full");
      url.updateUrl({ id }, { replace: true });

      const lat = situation.centroidLat as number | undefined;
      const lng = situation.centroidLng as number | undefined;
      if (lat && lng) {
        setFlyToTarget({ lng, lat });
      }
    },
    [url],
  );

  const handleMapSignalClick = useCallback(
    (id: string, lng: number, lat: number) => {
      setSelectedId(id);
      setSelectedType(tab === "situations" ? "situation" : "signal");
      setSheetSnap("full");
      setFlyToTarget({ lng, lat });
      url.updateUrl({ id }, { replace: true });
    },
    [url, tab],
  );

  const handleBack = useCallback(() => {
    setSelectedId(null);
    setSheetSnap("half");
    url.updateUrl({ id: undefined }, { replace: true });
  }, [url]);

  const handleTypeClick = useCallback(
    (typeKey: string) => {
      const next = toggleToken(rawQuery, "type", typeKey);
      setRawQuery(next);
      setSelectedId(null);
      url.updateUrl({ q: next || undefined }, { replace: false });
    },
    [rawQuery, url],
  );

  // Initial map position from URL
  const initialCenter: [number, number] | undefined =
    url.lng != null && url.lat != null ? [url.lng, url.lat] : undefined;
  const initialZoom = url.z ?? undefined;

  const handleSearchFocus = useCallback(() => {
    if (!isDesktop && sheetSnap === "peek") {
      setSheetSnap("half");
    }
  }, [isDesktop, sheetSnap]);

  const sidebarContent = (
    <>
      <div className="p-3 border-b border-border">
        <SearchInput
          initialValue={rawQuery}
          onSearch={handleSearch}
          loading={loading}
          onFocus={handleSearchFocus}
        />
      </div>

      <TabBar
        activeTab={tab}
        onTabChange={handleTabChange}
        signalCount={tab === "signals" ? signals.length : undefined}
        situationCount={tab === "situations" ? situations.length : undefined}
      />

      {/* Content area: detail or list */}
      <div className="flex-1 overflow-y-auto">
        {selectedId ? (
          selectedType === "signal" ? (
            <SignalDetail signalId={selectedId} onBack={handleBack} />
          ) : (
            <SituationDetail situationId={selectedId} onBack={handleBack} />
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
                onTypeClick={handleTypeClick}
              />
            ))
          )
        ) : situations.length === 0 && !loading ? (
          <EmptyState hasQuery={hasTextQuery || hasTypeFilter} />
        ) : (
          situations.map((situation: Record<string, unknown>) => (
            <SituationCard
              key={situation.id as string}
              situation={situation}
              isSelected={selectedId === situation.id}
              onClick={() => handleSituationSelect(situation)}
            />
          ))
        )}
      </div>
    </>
  );

  return (
    <div className="flex h-screen">
      {/* Desktop sidebar */}
      <aside className="hidden md:flex w-[400px] min-w-[400px] flex-col border-r border-border">
        {sidebarContent}
      </aside>

      {/* Mobile bottom sheet */}
      {!isDesktop && (
        <BottomSheet snap={sheetSnap} onSnapChange={setSheetSnap}>
          {sidebarContent}
        </BottomSheet>
      )}

      {/* Map */}
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
