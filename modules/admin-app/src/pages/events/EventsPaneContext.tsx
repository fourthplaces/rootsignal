import { createContext, useContext, useState, useMemo, useCallback, useEffect, useRef } from "react";
import { useSearchParams } from "react-router";
import { useQuery, useLazyQuery } from "@apollo/client";
import { ADMIN_EVENTS, ADMIN_CAUSAL_TREE } from "@/graphql/queries";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type AdminEvent = {
  seq: number;
  ts: string;
  type: string;
  name: string;
  layer: string;
  id: string | null;
  parentId: string | null;
  correlationId: string | null;
  runId: string | null;
  summary: string | null;
  payload: string;
};

type AdminEventsPage = {
  events: AdminEvent[];
  nextCursor: number | null;
};

export type CausalTreeResult = {
  events: AdminEvent[];
  rootSeq: number;
};

const LAYER_OPTIONS = ["world", "system", "telemetry"] as const;

// ---------------------------------------------------------------------------
// Context shape
// ---------------------------------------------------------------------------

type EventsPaneContextValue = {
  // Filters
  layers: Set<string>;
  toggleLayer: (layer: string) => void;
  search: string;
  setSearch: (v: string) => void;
  runId: string;
  setRunId: (v: string) => void;
  timeFrom: string;
  setTimeFrom: (v: string) => void;
  timeTo: string;
  setTimeTo: (v: string) => void;

  // Event data (for timeline)
  filteredEvents: AdminEvent[];
  loading: boolean;
  hasMore: boolean;
  loadMore: () => void;
  loadingMore: boolean;

  // Selection
  selectedSeq: number | null;
  selectSeq: (seq: number) => void;

  // Causal tree
  treeData: CausalTreeResult | null;
  treeLoading: boolean;

  // Investigate
  investigateEvent: AdminEvent | null;
  setInvestigateEvent: (event: AdminEvent | null) => void;
};

const EventsPaneContext = createContext<EventsPaneContextValue | null>(null);

