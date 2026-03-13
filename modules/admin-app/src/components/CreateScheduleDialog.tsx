import { useState } from "react";
import { useMutation } from "@apollo/client";
import { CREATE_SCHEDULE } from "@/graphql/mutations";
import { useRegion } from "@/contexts/RegionContext";

const FLOW_TYPES = ["scrape", "coalesce", "weave", "bootstrap"] as const;
const CADENCE_UNITS = [
  { label: "minutes", seconds: 60 },
  { label: "hours", seconds: 3600 },
  { label: "days", seconds: 86400 },
] as const;

export function CreateScheduleDialog({ onClose }: { onClose: () => void }) {
  const { regionId, regionName } = useRegion();
  const [flowType, setFlowType] = useState<string>("scrape");
  const [cadenceValue, setCadenceValue] = useState(24);
  const [cadenceUnit, setCadenceUnit] = useState(3600); // hours
  const [chain, setChain] = useState(false);
  const [createSchedule, { loading }] = useMutation(CREATE_SCHEDULE);

  const handleCreate = async () => {
    if (!regionId) return;
    const scope: Record<string, unknown> = {};
    if (chain) scope.chain = true;
    await createSchedule({
      variables: {
        flowType,
        scope: JSON.stringify(scope),
        cadenceSeconds: cadenceValue * cadenceUnit,
        regionId,
      },
    });
    onClose();
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-card border border-border rounded-lg p-6 w-full max-w-sm space-y-4">
        <h2 className="font-semibold">New Schedule</h2>
        <p className="text-sm text-muted-foreground">
          Region: <span className="text-foreground">{regionName}</span>
        </p>

        <div className="space-y-1">
          <label className="text-xs text-muted-foreground">Flow Type</label>
          <select
            value={flowType}
            onChange={(e) => setFlowType(e.target.value)}
            className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm"
          >
            {FLOW_TYPES.map((ft) => (
              <option key={ft} value={ft}>{ft}</option>
            ))}
          </select>
        </div>

        <div className="space-y-1">
          <label className="text-xs text-muted-foreground">Cadence</label>
          <div className="flex gap-2">
            <input
              type="number"
              min={1}
              value={cadenceValue}
              onChange={(e) => setCadenceValue(Number(e.target.value))}
              className="w-20 px-3 py-2 rounded-md border border-input bg-background text-sm tabular-nums"
            />
            <select
              value={cadenceUnit}
              onChange={(e) => setCadenceUnit(Number(e.target.value))}
              className="flex-1 px-3 py-2 rounded-md border border-input bg-background text-sm"
            >
              {CADENCE_UNITS.map((u) => (
                <option key={u.seconds} value={u.seconds}>{u.label}</option>
              ))}
            </select>
          </div>
        </div>

        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={chain}
            onChange={(e) => setChain(e.target.checked)}
            className="rounded border-input"
          />
          <span className="text-muted-foreground">Chain downstream flows</span>
        </label>

        <div className="flex gap-2 justify-end">
          <button
            onClick={onClose}
            className="px-3 py-1.5 rounded-md border border-border text-sm text-muted-foreground hover:text-foreground"
          >
            Cancel
          </button>
          <button
            onClick={handleCreate}
            disabled={loading || !regionId}
            className="px-3 py-1.5 rounded-md text-sm text-white bg-primary hover:bg-primary/90 disabled:opacity-50"
          >
            {loading ? "Creating..." : "Create"}
          </button>
        </div>
      </div>
    </div>
  );
}
