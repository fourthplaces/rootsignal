function statusDot(count: number): { color: string; shape: string } {
  if (count === 0) return { color: "text-emerald-500", shape: "●" };
  if (count <= 10) return { color: "text-amber-500", shape: "▲" };
  return { color: "text-red-500", shape: "■" };
}

export function QualityScorecard({
  label,
  count,
  onClick,
}: {
  label: string;
  count: number;
  onClick?: () => void;
}) {
  const dot = statusDot(count);

  return (
    <button
      onClick={onClick}
      className="rounded-lg border border-border p-4 text-left hover:bg-accent/50 transition-colors focus-visible:ring-2 ring-ring w-full group"
      aria-label={`${label}: ${count} issues`}
    >
      <div className="flex items-center justify-between">
        <p className="text-xs text-muted-foreground">{label}</p>
        <span className={`text-xs ${dot.color}`} aria-hidden="true">
          {dot.shape}
        </span>
      </div>
      <p className="text-2xl font-semibold tabular-nums mt-1">{count}</p>
      <p className="text-xs text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity mt-1">
        View details →
      </p>
    </button>
  );
}
