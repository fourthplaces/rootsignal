import { Link } from "react-router";

type HottestConcern = {
  id: string;
  title: string;
  category: string | null;
  causeHeat: number;
  corroborationCount: number;
};

function HeatBar({ value }: { value: number }) {
  const pct = Math.min(value * 100, 100);
  return (
    <div className="flex items-center gap-2">
      <div className="w-16 bg-muted rounded-full h-1.5">
        <div className="bg-red-500/70 h-1.5 rounded-full" style={{ width: `${pct}%` }} />
      </div>
      <span className="text-xs tabular-nums text-muted-foreground">{value.toFixed(2)}</span>
    </div>
  );
}

export function HottestConcernsTable({ concerns }: { concerns: HottestConcern[] }) {
  if (concerns.length === 0) {
    return <p className="text-sm text-muted-foreground">No active concerns</p>;
  }

  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b border-border text-left text-muted-foreground">
            <th className="pb-2 font-medium">Concern</th>
            <th className="pb-2 font-medium">Category</th>
            <th className="pb-2 font-medium">Heat</th>
            <th className="pb-2 font-medium text-right">Sources</th>
          </tr>
        </thead>
        <tbody>
          {concerns.map((c) => (
            <tr key={c.id} className="border-b border-border/50">
              <td className="py-2">
                <Link to={`/signals/${c.id}`} className="hover:underline">
                  {c.title}
                </Link>
              </td>
              <td className="py-2 text-muted-foreground">{c.category ?? "—"}</td>
              <td className="py-2">
                <HeatBar value={c.causeHeat} />
              </td>
              <td className="py-2 text-right tabular-nums">{c.corroborationCount}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
