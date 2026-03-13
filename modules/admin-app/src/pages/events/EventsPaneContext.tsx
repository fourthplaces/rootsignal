import { createContext, useContext, useReducer, useMemo, useCallback, useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router";
import { useQuery, useLazyQuery, useSubscription } from "@apollo/client";
import { ADMIN_EVENTS, ADMIN_CAUSAL_TREE, ADMIN_CAUSAL_FLOW, EVENTS_SUBSCRIPTION } from "@/graphql/queries";
import type { InvestigateMode } from "@/components/InvestigateDrawer";

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
    const handlerId = params.get("fsh");
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

export type LogsFilter = {
  eventId: string;
  handlerId: string;
  runId: string | null;
};

const LAYER_OPTIONS = ["world", "system", "telemetry"] as const;

// ---------------------------------------------------------------------------
// Reducer — single source of truth for all state
// ---------------------------------------------------------------------------

type State = {
  events: AdminEvent[];
  nextCursor: number | null;
  paginationCursor: number | null;
  layers: Set<string>;
  search: string;
  debouncedSearch: string;
  runId: string;
  selectedSeq: number | null;
  selectedRunId: string | null;
  flowRunId: string | null;
  flowEvents: AdminEvent[] | null;
  flowSelection: FlowSelection;
  investigation: InvestigateMode | null;
  logsFilter: LogsFilter | null;
};

type Action =
  | { type: "QUERY_LOADED"; events: AdminEvent[]; nextCursor: number | null }
  | { type: "EVENT_RECEIVED"; event: AdminEvent }
  | { type: "TOGGLE_LAYER"; layer: string }
  | { type: "SET_SEARCH"; search: string }
  | { type: "SET_DEBOUNCED_SEARCH"; search: string }
  | { type: "SET_RUN_ID"; runId: string }
  | { type: "LOAD_MORE"; cursor: number }
  | { type: "SELECT_SEQ"; seq: number; runId?: string }
  | { type: "OPEN_FLOW"; runId: string; selection?: FlowSelection }
  | { type: "CLOSE_FLOW" }
  | { type: "FLOW_LOADED"; events: AdminEvent[] }
  | { type: "FLOW_EVENT_RECEIVED"; event: AdminEvent }
  | { type: "SET_FLOW_SELECTION"; selection: FlowSelection }
  | { type: "SET_INVESTIGATION"; investigation: InvestigateMode | null }
  | { type: "SET_LOGS_FILTER"; filter: LogsFilter | null };

function eventsReducer(state: State, action: Action): State {
  switch (action.type) {
    case "QUERY_LOADED": {
      if (state.paginationCursor == null) {
        return { ...state, events: action.events, nextCursor: action.nextCursor };
      }
      const existing = new Set(state.events.map((e) => e.seq));
      const deduped = action.events.filter((e) => !existing.has(e.seq));
      return { ...state, events: [...state.events, ...deduped], nextCursor: action.nextCursor };
    }
    case "EVENT_RECEIVED": {
      if (state.events.some((e) => e.seq === action.event.seq)) return state;
      return { ...state, events: [action.event, ...state.events] };
    }
    case "TOGGLE_LAYER": {
      const next = new Set(state.layers);
      if (next.has(action.layer)) next.delete(action.layer);
      else next.add(action.layer);
      return { ...state, layers: next };
    }
    case "SET_SEARCH":
      return { ...state, search: action.search };
    case "SET_DEBOUNCED_SEARCH": {
      if (action.search === state.debouncedSearch) return state;
      return { ...state, debouncedSearch: action.search, events: [], paginationCursor: null, nextCursor: null };
    }
    case "SET_RUN_ID": {
      if (action.runId === state.runId) return state;
      return {
        ...state,
        runId: action.runId,
        events: [],
        paginationCursor: null,
        nextCursor: null,
        flowRunId: action.runId || null,
        flowEvents: null,
      };
    }
    case "LOAD_MORE":
      return { ...state, paginationCursor: action.cursor };
    case "SELECT_SEQ":
      return { ...state, selectedSeq: action.seq, selectedRunId: action.runId ?? state.selectedRunId };
    case "OPEN_FLOW":
      return {
        ...state,
        runId: action.runId,
        flowRunId: action.runId,
        flowSelection: action.selection ?? null,
        flowEvents: null,
        events: [],
        paginationCursor: null,
        nextCursor: null,
      };
    case "CLOSE_FLOW":
      return { ...state, flowRunId: null, flowSelection: null };
    case "FLOW_LOADED":
      return { ...state, flowEvents: action.events };
    case "FLOW_EVENT_RECEIVED": {
      if (!state.flowEvents) return { ...state, flowEvents: [action.event] };
      if (state.flowEvents.some((e) => e.seq === action.event.seq)) return state;
      return { ...state, flowEvents: [...state.flowEvents, action.event] };
    }
    case "SET_FLOW_SELECTION":
      return { ...state, flowSelection: action.selection };
    case "SET_INVESTIGATION":
      return { ...state, investigation: action.investigation };
    case "SET_LOGS_FILTER":
      return { ...state, logsFilter: action.filter };
    default:
      return state;
  }
}

// ---------------------------------------------------------------------------
// Context shape
// ---------------------------------------------------------------------------

type EventsPaneContextValue = {
  layers: Set<string>;
  toggleLayer: (layer: string) => void;
  search: string;
  setSearch: (v: string) => void;
  runId: string;
  setRunId: (v: string) => void;

  filteredEvents: AdminEvent[];
  loading: boolean;
  hasMore: boolean;
  loadMore: () => void;
  loadingMore: boolean;
  live: boolean;

  selectedSeq: number | null;
  selectSeq: (seq: number, runId?: string) => void;

  treeEvents: AdminEvent[] | null;
  treeLoading: boolean;

  investigation: InvestigateMode | null;
  setInvestigation: (mode: InvestigateMode | null) => void;

  flowRunId: string | null;
  openFlow: (runId: string, initialSelection?: FlowSelection) => void;
  closeFlow: () => void;
  flowData: AdminEvent[] | null;
  flowLoading: boolean;

  flowSelection: FlowSelection;
  setFlowSelection: (sel: FlowSelection) => void;

  logsFilter: LogsFilter | null;
  setLogsFilter: (filter: LogsFilter | null) => void;
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

  const [state, dispatch] = useReducer(eventsReducer, null, () => ({
    events: [] as AdminEvent[],
    nextCursor: null as number | null,
    paginationCursor: null as number | null,
    layers: new Set(searchParams.get("layers")?.split(",").filter(Boolean) ?? [...LAYER_OPTIONS]),
    search: searchParams.get("q") ?? "",
    debouncedSearch: searchParams.get("q") ?? "",
    runId: searchParams.get("runId") ?? "",
    selectedSeq: searchParams.get("seq") ? Number(searchParams.get("seq")) : null,
    selectedRunId: null as string | null,
    flowRunId: searchParams.get("runId") || null,
    flowEvents: null as AdminEvent[] | null,
    flowSelection: parseFlowSelection(searchParams),
    investigation: null as InvestigateMode | null,
    logsFilter: null as LogsFilter | null,
  }));

  // ---------------------------------------------------------------------------
  // Engine: search debounce
  // ---------------------------------------------------------------------------

  useEffect(() => {
    const id = setTimeout(() => dispatch({ type: "SET_DEBOUNCED_SEARCH", search: state.search }), 300);
    return () => clearTimeout(id);
  }, [state.search]);

  // ---------------------------------------------------------------------------
  // Engine: main events query
  // ---------------------------------------------------------------------------

  const queryVars = useMemo(
    () => ({
      limit: 50,
      cursor: state.paginationCursor ?? undefined,
      search: state.debouncedSearch || undefined,
      runId: state.runId || undefined,
    }),
    [state.paginationCursor, state.debouncedSearch, state.runId],
  );

  const { data, loading } = useQuery<{ adminEvents: AdminEventsPage }>(ADMIN_EVENTS, {
    variables: queryVars,
    fetchPolicy: "network-only",
  });

  useEffect(() => {
    if (data?.adminEvents) {
      dispatch({
        type: "QUERY_LOADED",
        events: data.adminEvents.events,
        nextCursor: data.adminEvents.nextCursor,
      });
    }
  }, [data]);

  // ---------------------------------------------------------------------------
  // Engine: live subscription
  // ---------------------------------------------------------------------------

  const [lastSeq, setLastSeq] = useState<number | undefined>(undefined);
  useEffect(() => {
    if (data?.adminEvents?.events?.length && lastSeq === undefined) {
      setLastSeq(data.adminEvents.events[0].seq);
    }
  }, [data, lastSeq]);

  const hasFilters = !!(state.debouncedSearch || state.runId);
  const subscriptionVars = useMemo(() => ({ lastSeq }), [lastSeq]);

  const { data: subData } = useSubscription<{ events: AdminEvent }>(EVENTS_SUBSCRIPTION, {
    variables: subscriptionVars,
    skip: hasFilters && !state.flowRunId,
  });

  useEffect(() => {
    if (!subData?.events) return;
    dispatch({ type: "EVENT_RECEIVED", event: subData.events });
  }, [subData]);

  useEffect(() => {
    if (!subData?.events || !state.flowRunId) return;
    if (subData.events.runId === state.flowRunId) {
      dispatch({ type: "FLOW_EVENT_RECEIVED", event: subData.events });
    }
  }, [subData, state.flowRunId]);

  const live = !hasFilters && !!subData;

  // ---------------------------------------------------------------------------
  // Engine: causal flow
  // ---------------------------------------------------------------------------

  const [fetchFlow, { data: flowQueryData, loading: flowLoading }] = useLazyQuery<{
    adminCausalFlow: { events: AdminEvent[] };
  }>(ADMIN_CAUSAL_FLOW);

  useEffect(() => {
    if (state.flowRunId) {
      fetchFlow({ variables: { runId: state.flowRunId } });
    }
  }, [state.flowRunId, fetchFlow]);

  useEffect(() => {
    if (flowQueryData?.adminCausalFlow?.events) {
      dispatch({ type: "FLOW_LOADED", events: flowQueryData.adminCausalFlow.events });
    }
  }, [flowQueryData]);

  // ---------------------------------------------------------------------------
  // Engine: causal tree
  // ---------------------------------------------------------------------------

  const [fetchTree, { data: treeData, loading: treeLoading }] = useLazyQuery<{
    adminCausalTree: CausalTreeResult;
  }>(ADMIN_CAUSAL_TREE);

  const didHydrateTree = useRef(false);
  useEffect(() => {
    if (!didHydrateTree.current && state.selectedSeq != null) {
      didHydrateTree.current = true;
      fetchTree({ variables: { seq: state.selectedSeq } });
    }
  }, [state.selectedSeq, fetchTree]);

  // ---------------------------------------------------------------------------
  // Selectors — ALL filtering happens here
  // ---------------------------------------------------------------------------

  const filteredEvents = useMemo(() => {
    const needle = state.debouncedSearch?.toLowerCase();
    return state.events.filter((e) => {
      if (!state.layers.has(e.layer)) return false;
      if (state.runId && e.runId !== state.runId) return false;
      if (needle) {
        const matches =
          e.payload.toLowerCase().includes(needle) ||
          e.name.toLowerCase().includes(needle) ||
          (e.runId?.toLowerCase().includes(needle) ?? false) ||
          (e.correlationId?.toLowerCase().includes(needle) ?? false);
        if (!matches) return false;
      }
      return true;
    });
  }, [state.events, state.layers, state.runId, state.debouncedSearch]);

  const hasMore = state.nextCursor != null;

  const effectiveTreeSource = useMemo(() => {
    const effectiveRunId = state.selectedRunId ?? state.flowRunId;
    if (state.flowRunId && state.flowEvents && effectiveRunId === state.flowRunId) {
      return { events: state.flowEvents, source: "flow" as const };
    }
    const tree = treeData?.adminCausalTree;
    if (tree) {
      return { events: tree.events, source: "tree" as const };
    }
    return null;
  }, [state.flowRunId, state.flowEvents, state.selectedRunId, treeData]);

  // ---------------------------------------------------------------------------
  // Callbacks (dispatch into reducer)
  // ---------------------------------------------------------------------------

  const toggleLayer = useCallback((layer: string) => {
    dispatch({ type: "TOGGLE_LAYER", layer });
  }, []);

  const setSearch = useCallback((v: string) => {
    dispatch({ type: "SET_SEARCH", search: v });
  }, []);

  const setRunId = useCallback((v: string) => {
    dispatch({ type: "SET_RUN_ID", runId: v });
  }, []);

  const loadMore = useCallback(() => {
    if (state.nextCursor != null) {
      dispatch({ type: "LOAD_MORE", cursor: state.nextCursor });
    }
  }, [state.nextCursor]);

  const stateRef = useRef(state);
  stateRef.current = state;
  const treeDataRef = useRef(treeData);
  treeDataRef.current = treeData;

  const selectSeq = useCallback(
    (seq: number, evtRunId?: string) => {
      dispatch({ type: "SELECT_SEQ", seq, runId: evtRunId });

      const inTree = treeDataRef.current?.adminCausalTree?.events.some((e) => e.seq === seq);
      const s = stateRef.current;
      const inFlow = s.flowRunId && evtRunId === s.flowRunId && s.flowEvents?.some((e) => e.seq === seq);
      if (!inTree && !inFlow) {
        fetchTree({ variables: { seq } });
      }
    },
    [fetchTree],
  );

  const openFlow = useCallback((id: string, initialSelection?: FlowSelection) => {
    dispatch({ type: "OPEN_FLOW", runId: id, selection: initialSelection });
  }, []);

  const closeFlow = useCallback(() => {
    dispatch({ type: "CLOSE_FLOW" });
    const currentSeq = stateRef.current.selectedSeq;
    if (currentSeq != null) {
      const inTree = treeDataRef.current?.adminCausalTree?.events.some((e) => e.seq === currentSeq);
      if (!inTree) fetchTree({ variables: { seq: currentSeq } });
    }
  }, [fetchTree]);

  const setFlowSelection = useCallback((sel: FlowSelection) => {
    dispatch({ type: "SET_FLOW_SELECTION", selection: sel });
  }, []);

  const setInvestigation = useCallback((inv: InvestigateMode | null) => {
    dispatch({ type: "SET_INVESTIGATION", investigation: inv });
  }, []);

  const setLogsFilter = useCallback((filter: LogsFilter | null) => {
    dispatch({ type: "SET_LOGS_FILTER", filter });
  }, []);

  // ---------------------------------------------------------------------------
  // URL sync
  // ---------------------------------------------------------------------------

  const lastParamsRef = useRef("");
  useEffect(() => {
    const params: Record<string, string> = {};
    const layersStr = [...state.layers].sort().join(",");
    if (layersStr !== "system,telemetry,world") params.layers = layersStr;
    if (state.search) params.q = state.search;
    if (state.runId) params.runId = state.runId;
    if (state.selectedSeq != null) params.seq = String(state.selectedSeq);
    serializeFlowSelection(state.flowSelection, params);
    const serialized = JSON.stringify(params);
    if (serialized !== lastParamsRef.current) {
      lastParamsRef.current = serialized;
      setSearchParams(params, { replace: true });
    }
  }, [state.layers, state.search, state.runId, state.selectedSeq, state.flowSelection, setSearchParams]);

  // ---------------------------------------------------------------------------
  // Context value
  // ---------------------------------------------------------------------------

  const value = useMemo<EventsPaneContextValue>(
    () => ({
      layers: state.layers,
      toggleLayer,
      search: state.search,
      setSearch,
      runId: state.runId,
      setRunId,
      filteredEvents,
      loading,
      hasMore,
      loadMore,
      loadingMore: loading && state.events.length > 0,
      live,
      selectedSeq: state.selectedSeq,
      selectSeq,
      treeEvents: effectiveTreeSource?.events ?? null,
      treeLoading: effectiveTreeSource?.source === "flow" ? flowLoading : treeLoading,
      investigation: state.investigation,
      setInvestigation,
      flowRunId: state.flowRunId,
      openFlow,
      closeFlow,
      flowData: state.flowEvents,
      flowLoading,
      flowSelection: state.flowSelection,
      setFlowSelection,
      logsFilter: state.logsFilter,
      setLogsFilter,
    }),
    [
      state.layers, toggleLayer, state.search, state.runId,
      filteredEvents, loading, hasMore, loadMore, state.events.length,
      live, state.selectedSeq, selectSeq, effectiveTreeSource, treeLoading, flowLoading,
      state.investigation,
      state.flowRunId, openFlow, closeFlow, state.flowEvents,
      state.flowSelection,
      state.logsFilter,
    ],
  );

  return (
    <EventsPaneContext.Provider value={value}>
      {children}
    </EventsPaneContext.Provider>
  );
}
