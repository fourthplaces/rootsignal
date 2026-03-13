import { Link, useParams } from "react-router";
import { useQuery } from "@apollo/client";
import { SITUATION_DETAIL } from "@/graphql/queries";
import { DataTable, type Column } from "@/components/DataTable";

const ARC_COLORS: Record<string, string> = {
  EMERGING: "bg-blue-500/20 text-blue-300",
  DEVELOPING: "bg-green-500/20 text-green-300",
  ACTIVE: "bg-orange-500/20 text-orange-300",
  COOLING: "bg-gray-500/20 text-gray-300",
  COLD: "bg-gray-500/20 text-gray-500",
};

const formatDate = (d: string | null | undefined) => {
  if (!d) return "—";
  return new Date(d).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
};

type Dispatch = {
  id: string;
  body: string;
  signalIds: string[];
  createdAt: string;
  dispatchType: string;
  supersedes: string | null;
  flaggedForReview: boolean;
  flagReason: string | null;
  fidelityScore: number | null;
};

const DISPATCH_TYPE_COLORS: Record<string, string> = {
  EMERGENCE: "bg-blue-500/10 text-blue-400 border-blue-500/20",
  UPDATE: "bg-green-500/10 text-green-400 border-green-500/20",
  CORRECTION: "bg-amber-500/10 text-amber-400 border-amber-500/20",
  SPLIT: "bg-purple-500/10 text-purple-400 border-purple-500/20",
  MERGE: "bg-indigo-500/10 text-indigo-400 border-indigo-500/20",
  REACTIVATION: "bg-orange-500/10 text-orange-400 border-orange-500/20",
};

const dispatchColumns: Column<Dispatch>[] = [
  {
    key: "dispatchType",
    label: "Type",
    render: (d) => (
      <span
        className={`px-2 py-0.5 rounded-full text-xs border ${DISPATCH_TYPE_COLORS[d.dispatchType] ?? "bg-muted text-muted-foreground border-border"}`}
      >
        {d.dispatchType}
      </span>
    ),
  },
  {
    key: "body",
    label: "Body",
    render: (d) => (
      <span className="line-clamp-2 text-sm">{d.body}</span>
    ),
  },
  {
    key: "signalIds",
    label: "Signals",
    align: "right",
    render: (d) => <span className="tabular-nums">{d.signalIds.length}</span>,
  },
  {
    key: "fidelityScore",
    label: "Fidelity",
    align: "right",
    render: (d) =>
      d.fidelityScore != null ? (
        <span className="tabular-nums">{(d.fidelityScore * 100).toFixed(0)}%</span>
      ) : (
        <span className="text-muted-foreground">—</span>
      ),
  },
  {
    key: "flaggedForReview",
    label: "Flag",
    render: (d) =>
      d.flaggedForReview ? (
        <span className="text-amber-400 text-xs" title={d.flagReason ?? undefined}>
          Flagged
        </span>
      ) : null,
  },
  {
    key: "createdAt",
    label: "Created",
    render: (d) => (
      <span className="text-muted-foreground whitespace-nowrap text-xs">
        {formatDate(d.createdAt)}
      </span>
    ),
  },
];

function TempBar({ label, value, max = 1 }: { label: string; value: number; max?: number }) {
  const pct = Math.min((value / max) * 100, 100);
  return (
    <div className="space-y-1">
      <div className="flex justify-between text-xs">
        <span className="text-muted-foreground">{label}</span>
        <span className="tabular-nums font-mono">{value.toFixed(2)}</span>
      </div>
      <div className="h-1.5 rounded-full bg-secondary overflow-hidden">
        <div
          className="h-full rounded-full bg-blue-500/60"
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  );
}

export function SituationDetailPage() {
  const { id } = useParams<{ id: string }>();
  const { data, loading } = useQuery(SITUATION_DETAIL, {
    variables: { id },
  });

  if (loading) return <p className="text-muted-foreground">Loading...</p>;

  const s = data?.situation;
  if (!s) return <p className="text-muted-foreground">Situation not found</p>;

  return (
    <div className="space-y-6 max-w-4xl">
      {/* Breadcrumb */}
      <nav className="text-sm text-muted-foreground">
        <Link to="/data?tab=situations" className="hover:text-foreground">
          Situations
        </Link>
        <span className="mx-2">/</span>
        <span className="line-clamp-1">{s.headline}</span>
      </nav>

      {/* Header */}
      <div>
        <div className="flex items-center gap-3 mb-2">
          <h1 className="text-xl font-semibold">{s.headline}</h1>
          <span
            className={`px-2 py-0.5 rounded-full text-xs ${ARC_COLORS[s.arc] ?? "bg-secondary"}`}
          >
            {s.arc}
          </span>
        </div>
        <p className="text-sm text-muted-foreground">{s.lede}</p>
        <div className="flex items-center gap-4 mt-2 text-xs text-muted-foreground">
          {s.locationName && <span>{s.locationName}</span>}
          {s.category && (
            <span className="px-2 py-0.5 rounded-full bg-secondary">{s.category}</span>
          )}
          {s.sensitivity && s.sensitivity !== "ROUTINE" && (
            <span className="px-2 py-0.5 rounded-full bg-amber-500/10 text-amber-400">
              {s.sensitivity}
            </span>
          )}
          {s.clarity && (
            <span className="px-2 py-0.5 rounded-full bg-secondary">{s.clarity}</span>
          )}
        </div>
      </div>

      {/* Stats row */}
      <div className="rounded-lg border border-border p-4">
        <dl className="grid grid-cols-5 gap-4">
          <div className="space-y-1">
            <dt className="text-xs text-muted-foreground">Temperature</dt>
            <dd className="text-lg font-mono tabular-nums font-semibold">
              {s.temperature.toFixed(2)}
            </dd>
          </div>
          <div className="space-y-1">
            <dt className="text-xs text-muted-foreground">Signals</dt>
            <dd className="text-lg font-mono tabular-nums font-semibold">{s.signalCount}</dd>
          </div>
          <div className="space-y-1">
            <dt className="text-xs text-muted-foreground">Dispatches</dt>
            <dd className="text-lg font-mono tabular-nums font-semibold">{s.dispatchCount}</dd>
          </div>
          <div className="space-y-1">
            <dt className="text-xs text-muted-foreground">First Seen</dt>
            <dd className="text-sm">{formatDate(s.firstSeen)}</dd>
          </div>
          <div className="space-y-1">
            <dt className="text-xs text-muted-foreground">Last Updated</dt>
            <dd className="text-sm">{formatDate(s.lastUpdated)}</dd>
          </div>
        </dl>
      </div>

      {/* Temperature components */}
      <div className="rounded-lg border border-border p-4 space-y-3">
        <h2 className="text-sm font-medium">Temperature Components</h2>
        <TempBar label="Tension Heat" value={s.tensionHeat} />
        <TempBar label="Entity Velocity" value={s.entityVelocity} />
        <TempBar label="Amplification" value={s.amplification} />
        <TempBar label="Response Coverage" value={s.responseCoverage} />
        <TempBar label="Clarity Need" value={s.clarityNeed} />
      </div>

      {/* Dispatches */}
      <div>
        <h2 className="text-sm font-medium mb-3">
          Dispatches ({s.dispatches?.length ?? 0})
        </h2>
        <DataTable<Dispatch>
          columns={dispatchColumns}
          data={s.dispatches ?? []}
          getRowKey={(d) => d.id}
          emptyMessage="No dispatches yet."
        />
      </div>
    </div>
  );
}
