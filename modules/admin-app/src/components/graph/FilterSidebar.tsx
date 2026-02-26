import { useMemo } from "react";

const NODE_TYPE_OPTIONS = [
  { key: "Gathering", label: "Gathering" },
  { key: "Aid", label: "Aid" },
  { key: "Need", label: "Need" },
  { key: "Notice", label: "Notice" },
  { key: "Tension", label: "Tension" },
  { key: "Actor", label: "Actor" },
  { key: "Citation", label: "Citation" },
] as const;

type NodeMetadata = {
  id: string;
  metadata: string;
};

export function FilterSidebar({
  nodeTypes,
  onToggleNodeType,
  maxNodes,
  onMaxNodesChange,
  timeFrom,
  timeTo,
  onTimeFromChange,
  onTimeToChange,
  search,
  onSearchChange,
  totalCount,
  visibleCount,
  nodeCounts,
  allNodes,
}: {
  nodeTypes: Set<string>;
  onToggleNodeType: (type: string) => void;
  maxNodes: number;
  onMaxNodesChange: (n: number) => void;
  timeFrom: string;
  timeTo: string;
  onTimeFromChange: (d: string) => void;
  onTimeToChange: (d: string) => void;
  search: string;
  onSearchChange: (s: string) => void;
  totalCount: number;
  visibleCount: number;
  nodeCounts: Record<string, number>;
  allNodes: NodeMetadata[];
}) {
  // Compute daily activity histogram from node metadata extractedAt
  const histogram = useMemo(() => {
    const counts = new Map<string, number>();
    for (const node of allNodes) {
      try {
        const meta = JSON.parse(node.metadata);
        const dateStr = (meta.extractedAt ?? meta.firstSeen ?? "").slice(0, 10);
        if (dateStr) {
          counts.set(dateStr, (counts.get(dateStr) ?? 0) + 1);
        }
      } catch {
        // skip
      }
    }
    if (counts.size === 0) return [];

    // Generate all days in the time range
    const start = new Date(timeFrom);
    const end = new Date(timeTo);
    const days: { day: string; count: number }[] = [];
    const d = new Date(start);
    while (d <= end) {
      const key = d.toISOString().slice(0, 10);
      days.push({ day: key, count: counts.get(key) ?? 0 });
      d.setDate(d.getDate() + 1);
    }
    return days;
  }, [allNodes, timeFrom, timeTo]);

  const maxCount = useMemo(
    () => Math.max(1, ...histogram.map((d) => d.count)),
    [histogram],
  );

  return (
    <div className="w-56 shrink-0 border-l border-border bg-card p-3 space-y-4 overflow-y-auto text-sm">
      <h2 className="font-semibold text-xs uppercase tracking-wider text-muted-foreground">
        Explorer
      </h2>

      {/* Search */}
      <div>
        <input
          type="text"
          placeholder="Search nodes..."
          value={search}
          onChange={(e) => onSearchChange(e.target.value)}
          className="w-full px-2 py-1.5 rounded border border-input bg-background text-xs"
        />
      </div>

      {/* Time window with histogram */}
      <div className="space-y-1.5">
        <label className="text-xs text-muted-foreground font-medium">Time window</label>

        {/* Activity histogram */}
        {histogram.length > 0 && (
          <div className="flex items-end gap-px h-8 px-0.5">
            {histogram.map((d) => (
              <div
                key={d.day}
                className="flex-1 bg-indigo-500/40 rounded-t-sm min-w-[2px] transition-all"
                style={{ height: `${(d.count / maxCount) * 100}%` }}
                title={`${d.day}: ${d.count} nodes`}
              />
            ))}
          </div>
        )}

        <div className="space-y-1">
          <input
            type="date"
            value={timeFrom}
            onChange={(e) => onTimeFromChange(e.target.value)}
            className="w-full px-2 py-1 rounded border border-input bg-background text-xs"
          />
          <input
            type="date"
            value={timeTo}
            onChange={(e) => onTimeToChange(e.target.value)}
            className="w-full px-2 py-1 rounded border border-input bg-background text-xs"
          />
        </div>
      </div>

      {/* Max nodes */}
      <div className="space-y-1.5">
        <label className="text-xs text-muted-foreground font-medium">
          Max nodes: {maxNodes}
        </label>
        <input
          type="range"
          min={25}
          max={500}
          step={25}
          value={maxNodes}
          onChange={(e) => onMaxNodesChange(Number(e.target.value))}
          className="w-full"
        />
        <div className="text-[10px] text-muted-foreground">
          Showing {visibleCount} of {totalCount}
        </div>
      </div>

      {/* Node types */}
      <div className="space-y-1.5">
        <label className="text-xs text-muted-foreground font-medium">Node types</label>
        {NODE_TYPE_OPTIONS.map((opt) => (
          <label key={opt.key} className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={nodeTypes.has(opt.key)}
              onChange={() => onToggleNodeType(opt.key)}
              className="rounded border-border"
            />
            <span className="text-xs">{opt.label}</span>
            <span className="text-[10px] text-muted-foreground ml-auto tabular-nums">
              {nodeCounts[opt.key] ?? 0}
            </span>
          </label>
        ))}
      </div>
    </div>
  );
}
