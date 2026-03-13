import { Link } from "react-router";
import { useQuery } from "@apollo/client";
import { SITUATIONS } from "@/graphql/queries";
import { DataTable, type Column } from "@/components/DataTable";

const ARC_COLORS: Record<string, string> = {
  EMERGING: "bg-blue-500/20 text-blue-300",
  DEVELOPING: "bg-green-500/20 text-green-300",
  ACTIVE: "bg-orange-500/20 text-orange-300",
  COOLING: "bg-gray-500/20 text-gray-300",
  COLD: "bg-gray-500/20 text-gray-500",
};

type Situation = {
  id: string;
  headline: string;
  arc: string;
  temperature: number;
  clarity: string;
  signalCount: number;
  dispatchCount: number;
  locationName: string | null;
};

const columns: Column<Situation>[] = [
  {
    key: "headline",
    label: "Headline",
    render: (s) => (
      <span>
        <Link to={`/situations/${s.id}`} className="hover:underline line-clamp-1">
          {s.headline}
        </Link>
        {s.locationName && (
          <span className="text-xs text-muted-foreground ml-2">{s.locationName}</span>
        )}
      </span>
    ),
  },
  {
    key: "arc",
    label: "Arc",
    render: (s) => (
      <span className={`px-2 py-0.5 rounded-full text-xs ${ARC_COLORS[s.arc] ?? "bg-secondary"}`}>
        {s.arc}
      </span>
    ),
  },
  {
    key: "temperature",
    label: "Temp",
    render: (s) => <span className="font-mono tabular-nums">{s.temperature.toFixed(2)}</span>,
  },
  {
    key: "clarity",
    label: "Clarity",
    render: (s) => <span className="text-muted-foreground">{s.clarity}</span>,
  },
  {
    key: "signalCount",
    label: "Signals",
    align: "right",
    render: (s) => <span className="tabular-nums">{s.signalCount}</span>,
  },
  {
    key: "dispatchCount",
    label: "Dispatches",
    align: "right",
    render: (s) => <span className="tabular-nums">{s.dispatchCount}</span>,
  },
];

export function SituationsPage() {
  const { data, loading } = useQuery(SITUATIONS, {
    variables: { limit: 50 },
  });

  const situations: Situation[] = data?.situations ?? [];

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-3">
        <h1 className="text-xl font-semibold">Situations</h1>
        <span className="text-sm text-muted-foreground">({situations.length})</span>
      </div>

      <DataTable<Situation>
        columns={columns}
        data={situations}
        getRowKey={(s) => s.id}
        loading={loading}
        emptyMessage="No situations found."
      />
    </div>
  );
}
