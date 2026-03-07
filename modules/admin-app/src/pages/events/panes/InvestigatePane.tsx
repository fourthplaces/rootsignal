import { useEventsPaneContext } from "../EventsPaneContext";
import { InvestigateDrawer } from "@/components/InvestigateDrawer";

function investigationKey(investigation: NonNullable<ReturnType<typeof useEventsPaneContext>["investigation"]>): string {
  switch (investigation.mode) {
    case "event":
      return `event-${investigation.event.seq}`;
    case "logs":
      return `logs-${investigation.runId ?? ""}-${investigation.handlerId ?? ""}`;
    case "sources":
      return `sources-${investigation.sourceIds.join(",")}`;
    case "scout_run":
      return `run-${investigation.runId}`;
    case "source_dive":
      return `source-${investigation.sourceId}`;
  }
}

export function InvestigatePane() {
  const { investigation } = useEventsPaneContext();

  if (!investigation) {
    return (
      <div className="flex items-center justify-center h-full text-sm text-muted-foreground">
        Click investigate on an event or logs to start
      </div>
    );
  }

  return (
    <InvestigateDrawer
      key={investigationKey(investigation)}
      investigation={investigation}
      onClose={() => {
        // Tab close (X) is the primary close mechanism.
        // This onClose is kept for the internal close button which
        // we preserve for now — it provides a secondary close path.
      }}
    />
  );
}
