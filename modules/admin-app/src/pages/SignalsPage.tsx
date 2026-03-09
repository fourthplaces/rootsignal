import { useState, useMemo } from "react";
import { Link } from "react-router";
import { useQuery } from "@apollo/client";
import { ADMIN_SIGNALS } from "@/graphql/queries";
import { ReviewStatusBadge } from "@/components/ReviewStatusBadge";
import { DataTable, type Column } from "@/components/DataTable";

const SIGNAL_TYPE_COLORS: Record<string, string> = {
  Gathering: "bg-blue-500/10 text-blue-400 border-blue-500/20",
  Resource: "bg-green-500/10 text-green-400 border-green-500/20",
  HelpRequest: "bg-amber-500/10 text-amber-400 border-amber-500/20",
  Announcement: "bg-purple-500/10 text-purple-400 border-purple-500/20",
  Concern: "bg-red-500/10 text-red-400 border-red-500/20",
};

const SIGNAL_TYPES = ["All", "Gathering", "Resource", "HelpRequest", "Announcement", "Concern"] as const;

const STATUS_OPTIONS = [
  { value: "", label: "All" },
  { value: "staged", label: "Staged" },
  { value: "accepted", label: "Accepted" },
  { value: "rejected", label: "Rejected" },
];

type SortKey = "type" | "title" | "confidence" | "sourceUrl" | "contentDate" | "extractedAt" | "reviewStatus";
type SortDir = "asc" | "desc";

const PAGE_SIZES = [25, 50, 100];

type Signal = {
  id: string;
  title: string;
  confidence: number;
  extractedAt: string;
  contentDate: string | null;
  reviewStatus: string;
  wasCorrected: boolean;
  sourceUrl: string | null;
  type: string;
  __typename: string;
};

function getSignalFields(s: Record<string, unknown>): Signal {
  const typeName = (s.__typename as string).replace("Gql", "").replace("Signal", "");
  return {
    id: s.id as string,
    title: s.title as string,
    confidence: s.confidence as number,
    extractedAt: s.extractedAt as string,
    contentDate: (s.contentDate as string) ?? null,
    reviewStatus: (s.reviewStatus as string) ?? "accepted",
    wasCorrected: (s.wasCorrected as boolean) ?? false,
    sourceUrl: (s.sourceUrl as string) ?? null,
    type: typeName,
    __typename: s.__typename as string,
  };
}

function truncateUrl(url: string, max = 40): string {
  try {
    const u = new URL(url);
    const path = u.pathname + u.search;
    const display = u.host + path;
    return display.length > max ? display.slice(0, max) + "..." : display;
  } catch {
    return url.length > max ? url.slice(0, max) + "..." : url;
  }
}

const columns: Column<Signal>[] = [
  {
    key: "type",
    label: "Type",
    render: (s) => (
      <span className={`px-2 py-0.5 rounded-full text-xs border ${SIGNAL_TYPE_COLORS[s.type] ?? "bg-secondary"}`}>
        {s.type}
      </span>
    ),
  },
  {
    key: "title",
    label: "Title",
    className: "max-w-[300px] truncate",
    render: (s) => (
      <Link to={`/signals/${s.id}`} className="text-blue-400 hover:underline">
        {s.title}
      </Link>
    ),
  },
  {
    key: "confidence",
    label: "Confidence",
    align: "right",
    render: (s) => <span className="tabular-nums">{(s.confidence * 100).toFixed(0)}%</span>,
  },
  {
    key: "sourceUrl",
    label: "Source URL",
    className: "max-w-[200px] truncate",
    render: (s) =>
      s.sourceUrl ? (
        <a
          href={s.sourceUrl}
          target="_blank"
          rel="noopener noreferrer"
          className="text-muted-foreground hover:text-foreground"
          title={s.sourceUrl}
        >
          {truncateUrl(s.sourceUrl)}
        </a>
      ) : (
        <span className="text-muted-foreground/50">&mdash;</span>
      ),
  },
  {
    key: "contentDate",
    label: "Content Date",
    render: (s) => (
      <span className="text-muted-foreground tabular-nums whitespace-nowrap">
        {s.contentDate ? new Date(s.contentDate).toLocaleDateString() : "\u2014"}
      </span>
    ),
  },
  {
    key: "extractedAt",
    label: "Extracted At",
    render: (s) => (
      <span className="text-muted-foreground tabular-nums whitespace-nowrap">
        {new Date(s.extractedAt).toLocaleDateString()}
      </span>
    ),
  },
  {
    key: "reviewStatus",
    label: "Status",
    render: (s) => <ReviewStatusBadge status={s.reviewStatus} wasCorrected={s.wasCorrected} />,
  },
];

