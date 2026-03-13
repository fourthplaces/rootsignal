import { Link } from "react-router";

export function DomainCountCard({
  label,
  count,
  to,
}: {
  label: string;
  count: number;
  to: string;
}) {
  return (
    <Link
      to={to}
      className="rounded-lg border border-border p-4 hover:bg-accent/50 transition-colors focus-visible:ring-2 ring-ring"
    >
      <p className="text-xs text-muted-foreground">{label}</p>
      <p className="text-2xl font-semibold tabular-nums mt-1">{count}</p>
    </Link>
  );
}
