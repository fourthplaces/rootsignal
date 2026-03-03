import { useCallback, useRef, useEffect } from "react";
import { Actions } from "flexlayout-react";
import type { Model, Action } from "flexlayout-react";
import { PaneManager, type PaneType, type PaneManagerHandle } from "@/components/PaneManager";
import { EventsPaneProvider, useEventsPaneContext } from "./EventsPaneContext";
import { TimelinePane } from "./panes/TimelinePane";
import { CausalTreePane } from "./panes/CausalTreePane";
import { InvestigatePane } from "./panes/InvestigatePane";
import { DEFAULT_EVENTS_LAYOUT } from "./defaultLayout";

// ---------------------------------------------------------------------------
// Pane registry
// ---------------------------------------------------------------------------

const PANE_REGISTRY: PaneType[] = [
  { name: "Timeline", component: "timeline", render: () => <TimelinePane /> },
  { name: "Causal Tree", component: "causal-tree", render: () => <CausalTreePane /> },
  { name: "Investigate", component: "investigate", render: () => <InvestigatePane /> },
];

// ---------------------------------------------------------------------------
// Inner component (has access to context)
// ---------------------------------------------------------------------------

function EventsPageInner() {
  const { selectedSeq, investigateEvent, setInvestigateEvent } = useEventsPaneContext();
  const paneManagerRef = useRef<PaneManagerHandle>(null);

  // Auto-open causal tree tab when an event is selected
  useEffect(() => {
    if (selectedSeq == null || !paneManagerRef.current) return;
    const pm = paneManagerRef.current;
    if (!pm.hasTab("causal-tree")) {
      pm.addTab("causal-tree", "Causal Tree");
    }
  }, [selectedSeq]);

  // Auto-open investigate tab when investigateEvent is set
  useEffect(() => {
    if (!investigateEvent || !paneManagerRef.current) return;
    const pm = paneManagerRef.current;
    if (pm.hasTab("investigate")) {
      pm.selectTab("investigate");
    } else {
      pm.addTab("investigate", "Investigate");
    }
  }, [investigateEvent]);

  const handleModelChange = useCallback(
    (model: Model, action: Action) => {
      // If an investigate tab was closed, clear the investigate event
      if (action.type === Actions.DELETE_TAB) {
        let hasInvestigateTab = false;
        model.visitNodes((node) => {
          if ("getComponent" in node && (node as any).getComponent() === "investigate") {
            hasInvestigateTab = true;
          }
        });
        if (!hasInvestigateTab) {
          setInvestigateEvent(null);
        }
      }
    },
    [setInvestigateEvent],
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
