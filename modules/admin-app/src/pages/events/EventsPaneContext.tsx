import { createContext, useContext, useState, useMemo, useCallback, useEffect, useRef } from "react";
import { useSearchParams } from "react-router";
import { useQuery, useLazyQuery, useSubscription } from "@apollo/client";
import { ADMIN_EVENTS, ADMIN_CAUSAL_TREE, ADMIN_CAUSAL_FLOW, EVENTS_SUBSCRIPTION } from "@/graphql/queries";

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
  handlerId: string | null;
  summary: string | null;
  payload: string;
};

type AdminEventsPage = {
  events: AdminEvent[];
  nextCursor: number | null;
};

// CausalTreeResult kept for the GraphQL query shape (rootSeq comes from server)
type CausalTreeResult = {
  events: AdminEvent[];
  rootSeq: number;
};

export type FlowSelection =
  | { kind: "event-type"; handlerId: string | null; name: string }
  | { kind: "handler"; handlerId: string }
  | null;

function parseFlowSelection(params: URLSearchParams): FlowSelection {
  const kind = params.get("fsk");
  if (kind === "event-type") {
    const name = params.get("fsn");
    if (!name) return null;
    const handlerId = params.get("fsh"); // null when absent = root
    return { kind: "event-type", handlerId, name };
  }
  if (kind === "handler") {
    const handlerId = params.get("fsh");
    if (!handlerId) return null;
    return { kind: "handler", handlerId };
  }
  return null;
}

