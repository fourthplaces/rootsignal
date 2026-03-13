import { useState, useMemo } from "react";
import { useQuery, useMutation } from "@apollo/client";
import { SCHEDULES_FOR_ENTITY } from "@/graphql/queries";
import {
  CREATE_SCHEDULE,
  TOGGLE_SCHEDULE,
  DELETE_SCHEDULE,
  UPDATE_SCHEDULE_CADENCE,
} from "@/graphql/mutations";
import { DataTable, type Column } from "@/components/DataTable";
import { formatCadence } from "@/lib/utils";

type Schedule = {
  scheduleId: string;
  flowType: string;
  scope: string;
  cadenceSeconds: number;
  baseCadenceSeconds: number;
  recurring: boolean;
  enabled: boolean;
  lastRunId: string | null;
  nextRunAt: string | null;
  createdAt: string;
  regionId: string | null;
};

type EntityType = "source" | "region" | "cluster";

const COMPATIBLE_FLOWS: Record<EntityType, { key: string; label: string }[]> = {
  source: [{ key: "scout_source", label: "Scout Source" }],
  region: [
    { key: "scrape", label: "Scrape" },
    { key: "bootstrap", label: "Bootstrap" },
    { key: "weave", label: "Weave" },
    { key: "coalesce", label: "Coalesce" },
  ],
  cluster: [{ key: "group_feed", label: "Group Feed" }],
};

const CADENCE_UNITS = [
  { label: "minutes", seconds: 60 },
  { label: "hours", seconds: 3600 },
  { label: "days", seconds: 86400 },
] as const;

const EMPTY_MESSAGES: Record<EntityType, string> = {
  source: "No schedules. Add a scout schedule to automatically monitor this source.",
  region: "No schedules. Add schedules to automate scraping, bootstrapping, weaving, or coalescing.",
  cluster: "No schedules.",
};

