import { useCallback, useRef, useEffect } from "react";
import { Actions } from "flexlayout-react";
import type { Model, Action } from "flexlayout-react";
import { PaneManager, type PaneType, type PaneManagerHandle } from "@/components/PaneManager";
import { EventsPaneProvider, useEventsPaneContext } from "./EventsPaneContext";
import { TimelinePane } from "./panes/TimelinePane";
import { CausalTreePane } from "./panes/CausalTreePane";
import { CausalFlowPane } from "./panes/CausalFlowPane";
import { InvestigatePane } from "./panes/InvestigatePane";
import { LogsPane } from "./panes/LogsPane";
import { DEFAULT_EVENTS_LAYOUT } from "./defaultLayout";

// ---------------------------------------------------------------------------
// Pane registry
// ---------------------------------------------------------------------------

const PANE_REGISTRY: PaneType[] = [
  { name: "Timeline", component: "timeline", render: () => <TimelinePane /> },
  { name: "Causal Tree", component: "causal-tree", render: () => <CausalTreePane /> },
  { name: "Flow", component: "causal-flow", render: () => <CausalFlowPane /> },
  { name: "Investigate", component: "investigate", render: () => <InvestigatePane /> },
  { name: "Logs", component: "logs", render: () => <LogsPane /> },
];

// ---------------------------------------------------------------------------
// Inner component (has access to context)
// ---------------------------------------------------------------------------

function EventsPageInner() {
  const { selectedSeq, investigation, setInvestigation, flowRunId, logsFilter, setLogsFilter } = useEventsPaneContext();
  const paneManagerRef = useRef<PaneManagerHandle>(null);

  // Auto-open causal tree tab when an event is selected
  useEffect(() => {
    if (selectedSeq == null || !paneManagerRef.current) return;
    const pm = paneManagerRef.current;
    if (!pm.hasTab("causal-tree")) {
      pm.addTab("causal-tree", "Causal Tree");
    }
  }, [selectedSeq]);

  // Auto-open flow tab when flowRunId is set
  useEffect(() => {
    if (!flowRunId || !paneManagerRef.current) return;
    const pm = paneManagerRef.current;
    if (pm.hasTab("causal-flow")) {
      pm.selectTab("causal-flow");
    } else {
      pm.addTab("causal-flow", "Flow");
    }
  }, [flowRunId]);

  // Auto-open investigate tab when investigation is set
  useEffect(() => {
    if (!investigation || !paneManagerRef.current) return;
    const pm = paneManagerRef.current;
    if (pm.hasTab("investigate")) {
      pm.selectTab("investigate");
    } else {
      pm.addTab("investigate", "Investigate");
    }
  }, [investigation]);

  // Auto-open logs tab when logsFilter is set
  useEffect(() => {
    if (!logsFilter || !paneManagerRef.current) return;
    const pm = paneManagerRef.current;
    if (pm.hasTab("logs")) {
      pm.selectTab("logs");
    } else {
      pm.addTab("logs", "Logs");
    }
  }, [logsFilter]);

  const handleModelChange = useCallback(
    (model: Model, action: Action) => {
      // If an investigate tab was closed, clear the investigate event
      if (action.type === Actions.DELETE_TAB) {
        let hasInvestigateTab = false;
        let hasLogsTab = false;
        model.visitNodes((node) => {
          if ("getComponent" in node) {
            const comp = (node as any).getComponent();
            if (comp === "investigate") hasInvestigateTab = true;
            if (comp === "logs") hasLogsTab = true;
          }
        });
        if (!hasInvestigateTab) setInvestigation(null);
        if (!hasLogsTab) setLogsFilter(null);
      }
    },
    [setInvestigation, setLogsFilter],
  );

  return (
    <div className="h-[calc(100vh-3rem)] -m-6">
      <PaneManager
        ref={paneManagerRef}
        defaultLayout={DEFAULT_EVENTS_LAYOUT as any}
        paneRegistry={PANE_REGISTRY}
        storageKey="events-pane-layout"
        onModelChange={handleModelChange}
      />
    </div>
  );
}

// ---------------------------------------------------------------------------
// EventsPage (public export)
// ---------------------------------------------------------------------------

export function EventsPage() {
  return (
    <EventsPaneProvider>
      <EventsPageInner />
    </EventsPaneProvider>
  );
}
