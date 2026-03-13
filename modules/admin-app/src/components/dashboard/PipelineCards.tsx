import { Link } from "react-router";

type PipelineStatus = {
  runId: string;
  region: string;
  flowType: string;
  status: string;
  startedAt: string;
  finishedAt: string | null;
  error: string | null;
};

const STATUS_STYLES: Record<string, { shape: string; color: string; label: string }> = {
  completed: { shape: "●", color: "text-emerald-500", label: "OK" },
  running: { shape: "●", color: "text-blue-500", label: "Running" },
  failed: { shape: "■", color: "text-red-500", label: "Failed" },
  cancelled: { shape: "▲", color: "text-amber-500", label: "Cancelled" },
  stale: { shape: "○", color: "text-zinc-400", label: "Stale" },
};

function timeAgo(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const mins = Math.floor(diff / 60_000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

const FLOW_LABELS: Record<string, string> = {
  scout: "Scout",
  coalesce: "Coalesce",
  weave: "Weave",
};

export function PipelineCards({ statuses }: { statuses: PipelineStatus[] }) {
  return (
    <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
      {statuses.map((s) => {
        const style = STATUS_STYLES[s.status] ?? STATUS_STYLES.stale;
        return (
          <Link
            key={s.flowType}
            to={`/workflows/${s.runId}`}
            className="rounded-lg border border-border p-4 hover:bg-accent/50 transition-colors focus-visible:ring-2 ring-ring"
          >
            <div className="flex items-center justify-between">
              <p className="text-sm font-medium">{FLOW_LABELS[s.flowType] ?? s.flowType}</p>
              <span className={`text-sm ${style.color}`} aria-label={style.label}>
                {style.shape} {style.label}
              </span>
            </div>
            <p className="text-xs text-muted-foreground mt-1 tabular-nums">
              {s.finishedAt ? timeAgo(s.finishedAt) : timeAgo(s.startedAt)}
            </p>
            {s.error && (
              <p className="text-xs text-red-400 mt-1 truncate" title={s.error}>
                {s.error}
              </p>
            )}
          </Link>
        );
      })}
    </div>
  );
}