function formatDate(d: string): string {
  return new Date(d).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function buildScope(entityType: EntityType, entityId: string, chain?: boolean): string {
  const scope: Record<string, unknown> = {};
  if (entityType === "source") scope.source_ids = [entityId];
  if (entityType === "cluster") scope.group_id = entityId;
  if (chain) scope.chain = true;
  return JSON.stringify(scope);
}

function parseCadence(seconds: number): { value: number; unit: number } {
  if (seconds >= 86400 && seconds % 86400 === 0) return { value: seconds / 86400, unit: 86400 };
  if (seconds >= 3600 && seconds % 3600 === 0) return { value: seconds / 3600, unit: 3600 };
  return { value: Math.round(seconds / 60), unit: 60 };
}

function InlineCadenceEditor({
  cadenceSeconds,
  onSave,
}: {
  cadenceSeconds: number;
  onSave: (seconds: number) => void;
}) {
  const initial = parseCadence(cadenceSeconds);
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(initial.value);
  const [unit, setUnit] = useState(initial.unit);

  if (!editing) {
    return (
      <button
        onClick={() => setEditing(true)}
        className="text-xs tabular-nums hover:text-foreground hover:underline cursor-pointer"
        title="Click to edit cadence"
      >
        {formatCadence(cadenceSeconds)}
      </button>
    );
  }

  const handleSave = () => {
    const newSeconds = value * unit;
    if (newSeconds > 0 && newSeconds !== cadenceSeconds) {
      onSave(newSeconds);
    }
    setEditing(false);
  };

  return (
    <span className="inline-flex gap-1 items-center">
      <input
        type="number"
        min={1}
        value={value}
        onChange={(e) => setValue(Number(e.target.value))}
        onKeyDown={(e) => {
          if (e.key === "Enter") handleSave();
          if (e.key === "Escape") setEditing(false);
        }}
        autoFocus
        className="w-14 px-1.5 py-0.5 rounded border border-input bg-background text-xs tabular-nums"
      />
      <select
        value={unit}
        onChange={(e) => setUnit(Number(e.target.value))}
        className="px-1 py-0.5 rounded border border-input bg-background text-xs"
      >
        {CADENCE_UNITS.map((u) => (
          <option key={u.seconds} value={u.seconds}>{u.label}</option>
        ))}
      </select>
      <button
        onClick={handleSave}
        className="text-xs px-1.5 py-0.5 rounded bg-primary text-white hover:bg-primary/90"
      >
        Save
      </button>
      <button
        onClick={() => setEditing(false)}
        className="text-xs px-1.5 py-0.5 rounded border border-border text-muted-foreground hover:text-foreground"
      >
        Cancel
      </button>
    </span>
  );
}

function AddScheduleButton({
  entityType,
  entityId,
  regionId,
  existingFlowTypes,
  onCreated,
}: {
  entityType: EntityType;
  entityId: string;
  regionId?: string;
  existingFlowTypes: Set<string>;
  onCreated: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [flowType, setFlowType] = useState("");
  const [cadenceValue, setCadenceValue] = useState(24);
  const [cadenceUnit, setCadenceUnit] = useState(3600);
  const [chain, setChain] = useState(false);
  const [createSchedule, { loading }] = useMutation(CREATE_SCHEDULE);

  const availableFlows = COMPATIBLE_FLOWS[entityType].filter(
    (f) => !existingFlowTypes.has(f.key)
  );

  if (availableFlows.length === 0) return null;

  const handleOpen = () => {
    setFlowType(availableFlows[0]!.key);
    setCadenceValue(24);
    setCadenceUnit(3600);
    setChain(false);
    setOpen(true);
  };

  const handleCreate = async () => {
    await createSchedule({
      variables: {
        flowType,
        scope: buildScope(entityType, entityId, chain),
        cadenceSeconds: cadenceValue * cadenceUnit,
        regionId: entityType === "region" ? entityId : regionId,
      },
    });
    setOpen(false);
    onCreated();
  };

  return (
    <>
      <button
        onClick={handleOpen}
        className="text-xs px-2.5 py-1 rounded-md border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50"
      >
        + Add
      </button>
      {open && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
          onClick={(e) => { if (e.target === e.currentTarget) setOpen(false); }}
          onKeyDown={(e) => { if (e.key === "Escape") setOpen(false); }}
        >
          <div className="w-80 rounded-lg border border-border bg-background p-5 shadow-xl space-y-4">
            <h4 className="text-sm font-medium">Add Schedule</h4>

            <label className="block space-y-1">
              <span className="text-xs text-muted-foreground">Flow type</span>
              <select
                value={flowType}
                onChange={(e) => setFlowType(e.target.value)}
                className="w-full px-2.5 py-1.5 rounded-md border border-input bg-background text-sm"
              >
                {availableFlows.map((f) => (
                  <option key={f.key} value={f.key}>{f.label}</option>
                ))}
              </select>
            </label>

            <label className="block space-y-1">
              <span className="text-xs text-muted-foreground">Cadence</span>
              <div className="flex gap-2">
                <input
                  type="number"
                  min={1}
                  value={cadenceValue}
                  onChange={(e) => setCadenceValue(Number(e.target.value))}
                  className="w-20 px-2.5 py-1.5 rounded-md border border-input bg-background text-sm tabular-nums"
                />
                <select
                  value={cadenceUnit}
                  onChange={(e) => setCadenceUnit(Number(e.target.value))}
                  className="flex-1 px-2.5 py-1.5 rounded-md border border-input bg-background text-sm"
                >
                  {CADENCE_UNITS.map((u) => (
                    <option key={u.seconds} value={u.seconds}>{u.label}</option>
                  ))}
                </select>
              </div>
            </label>

            {entityType === "region" && (
              <label className="flex items-center gap-2 text-sm text-muted-foreground">
                <input
                  type="checkbox"
                  checked={chain}
                  onChange={(e) => setChain(e.target.checked)}
                  className="rounded border-input"
                />
                Chain subsequent flows
              </label>
            )}

            <div className="flex justify-end gap-2 pt-1">
              <button
                onClick={() => setOpen(false)}
                className="text-xs px-3 py-1.5 rounded-md border border-border text-muted-foreground hover:text-foreground"
              >
                Cancel
              </button>
              <button
                onClick={handleCreate}
                disabled={loading}
                className="text-xs px-3 py-1.5 rounded-md text-white bg-primary hover:bg-primary/90 disabled:opacity-50"
              >
                {loading ? "Creating..." : "Create"}
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}

export function SchedulesPanel({
  entityType,
  entityId,
  regionId,
}: {
  entityType: EntityType;
  entityId: string;
  regionId?: string;
}) {
  const { data, loading, refetch } = useQuery(SCHEDULES_FOR_ENTITY, {
    variables: { entityType, entityId },
  });
  const [toggleSchedule] = useMutation(TOGGLE_SCHEDULE);
  const [deleteSchedule] = useMutation(DELETE_SCHEDULE);
  const [updateCadence] = useMutation(UPDATE_SCHEDULE_CADENCE);

  const schedules: Schedule[] = data?.schedulesForEntity ?? [];
  const existingFlowTypes = useMemo(
    () => new Set(schedules.map((s) => s.flowType)),
    [schedules]
  );

  const columns: Column<Schedule>[] = [
    {
      key: "flowType",
      label: "Flow",
      render: (s) => (
        <span className="text-xs px-2 py-0.5 rounded-full bg-blue-500/10 text-blue-400">
          {s.flowType}
        </span>
      ),
    },
    {
      key: "cadenceSeconds",
      label: "Cadence",
      render: (s) => (
        <span className="inline-flex items-center gap-1.5">
          <InlineCadenceEditor
            cadenceSeconds={s.cadenceSeconds}
            onSave={async (newSeconds) => {
              await updateCadence({
                variables: { scheduleId: s.scheduleId, cadenceSeconds: newSeconds },
              });
              refetch();
            }}
          />
          {s.cadenceSeconds !== s.baseCadenceSeconds && (
            <span
              className="text-[9px] px-1 py-0.5 rounded bg-amber-500/10 text-amber-400 border border-amber-500/20"
              title={`Base: ${formatCadence(s.baseCadenceSeconds)} — backoff active`}
            >
              backoff
            </span>
          )}
        </span>
      ),
    },
    {
      key: "enabled",
      label: "Status",
      render: (s) => (
        <button
          onClick={async () => {
            await toggleSchedule({
              variables: { scheduleId: s.scheduleId, enabled: !s.enabled },
            });
            refetch();
          }}
          className={`text-xs px-2 py-0.5 rounded-full border cursor-pointer ${
            s.enabled
              ? "bg-green-500/10 text-green-400 border-green-500/20 hover:bg-green-500/20"
              : "bg-muted text-muted-foreground border-border hover:bg-accent/50"
          }`}
        >
          {s.enabled ? "Enabled" : "Disabled"}
        </button>
      ),
    },
    {
      key: "nextRunAt",
      label: "Next Run",
      render: (s) => (
        <span className="text-muted-foreground whitespace-nowrap text-xs">
          {s.nextRunAt ? formatDate(s.nextRunAt) : "\u2014"}
        </span>
      ),
    },
    {
      key: "createdAt",
      label: "Created",
      render: (s) => (
        <span className="text-muted-foreground whitespace-nowrap text-xs">
          {formatDate(s.createdAt)}
        </span>
      ),
    },
    {
      key: "actions",
      label: "",
      sortable: false,
      align: "right",
      render: (s) => (
        <button
          onClick={async () => {
            await deleteSchedule({ variables: { scheduleId: s.scheduleId } });
            refetch();
          }}
          className="text-xs px-2 py-1 rounded border border-red-500/30 text-red-400 hover:bg-red-500/10"
        >
          Delete
        </button>
      ),
    },
  ];

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium">Schedules</h3>
        <AddScheduleButton
          entityType={entityType}
          entityId={entityId}
          regionId={regionId}
          existingFlowTypes={existingFlowTypes}
          onCreated={() => refetch()}
        />
      </div>

      <DataTable<Schedule>
        columns={columns}
        data={schedules}
        getRowKey={(s) => s.scheduleId}
        loading={loading}
        emptyMessage={EMPTY_MESSAGES[entityType]}
      />
    </div>
  );
}