export function useEventsPaneContext() {
  const ctx = useContext(EventsPaneContext);
  if (!ctx) throw new Error("useEventsPaneContext must be used within EventsPaneProvider");
  return ctx;
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

export function EventsPaneProvider({ children }: { children: React.ReactNode }) {
  const [searchParams, setSearchParams] = useSearchParams();

  // Filter state from URL
  const [layers, setLayers] = useState<Set<string>>(
    () => new Set(searchParams.get("layers")?.split(",").filter(Boolean) ?? LAYER_OPTIONS),
  );
  const [search, setSearch] = useState(searchParams.get("q") ?? "");
  const [runId, setRunId] = useState(searchParams.get("runId") ?? "");
  const [timeFrom, setTimeFrom] = useState(searchParams.get("from") ?? "");
  const [timeTo, setTimeTo] = useState(searchParams.get("to") ?? "");
  const [selectedSeq, setSelectedSeq] = useState<number | null>(
    searchParams.get("seq") ? Number(searchParams.get("seq")) : null,
  );

  // Infinite scroll state
  const [allEvents, setAllEvents] = useState<AdminEvent[]>([]);
  const [cursor, setCursor] = useState<number | null>(null);

  // Investigate
  const [investigateEvent, setInvestigateEvent] = useState<AdminEvent | null>(null);

  // Sync URL params
  const lastParamsRef = useRef("");
  useEffect(() => {
    const params: Record<string, string> = {};
    const layersStr = [...layers].sort().join(",");
    if (layersStr !== "system,telemetry,world") params.layers = layersStr;
    if (search) params.q = search;
    if (runId) params.runId = runId;
    if (timeFrom) params.from = timeFrom;
    if (timeTo) params.to = timeTo;
    if (selectedSeq != null) params.seq = String(selectedSeq);
    const serialized = JSON.stringify(params);
    if (serialized !== lastParamsRef.current) {
      lastParamsRef.current = serialized;
      setSearchParams(params, { replace: true });
    }
  }, [layers, search, runId, timeFrom, timeTo, selectedSeq, setSearchParams]);

  // Query
  const queryVars = useMemo(
    () => ({
      limit: 50,
      cursor: cursor ?? undefined,
      search: search || undefined,
      runId: runId || undefined,
      from: timeFrom ? new Date(timeFrom).toISOString() : undefined,
      to: timeTo ? new Date(timeTo + "T23:59:59").toISOString() : undefined,
    }),
    [cursor, search, runId, timeFrom, timeTo],
  );

  const { data, loading } = useQuery<{ adminEvents: AdminEventsPage }>(ADMIN_EVENTS, {
    variables: queryVars,
    fetchPolicy: "network-only",
  });

  // When filters change (but not cursor), reset
  const filterKey = useMemo(
    () => JSON.stringify({ search, runId, timeFrom, timeTo }),
    [search, runId, timeFrom, timeTo],
  );
  const prevFilterKeyRef = useRef(filterKey);
  useEffect(() => {
    if (filterKey !== prevFilterKeyRef.current) {
      prevFilterKeyRef.current = filterKey;
      setAllEvents([]);
      setCursor(null);
    }
  }, [filterKey]);

  // Append new data
  useEffect(() => {
    if (data?.adminEvents?.events) {
      const newEvents = data.adminEvents.events;
      if (cursor == null) {
        setAllEvents(newEvents);
      } else {
        setAllEvents((prev) => {
          const existing = new Set(prev.map((e) => e.seq));
          const deduped = newEvents.filter((e) => !existing.has(e.seq));
          return [...prev, ...deduped];
        });
      }
    }
  }, [data, cursor]);

  // Filter by active layers client-side
  const filteredEvents = useMemo(
    () => allEvents.filter((e) => layers.has(e.layer)),
    [allEvents, layers],
  );

  const hasMore = data?.adminEvents?.nextCursor != null;

  const loadMore = useCallback(() => {
    if (data?.adminEvents?.nextCursor != null) {
      setCursor(data.adminEvents.nextCursor);
    }
  }, [data]);

  // Causal tree
  const [fetchTree, { data: treeData, loading: treeLoading }] = useLazyQuery<{
    adminCausalTree: CausalTreeResult;
  }>(ADMIN_CAUSAL_TREE);

  const selectSeq = useCallback(
    (seq: number) => {
      setSelectedSeq(seq);
      // Skip re-fetch if this seq is already within the loaded causal tree
      const currentTree = treeData?.adminCausalTree;
      if (currentTree?.events.some((e) => e.seq === seq)) return;
      fetchTree({ variables: { seq } });
    },
    [fetchTree, treeData],
  );

  const toggleLayer = useCallback((layer: string) => {
    setLayers((prev) => {
      const next = new Set(prev);
      if (next.has(layer)) next.delete(layer);
      else next.add(layer);
      return next;
    });
  }, []);

  const value = useMemo<EventsPaneContextValue>(
    () => ({
      layers,
      toggleLayer,
      search,
      setSearch,
      runId,
      setRunId,
      timeFrom,
      setTimeFrom,
      timeTo,
      setTimeTo,
      filteredEvents,
      loading,
      hasMore,
      loadMore,
      loadingMore: loading && allEvents.length > 0,
      selectedSeq,
      selectSeq,
      treeData: treeData?.adminCausalTree ?? null,
      treeLoading,
      investigateEvent,
      setInvestigateEvent,
    }),
    [
      layers, toggleLayer, search, runId, timeFrom, timeTo,
      filteredEvents, loading, hasMore, loadMore, allEvents.length,
      selectedSeq, selectSeq, treeData, treeLoading,
      investigateEvent,
    ],
  );

  return (
    <EventsPaneContext.Provider value={value}>
      {children}
    </EventsPaneContext.Provider>
  );
}
