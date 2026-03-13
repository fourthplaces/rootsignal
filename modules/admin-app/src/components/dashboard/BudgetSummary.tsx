import { Link } from "react-router";

export function BudgetSummary({
  spentCents,
  limitCents,
}: {
  spentCents: number;
  limitCents: number;
}) {
  const pct = limitCents > 0 ? Math.min((spentCents / limitCents) * 100, 100) : 0;
  const color = pct > 85 ? "bg-red-500" : pct > 60 ? "bg-amber-500" : "bg-emerald-500";
  const spent = `$${(spentCents / 100).toFixed(2)}`;
  const limit = limitCents > 0 ? `$${(limitCents / 100).toFixed(2)}` : "No limit";

  return (
    <Link
      to="/budget"
      className="rounded-lg border border-border p-4 hover:bg-accent/50 transition-colors focus-visible:ring-2 ring-ring"
    >
      <p className="text-sm font-medium">Budget</p>
      <p className="text-lg font-semibold tabular-nums mt-1">
        {spent} <span className="text-sm font-normal text-muted-foreground">/ {limit}</span>
      </p>
      {limitCents > 0 && (
        <div
          className="w-full bg-muted rounded-full h-1.5 mt-2"
          role="progressbar"
          aria-valuenow={pct}
          aria-valuemin={0}
          aria-valuemax={100}
        >
          <div className={`${color} h-1.5 rounded-full`} style={{ width: `${pct}%` }} />
        </div>
      )}
    </Link>
  );
}
