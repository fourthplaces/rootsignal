import { useEventsPaneContext } from "../EventsPaneContext";
import { InvestigateDrawer } from "@/components/InvestigateDrawer";

export function InvestigatePane() {
  const { investigateEvent } = useEventsPaneContext();

  if (!investigateEvent) {
    return (
      <div className="flex items-center justify-center h-full text-sm text-muted-foreground">
        Click the investigate icon on an event to start
      </div>
    );
  }

  return (
    <InvestigateDrawer
      key={investigateEvent.seq}
      event={investigateEvent}
      onClose={() => {
        // Tab close (X) is the primary close mechanism.
        // This onClose is kept for the internal close button which
        // we preserve for now — it provides a secondary close path.
      }}
    />
  );
}
