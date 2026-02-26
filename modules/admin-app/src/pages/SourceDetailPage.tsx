import { useParams, Link } from "react-router";
import { useQuery } from "@apollo/client";
import { SOURCE_DETAIL } from "@/graphql/queries";

const formatDate = (d: string | null | undefined) => {
  if (!d) return "Never";
  return new Date(d).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
};

const SIGNAL_TYPE_COLORS: Record<string, string> = {
  Gathering: "bg-blue-500/10 text-blue-400 border-blue-500/20",
  Aid: "bg-green-500/10 text-green-400 border-green-500/20",
  Need: "bg-amber-500/10 text-amber-400 border-amber-500/20",
  Notice: "bg-purple-500/10 text-purple-400 border-purple-500/20",
  Tension: "bg-red-500/10 text-red-400 border-red-500/20",
};

type SignalBrief = {
  id: string;
  title: string;
  signalType: string;
  confidence: number;
  extractedAt: string | null;
  sourceUrl: string;
};

type ArchiveSummary = {
  posts: number;
  pages: number;
  feeds: number;
  shortVideos: number;
  longVideos: number;
  stories: number;
  searchResults: number;
  files: number;
  lastFetchedAt: string | null;
};

type TreeNode = {
  id: string;
  canonicalValue: string;
  discoveryMethod: string;
  active: boolean;
  signalsProduced: number;
};

type TreeEdge = {
  childId: string;
  parentId: string;
};

type DiscoveryTree = {
  nodes: TreeNode[];
  edges: TreeEdge[];
  rootId: string;
};

type SourceDetail = {
  id: string;
  url: string;
  canonicalValue: string;
  sourceLabel: string;
  weight: number;
  qualityPenalty: number;
  effectiveWeight: number;
  discoveryMethod: string;
  lastScraped: string | null;
  cadenceHours: number;
  signalsProduced: number;
  signalsCorroborated: number;
  consecutiveEmptyRuns: number;
  active: boolean;
  gapContext: string | null;
  scrapeCount: number;
  avgSignalsPerScrape: number;
  sourceRole: string;
  createdAt: string;
  lastProducedSignal: string | null;
  signals: SignalBrief[];
  archiveSummary: ArchiveSummary | null;
  discoveryTree: DiscoveryTree;
};

function MetaCard({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="space-y-1">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="text-sm font-medium tabular-nums">{value}</dd>
    </div>
  );
}

function DiscoveryTreeView({ tree }: { tree: DiscoveryTree }) {
  if (tree.edges.length === 0) {
    return (
      <p className="text-sm text-muted-foreground">No discovery lineage</p>
    );
  }

  const nodesById = new Map(tree.nodes.map((n) => [n.id, n]));

  // Build parent→children map
  const childrenOf = new Map<string, string[]>();
  const parentOf = new Map<string, string>();
  for (const edge of tree.edges) {
    const children = childrenOf.get(edge.parentId) ?? [];
    children.push(edge.childId);
    childrenOf.set(edge.parentId, children);
    parentOf.set(edge.childId, edge.parentId);
  }

  // Find ancestors (walk up from root)
  const ancestors: string[] = [];
  let current = tree.rootId;
  while (parentOf.has(current)) {
    const parent = parentOf.get(current)!;
    ancestors.unshift(parent);
    current = parent;
  }

  // Find direct descendants
  const descendants = childrenOf.get(tree.rootId) ?? [];

  const renderNode = (id: string, indent: number, isRoot: boolean) => {
    const node = nodesById.get(id);
    if (!node) return null;
    return (
      <div
        key={id}
        className="flex items-center gap-2 py-1.5"
        style={{ paddingLeft: `${indent * 24}px` }}
      >
        {indent > 0 && (
          <span className="text-muted-foreground text-xs">{"└─"}</span>
        )}
        <Link
          to={`/sources/${id}`}
          className={`text-sm hover:underline ${
            isRoot ? "text-blue-400 font-medium" : "text-foreground"
          }`}
        >
          {node.canonicalValue}
        </Link>
        <span className="text-xs text-muted-foreground">
          {node.signalsProduced} signals
        </span>
        {!node.active && (
          <span className="text-xs px-1.5 py-0.5 rounded-full bg-muted text-muted-foreground border border-border">
            Inactive
          </span>
        )}
        <span className="text-xs text-muted-foreground">
          {node.discoveryMethod}
        </span>
      </div>
    );
  };

  return (
    <div className="space-y-0">
      {ancestors.map((id, i) => renderNode(id, i, false))}
      {renderNode(tree.rootId, ancestors.length, true)}
      {descendants.map((id) =>
        renderNode(id, ancestors.length + 1, false)
      )}
    </div>
  );
}