function serializeFlowSelection(sel: FlowSelection, params: Record<string, string>) {
  if (!sel) return;
  params.fsk = sel.kind;
  if (sel.kind === "event-type") {
    params.fsn = sel.name;
    if (sel.handlerId != null) params.fsh = sel.handlerId;
  } else {
    params.fsh = sel.handlerId;
  }
}

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

  // Live subscription
  live: boolean;

  // Selection
  selectedSeq: number | null;
  selectSeq: (seq: number, runId?: string) => void;

  // Causal tree (smart source — flow data when flow is open for same run, else tree query)
  treeEvents: AdminEvent[] | null;
  treeLoading: boolean;

  // Investigate
  investigateEvent: AdminEvent | null;
  setInvestigateEvent: (event: AdminEvent | null) => void;

  // Causal flow
  flowRunId: string | null;
  openFlow: (runId: string, initialSelection?: FlowSelection) => void;
  closeFlow: () => void;
  flowData: AdminEvent[] | null;
  flowLoading: boolean;

  // Flow → Tree highlighting
  flowSelection: FlowSelection;
  setFlowSelection: (sel: FlowSelection) => void;
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
  const [debouncedSearch, setDebouncedSearch] = useState(search);
  const [runId, setRunId] = useState(searchParams.get("runId") ?? "");
  const [timeFrom, setTimeFrom] = useState(searchParams.get("from") ?? "");
  const [timeTo, setTimeTo] = useState(searchParams.get("to") ?? "");
  const [selectedSeq, setSelectedSeq] = useState<number | null>(
    searchParams.get("seq") ? Number(searchParams.get("seq")) : null,
  );

  // Debounce search → debouncedSearch (300ms)
  useEffect(() => {
    const id = setTimeout(() => setDebouncedSearch(search), 300);
    return () => clearTimeout(id);
  }, [search]);

  // Infinite scroll state
  const [allEvents, setAllEvents] = useState<AdminEvent[]>([]);
  const [cursor, setCursor] = useState<number | null>(null);

  // Investigate
  const [investigateEvent, setInvestigateEvent] = useState<AdminEvent | null>(null);

  // Causal flow
  const [flowSelection, setFlowSelection] = useState<FlowSelection>(() => parseFlowSelection(searchParams));
  const [flowRunId, setFlowRunId] = useState<string | null>(searchParams.get("flow"));
  const [flowEvents, setFlowEvents] = useState<AdminEvent[] | null>(null);
  const [fetchFlow, { data: flowQueryData, loading: flowLoading }] = useLazyQuery<{
    adminCausalFlow: { events: AdminEvent[] };
  }>(ADMIN_CAUSAL_FLOW);

  useEffect(() => {
    if (flowRunId) {
      setFlowEvents(null); // reset on new flow
      fetchFlow({ variables: { runId: flowRunId } });
    } else {
      setFlowEvents(null);
    }
  }, [flowRunId, fetchFlow]);

  // Seed local flow events from query result
  useEffect(() => {
    if (flowQueryData?.adminCausalFlow?.events) {
      setFlowEvents(flowQueryData.adminCausalFlow.events);
    }
  }, [flowQueryData]);

  // Selected run ID — tracks which run the user last clicked on
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);

  // Atomic open: sets run + optional initial selection together.
  const openFlow = useCallback((runId: string, initialSelection?: FlowSelection) => {
    setFlowRunId(runId);
    setFlowSelection(initialSelection ?? null);
  }, []);

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
    if (flowRunId) params.flow = flowRunId;
    serializeFlowSelection(flowSelection, params);
    const serialized = JSON.stringify(params);
    if (serialized !== lastParamsRef.current) {
      lastParamsRef.current = serialized;
      setSearchParams(params, { replace: true });
    }
  }, [layers, search, runId, timeFrom, timeTo, selectedSeq, flowRunId, flowSelection, setSearchParams]);

  // Query
  const queryVars = useMemo(
    () => ({
      limit: 50,
      cursor: cursor ?? undefined,
      search: debouncedSearch || undefined,
      runId: runId || undefined,
      from: timeFrom ? new Date(timeFrom).toISOString() : undefined,
      to: timeTo ? new Date(timeTo + "T23:59:59").toISOString() : undefined,
    }),
    [cursor, debouncedSearch, runId, timeFrom, timeTo],
  );

  const { data, loading } = useQuery<{ adminEvents: AdminEventsPage }>(ADMIN_EVENTS, {
    variables: queryVars,
    fetchPolicy: "network-only",
  });

  // When filters change (but not cursor), reset
  const filterKey = useMemo(
    () => JSON.stringify({ search: debouncedSearch, runId, timeFrom, timeTo }),
    [debouncedSearch, runId, timeFrom, timeTo],
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

  // ── Live subscription ──
  // Capture the highest seq from the initial query (set once) to use as
  // the catch-up cursor so the subscription replays any events missed
  // between the initial HTTP query and the WebSocket connect.
  const [lastSeq, setLastSeq] = useState<number | undefined>(undefined);
  useEffect(() => {
    if (data?.adminEvents?.events?.length && lastSeq === undefined) {
      setLastSeq(data.adminEvents.events[0].seq);
    }
  }, [data, lastSeq]);

  // Only subscribe when no filters are active (live view of all events)
  const hasFilters = !!(debouncedSearch || runId || timeFrom || timeTo);
  const subscriptionVars = useMemo(() => ({ lastSeq }), [lastSeq]);

  const { data: subData } = useSubscription<{ events: AdminEvent }>(EVENTS_SUBSCRIPTION, {
    variables: subscriptionVars,
    skip: hasFilters,
  });

  // Prepend live events
  useEffect(() => {
    if (!subData?.events) return;
    const event = subData.events;
    setAllEvents((prev) => {
      if (prev.some((e) => e.seq === event.seq)) return prev;
      return [event, ...prev];
    });
  }, [subData]);

  // Append live events to flow when they belong to the open flow's run
  useEffect(() => {
    if (!subData?.events || !flowRunId) return;
    const event = subData.events;
    if (event.runId === flowRunId) {
      setFlowEvents((prev) => {
        if (!prev) return [event];
        if (prev.some((e) => e.seq === event.seq)) return prev;
        return [...prev, event];
      });
    }
  }, [subData, flowRunId]);

  const live = !hasFilters && !!subData;

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

  // Refs for stable callbacks (avoid cascade re-renders from data deps)
  const selectedSeqRef = useRef(selectedSeq);
  selectedSeqRef.current = selectedSeq;
  const treeDataRef = useRef(treeData);
  treeDataRef.current = treeData;
  const flowEventsRef = useRef(flowEvents);
  flowEventsRef.current = flowEvents;

  const closeFlow = useCallback(() => {
    setFlowRunId(null);
    setFlowSelection(null);
    // Re-fetch tree for current selectedSeq since treeData may be stale
    const currentSeq = selectedSeqRef.current;
    if (currentSeq != null) {
      const inTree = treeDataRef.current?.adminCausalTree?.events.some((e) => e.seq === currentSeq);
      if (!inTree) fetchTree({ variables: { seq: currentSeq } });
    }
  }, [fetchTree]);

  const selectSeq = useCallback(
    (seq: number, evtRunId?: string) => {
      setSelectedSeq(seq);
      if (evtRunId) setSelectedRunId(evtRunId);

      const inTree = treeDataRef.current?.adminCausalTree?.events.some((e) => e.seq === seq);
      const inFlow = flowRunId && evtRunId === flowRunId &&
        flowEventsRef.current?.some((e) => e.seq === seq);
      if (!inTree && !inFlow) {
        fetchTree({ variables: { seq } });
      }
    },
    [fetchTree, flowRunId],
  );

  // Hydrate tree on mount when selectedSeq comes from URL
  const didHydrateTree = useRef(false);
  useEffect(() => {
    if (!didHydrateTree.current && selectedSeq != null) {
      didHydrateTree.current = true;
      fetchTree({ variables: { seq: selectedSeq } });
    }
  }, [selectedSeq, fetchTree]);

  const toggleLayer = useCallback((layer: string) => {
    setLayers((prev) => {
      const next = new Set(prev);
      if (next.has(layer)) next.delete(layer);
      else next.add(layer);
      return next;
    });
  }, []);

  // Smart tree source: use flow data when flow is open for the same run
  const effectiveTreeSource = useMemo(() => {
    // Use selectedRunId ?? flowRunId to handle mount hydration
    // (when ?flow=abc&seq=42 is in URL, selectedRunId is null until first click)
    const effectiveRunId = selectedRunId ?? flowRunId;
    if (flowRunId && flowEvents && effectiveRunId === flowRunId) {
      return { events: flowEvents, source: "flow" as const };
    }
    const tree = treeData?.adminCausalTree;
    if (tree) {
      return { events: tree.events, source: "tree" as const };
    }
    return null;
  }, [flowRunId, flowEvents, selectedRunId, treeData]);

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
      live,
      selectedSeq,
      selectSeq,
      treeEvents: effectiveTreeSource?.events ?? null,
      treeLoading: effectiveTreeSource?.source === "flow" ? flowLoading : treeLoading,
      investigateEvent,
      setInvestigateEvent,
      flowRunId,
      openFlow,
      closeFlow,
      flowData: flowEvents,
      flowLoading,
      flowSelection,
      setFlowSelection,
    }),
    [
      layers, toggleLayer, search, runId, timeFrom, timeTo,
      filteredEvents, loading, hasMore, loadMore, allEvents.length,
      live, selectedSeq, selectSeq, effectiveTreeSource, treeLoading, flowLoading,
      investigateEvent,
      flowRunId, openFlow, closeFlow, flowEvents,
      flowSelection,
    ],
  );

  return (
    <EventsPaneContext.Provider value={value}>
      {children}
    </EventsPaneContext.Provider>
  );
}
