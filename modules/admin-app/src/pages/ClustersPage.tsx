import { useState } from "react";
import { Link } from "react-router";
import { useQuery } from "@apollo/client";
import { ADMIN_CLUSTERS } from "@/graphql/queries";
import { DataTable, type Column } from "@/components/DataTable";

type Cluster = {
  id: string;
  label: string;
  queries: string[];
  createdAt: string;
  memberCount: number;
  wovenSituationId: string | null;
};

const formatDate = (d: string) =>
  new Date(d).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });

const columns: Column<Cluster>[] = [
  {
    key: "label",
    label: "Label",
    render: (c) => (
      <Link to={`/clusters/${c.id}`} className="text-blue-400 hover:underline font-medium">
        {c.label}
      </Link>
    ),
  },
  {
    key: "memberCount",
    label: "Signals",
    align: "right",
    render: (c) => <span className="tabular-nums">{c.memberCount}</span>,
  },
  {
    key: "queries",
    label: "Queries",
    render: (c) => (
      <div className="flex flex-wrap gap-1">
        {c.queries.slice(0, 3).map((q, i) => (
          <span key={i} className="text-xs px-2 py-0.5 rounded-full bg-muted text-muted-foreground truncate max-w-[200px]">
            {q}
          </span>
        ))}
        {c.queries.length > 3 && (
          <span className="text-xs text-muted-foreground">+{c.queries.length - 3}</span>
        )}
      </div>
    ),
  },
  {
    key: "wovenSituationId",
    label: "Status",
    render: (c) =>
      c.wovenSituationId ? (
        <span className="text-xs px-2 py-0.5 rounded-full bg-green-900/30 text-green-400 border border-green-500/30">
          Woven
        </span>
      ) : (
        <span className="text-xs px-2 py-0.5 rounded-full bg-amber-900/30 text-amber-400 border border-amber-500/30">
          Open
        </span>
      ),
  },
  {
    key: "createdAt",
    label: "Created",
    render: (c) => (
      <span className="text-muted-foreground whitespace-nowrap">
        {c.createdAt ? formatDate(c.createdAt) : "-"}
      </span>
    ),
  },
];

export function ClustersPage() {
  const [search, setSearch] = useState("");
  const { data, loading } = useQuery(ADMIN_CLUSTERS, {
    variables: { limit: 200 },
  });
  const allClusters: Cluster[] = data?.adminClusters ?? [];
  const clusters = search
    ? allClusters.filter(
        (c) =>
          c.label.toLowerCase().includes(search.toLowerCase()) ||
          c.queries.some((q) => q.toLowerCase().includes(search.toLowerCase()))
      )
    : allClusters;

  const wovenCount = clusters.filter((c) => c.wovenSituationId).length;
  const openCount = clusters.length - wovenCount;

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h1 className="text-xl font-semibold">Clusters</h1>
          <span className="text-sm text-muted-foreground">({clusters.length})</span>
        </div>
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <span className="px-2 py-0.5 rounded-full bg-amber-900/30 text-amber-400 border border-amber-500/30">
            {openCount} open
          </span>
          <span className="px-2 py-0.5 rounded-full bg-green-900/30 text-green-400 border border-green-500/30">
            {wovenCount} woven
          </span>
        </div>
      </div>

      <input
        type="text"
        value={search}
        onChange={(e) => setSearch(e.target.value)}
        placeholder="Search clusters..."
        className="px-3 py-1.5 rounded-md border border-input bg-background text-sm w-64"
      />

      <DataTable<Cluster>
        columns={columns}
        data={clusters}
        getRowKey={(c) => c.id}
        loading={loading}
        emptyMessage="No clusters found."
      />
    </div>
  );
}