export function SourceDetailPage() {
  const { id } = useParams<{ id: string }>();
  const { data, loading } = useQuery(SOURCE_DETAIL, {
    variables: { id },
  });

  if (loading) return <p className="text-muted-foreground">Loading...</p>;

  const source: SourceDetail | undefined = data?.sourceDetail;
  if (!source) return <p className="text-muted-foreground">Source not found</p>;

  const archive = source.archiveSummary;
  const archiveEntries = archive
    ? [
        { label: "Posts", count: archive.posts },
        { label: "Pages", count: archive.pages },
        { label: "Feeds", count: archive.feeds },
        { label: "Short Videos", count: archive.shortVideos },
        { label: "Long Videos", count: archive.longVideos },
        { label: "Stories", count: archive.stories },
        { label: "Search Results", count: archive.searchResults },
        { label: "Files", count: archive.files },
      ].filter((e) => e.count > 0)
    : [];

  return (
    <div className="space-y-6 max-w-4xl">
      {/* Header */}
      <div className="space-y-2">
        <div className="flex items-center gap-3">
          <Link
            to="/sources"
            className="text-muted-foreground hover:text-foreground text-sm"
          >
            Sources
          </Link>
          <span className="text-muted-foreground">/</span>
        </div>
        <div className="flex items-center gap-3 flex-wrap">
          <h1 className="text-xl font-semibold break-all">
            {source.canonicalValue}
          </h1>
          {source.url && (
            <a
              href={source.url}
              target="_blank"
              rel="noopener noreferrer"
              className="text-muted-foreground hover:text-foreground"
              title="Open externally"
            >
              <svg
                className="w-4 h-4"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14"
                />
              </svg>
            </a>
          )}
        </div>
        <div className="flex items-center gap-2 flex-wrap">
          <span className="text-xs px-2 py-0.5 rounded-full border bg-muted text-muted-foreground border-border">
            {source.sourceLabel}
          </span>
          <span
            className={`text-xs px-2 py-0.5 rounded-full border ${
              source.active
                ? "bg-green-900/30 text-green-400 border-green-500/30"
                : "bg-muted text-muted-foreground border-border"
            }`}
          >
            {source.active ? "Active" : "Inactive"}
          </span>
          <span className="text-xs px-2 py-0.5 rounded-full border bg-muted text-muted-foreground border-border">
            {source.discoveryMethod}
          </span>
          <span className="text-xs px-2 py-0.5 rounded-full border bg-muted text-muted-foreground border-border">
            {source.sourceRole}
          </span>
          <span className="text-xs text-muted-foreground">
            Created {formatDate(source.createdAt)}
          </span>
        </div>
      </div>

      {/* Metadata cards */}
      <div className="grid grid-cols-2 gap-4">
        <div className="rounded-lg border border-border p-4 space-y-3">
          <h3 className="text-sm font-medium text-muted-foreground">
            Weight
          </h3>
          <dl className="grid grid-cols-3 gap-4">
            <MetaCard label="Weight" value={source.weight.toFixed(2)} />
            <MetaCard
              label="Quality Penalty"
              value={source.qualityPenalty.toFixed(2)}
            />
            <MetaCard
              label="Effective"
              value={source.effectiveWeight.toFixed(2)}
            />
          </dl>
        </div>

        <div className="rounded-lg border border-border p-4 space-y-3">
          <h3 className="text-sm font-medium text-muted-foreground">
            Scrape Stats
          </h3>
          <dl className="grid grid-cols-3 gap-4">
            <MetaCard label="Scrape Count" value={source.scrapeCount} />
            <MetaCard
              label="Avg Signals/Scrape"
              value={source.avgSignalsPerScrape.toFixed(1)}
            />
            <MetaCard
              label="Empty Runs"
              value={source.consecutiveEmptyRuns}
            />
          </dl>
        </div>

        <div className="rounded-lg border border-border p-4 space-y-3">
          <h3 className="text-sm font-medium text-muted-foreground">
            Schedule
          </h3>
          <dl className="grid grid-cols-3 gap-4">
            <MetaCard
              label="Cadence"
              value={`${source.cadenceHours}h`}
            />
            <MetaCard
              label="Last Scraped"
              value={formatDate(source.lastScraped)}
            />
            <MetaCard
              label="Last Signal"
              value={formatDate(source.lastProducedSignal)}
            />
          </dl>
        </div>

        <div className="rounded-lg border border-border p-4 space-y-3">
          <h3 className="text-sm font-medium text-muted-foreground">
            Output
          </h3>
          <dl className="grid grid-cols-3 gap-4">
            <MetaCard
              label="Signals Produced"
              value={source.signalsProduced}
            />
            <MetaCard
              label="Corroborated"
              value={source.signalsCorroborated}
            />
            <MetaCard label="Role" value={source.sourceRole} />
          </dl>
        </div>
      </div>

      {/* Gap context */}
      {source.gapContext && (
        <div className="rounded-lg border border-border p-4">
          <h3 className="text-sm font-medium text-muted-foreground mb-2">
            Gap Context
          </h3>
          <p className="text-sm">{source.gapContext}</p>
        </div>
      )}

      {/* Signals produced */}
      <div className="rounded-lg border border-border">
        <div className="px-4 py-3 border-b border-border flex items-center justify-between">
          <h3 className="text-sm font-medium">
            Signals Produced ({source.signals.length}
            {source.signals.length >= 50 ? "+" : ""})
          </h3>
        </div>
        {source.signals.length === 0 ? (
          <p className="px-4 py-3 text-sm text-muted-foreground">
            No signals produced yet
          </p>
        ) : (
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border bg-muted/50 text-left text-muted-foreground">
                <th className="px-4 py-2 font-medium">Type</th>
                <th className="px-4 py-2 font-medium">Title</th>
                <th className="px-4 py-2 font-medium text-right">
                  Confidence
                </th>
                <th className="px-4 py-2 font-medium">Extracted</th>
              </tr>
            </thead>
            <tbody>
              {source.signals.map((s) => (
                <tr
                  key={s.id}
                  className="border-b border-border last:border-0 hover:bg-muted/30"
                >
                  <td className="px-4 py-2">
                    <span
                      className={`text-xs px-2 py-0.5 rounded-full border ${
                        SIGNAL_TYPE_COLORS[s.signalType] ??
                        "bg-muted text-muted-foreground border-border"
                      }`}
                    >
                      {s.signalType}
                    </span>
                  </td>
                  <td className="px-4 py-2 max-w-[300px] truncate">
                    <Link
                      to={`/signals/${s.id}`}
                      className="text-blue-400 hover:underline"
                    >
                      {s.title}
                    </Link>
                  </td>
                  <td className="px-4 py-2 text-right tabular-nums">
                    {(s.confidence * 100).toFixed(0)}%
                  </td>
                  <td className="px-4 py-2 text-muted-foreground whitespace-nowrap">
                    {formatDate(s.extractedAt)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {/* Archive summary */}
      {archiveEntries.length > 0 && (
        <div className="rounded-lg border border-border p-4 space-y-3">
          <h3 className="text-sm font-medium">Archive Content</h3>
          <div className="flex flex-wrap gap-3">
            {archiveEntries.map((e) => (
              <div key={e.label} className="text-sm">
                <span className="text-muted-foreground">{e.label}:</span>{" "}
                <span className="font-medium tabular-nums">{e.count}</span>
              </div>
            ))}
          </div>
          {archive?.lastFetchedAt && (
            <p className="text-xs text-muted-foreground">
              Last fetched {formatDate(archive.lastFetchedAt)}
            </p>
          )}
        </div>
      )}

      {/* Discovery tree */}
      <div className="rounded-lg border border-border p-4 space-y-3">
        <h3 className="text-sm font-medium">Discovery Tree</h3>
        <DiscoveryTreeView tree={source.discoveryTree} />
      </div>
    </div>
  );
}
