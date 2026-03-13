import { useState } from "react";
import { Link, useParams } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import { ADMIN_CLUSTER_DETAIL } from "@/graphql/queries";
import { WEAVE_CLUSTER, FEED_GROUP } from "@/graphql/mutations";
import { DataTable, type Column } from "@/components/DataTable";

const SIGNAL_TYPE_COLORS: Record<string, string> = {
  Gathering: "bg-blue-500/10 text-blue-400 border-blue-500/20",
  Resource: "bg-green-500/10 text-green-400 border-green-500/20",
  HelpRequest: "bg-amber-500/10 text-amber-400 border-amber-500/20",
  Announcement: "bg-purple-500/10 text-purple-400 border-purple-500/20",
  Concern: "bg-red-500/10 text-red-400 border-red-500/20",
  Condition: "bg-orange-500/10 text-orange-400 border-orange-500/20",
};

type ClusterMember = {
  id: string;
  title: string;
  signalType: string;
  confidence: number;
  sourceUrl: string | null;
  summary: string | null;
};

const memberColumns: Column<ClusterMember>[] = [
  {
    key: "title",
    label: "Title",
    render: (m) => (
      <span>
        <Link to={`/signals/${m.id}`} className="text-blue-400 hover:underline">
          {m.title}
        </Link>
        {m.summary && (
          <p className="text-xs text-muted-foreground mt-0.5 line-clamp-1">{m.summary}</p>
        )}
      </span>
    ),
  },
  {
    key: "signalType",
    label: "Type",
    render: (m) => (
      <span className={`px-2 py-0.5 rounded-full text-xs border ${SIGNAL_TYPE_COLORS[m.signalType] ?? "bg-muted text-muted-foreground border-border"}`}>
        {m.signalType}
      </span>
    ),
  },
  {
    key: "confidence",
    label: "Confidence",
    render: (m) => <span className="tabular-nums">{(m.confidence * 100).toFixed(0)}%</span>,
  },
  {
    key: "sourceUrl",
    label: "Source",
    render: (m) =>
      m.sourceUrl ? (
        <a href={m.sourceUrl} target="_blank" rel="noopener noreferrer" className="text-xs text-blue-400 hover:underline truncate max-w-[200px] block">
          {m.sourceUrl.replace(/^https?:\/\/(www\.)?/, "").slice(0, 40)}
        </a>
      ) : null,
  },
];

export function ClusterDetailPage() {
  const { id } = useParams<{ id: string }>();
  const { data, loading } = useQuery(ADMIN_CLUSTER_DETAIL, {
    variables: { groupId: id },
  });
  const [weave, { loading: weaving }] = useMutation(WEAVE_CLUSTER);
  const [feed, { loading: feeding }] = useMutation(FEED_GROUP);
  const [weaveMsg, setWeaveMsg] = useState<string | null>(null);
  const [feedMsg, setFeedMsg] = useState<string | null>(null);

  if (loading) return <p className="text-muted-foreground">Loading...</p>;

  const cluster = data?.adminClusterDetail;
  if (!cluster) return <p className="text-muted-foreground">Cluster not found</p>;

  const isWoven = !!cluster.wovenSituationId;

  return (
    <div className="space-y-6 max-w-4xl">
      {/* Breadcrumb */}
      <nav className="text-sm text-muted-foreground">
        <Link to="/data?tab=clusters" className="hover:text-foreground">Clusters</Link>
        <span className="mx-2">/</span>
        <span>{cluster.label}</span>
      </nav>

      {/* Header */}
      <div>
        <div className="flex items-center justify-between mb-1">
          <div className="flex items-center gap-3">
            <h1 className="text-xl font-semibold">{cluster.label}</h1>
            <span className="text-sm text-muted-foreground tabular-nums">
              {cluster.memberCount} signal{cluster.memberCount !== 1 ? "s" : ""}
            </span>
          </div>
          <div className="flex gap-2">
            <button
              className="px-3 py-1 text-xs rounded-md bg-secondary hover:bg-secondary/80 disabled:opacity-50"
              disabled={feeding}
              onClick={async () => {
                setFeedMsg(null);
                const { data } = await feed({ variables: { groupId: id } });
                setFeedMsg(data?.feedGroup?.message ?? null);
              }}
            >
              {feeding ? "Feeding..." : "Feed"}
            </button>
            <button
              className="px-3 py-1 text-xs rounded-md bg-secondary hover:bg-secondary/80 disabled:opacity-50"
              disabled={weaving}
              onClick={async () => {
                setWeaveMsg(null);
                const { data } = await weave({ variables: { groupId: id } });
                setWeaveMsg(data?.weaveCluster?.message ?? null);
              }}
            >
              {weaving ? "Weaving..." : isWoven ? "Re-weave" : "Weave"}
            </button>
          </div>
        </div>
        {(weaveMsg || feedMsg) && (
          <p className="text-xs text-muted-foreground">{feedMsg || weaveMsg}</p>
        )}
        <p className="text-sm text-muted-foreground">
          Created {new Date(cluster.createdAt).toLocaleDateString()}
        </p>
      </div>

      {/* Woven status */}
      {isWoven && (
        <div className="rounded-lg border border-green-500/30 bg-green-500/10 p-4 text-sm">
          <span className="text-green-400">Woven into </span>
          <Link
            to={`/situations/${cluster.wovenSituationId}`}
            className="text-green-400 hover:underline font-medium"
          >
            Situation {cluster.wovenSituationId.slice(0, 8)}
          </Link>
        </div>
      )}

      {/* Queries */}
      {cluster.queries.length > 0 && (
        <div className="rounded-lg border border-border p-4">
          <h2 className="text-sm font-medium mb-3">Queries ({cluster.queries.length})</h2>
          <div className="flex flex-wrap gap-1.5">
            {cluster.queries.map((q: string, i: number) => (
              <span key={i} className="text-xs px-2 py-0.5 rounded-full bg-blue-500/10 text-blue-400">
                {q}
              </span>
            ))}
          </div>
        </div>
      )}

      {/* Members table */}
      <div>
        <h2 className="text-sm font-medium mb-3">Member Signals ({cluster.memberCount})</h2>
        <DataTable<ClusterMember>
          columns={memberColumns}
          data={cluster.members}
          getRowKey={(m: ClusterMember) => m.id}
          emptyMessage="No member signals."
        />
      </div>
    </div>
  );
}