export function SignalsPage() {
  const [statusFilter, setStatusFilter] = useState("");
  const [typeFilter, setTypeFilter] = useState<string>("All");
  const [sortKey, setSortKey] = useState<SortKey>("extractedAt");
  const [sortDir, setSortDir] = useState<SortDir>("desc");
  const [page, setPage] = useState(0);
  const [pageSize, setPageSize] = useState<number>(50);

  const { data, loading } = useQuery(ADMIN_SIGNALS, {
    variables: {
      limit: 500,
      ...(statusFilter ? { status: statusFilter } : {}),
    },
  });

  const signals: Signal[] = useMemo(
    () => (data?.adminSignals ?? []).map((s: Record<string, unknown>) => getSignalFields(s)),
    [data],
  );

  // Counts per type (before type filter, after status filter)
  const typeCounts = useMemo(() => {
    const counts: Record<string, number> = { All: signals.length };
    for (const s of signals) {
      counts[s.type] = (counts[s.type] ?? 0) + 1;
    }
    return counts;
  }, [signals]);

  const handleSort = (key: string) => {
    const k = key as SortKey;
    if (sortKey === k) {
      setSortDir(sortDir === "asc" ? "desc" : "asc");
    } else {
      setSortKey(k);
      setSortDir("desc");
    }
    setPage(0);
  };

  const filtered = useMemo(() => {
    let list = [...signals];

    // Type filter
    if (typeFilter !== "All") {
      list = list.filter((s) => s.type === typeFilter);
    }

    // Sort
    list.sort((a, b) => {
      const av = a[sortKey];
      const bv = b[sortKey];
      if (av == null && bv == null) return 0;
      if (av == null) return 1;
      if (bv == null) return -1;
      if (typeof av === "string" && typeof bv === "string") {
        return sortDir === "asc" ? av.localeCompare(bv) : bv.localeCompare(av);
      }
      if (typeof av === "number" && typeof bv === "number") {
        return sortDir === "asc" ? av - bv : bv - av;
      }
      return 0;
    });

    return list;
  }, [signals, typeFilter, sortKey, sortDir]);

  // Pagination
  const totalPages = Math.max(1, Math.ceil(filtered.length / pageSize));
  const safePageIndex = Math.min(page, totalPages - 1);
  const pageStart = safePageIndex * pageSize;
  const pageSlice = filtered.slice(pageStart, pageStart + pageSize);

  return (
    <div className="space-y-4">
      <h1 className="text-xl font-semibold">Signals ({signals.length})</h1>

      {/* Type filter pills */}
      <div className="flex flex-wrap gap-2">
        {SIGNAL_TYPES.map((t) => (
          <button
            key={t}
            onClick={() => { setTypeFilter(t); setPage(0); }}
            className={`px-3 py-1.5 rounded-md text-sm transition-colors ${
              typeFilter === t
                ? "bg-accent text-accent-foreground"
                : "text-muted-foreground hover:text-foreground hover:bg-accent/50"
            }`}
          >
            {t} ({typeCounts[t] ?? 0})
          </button>
        ))}
      </div>

      {/* Review status filter */}
      <div className="flex gap-1">
        {STATUS_OPTIONS.map((opt) => (
          <button
            key={opt.value}
            onClick={() => { setStatusFilter(opt.value); setPage(0); }}
            className={`px-3 py-1 text-xs rounded-md ${
              statusFilter === opt.value
                ? "bg-foreground text-background"
                : "bg-secondary hover:bg-secondary/80"
            }`}
          >
            {opt.label}
          </button>
        ))}
      </div>

      <DataTable<Signal>
        columns={columns}
        data={pageSlice}
        getRowKey={(s) => s.id}
        sortKey={sortKey}
        sortDir={sortDir}
        onSort={handleSort}
        rowClassName={() => "border-border/50"}
        loading={loading}
        emptyMessage="No signals match the current filters."
        pagination={{
          mode: "offset",
          page: safePageIndex,
          pageSize,
          totalRows: filtered.length,
          pageSizes: PAGE_SIZES,
          onPageChange: setPage,
          onPageSizeChange: (size) => { setPageSize(size); setPage(0); },
        }}
      />
    </div>
  );
}
