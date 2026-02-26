import { useState, useMemo } from "react";
import { Link } from "react-router";
import { useQuery } from "@apollo/client";
import { ADMIN_SIGNALS } from "@/graphql/queries";
import { ReviewStatusBadge } from "@/components/ReviewStatusBadge";

const SIGNAL_TYPE_COLORS: Record<string, string> = {
  Gathering: "bg-blue-500/10 text-blue-400 border-blue-500/20",
  Aid: "bg-green-500/10 text-green-400 border-green-500/20",
  Need: "bg-amber-500/10 text-amber-400 border-amber-500/20",
  Notice: "bg-purple-500/10 text-purple-400 border-purple-500/20",
  Tension: "bg-red-500/10 text-red-400 border-red-500/20",
};

const SIGNAL_TYPES = ["All", "Gathering", "Aid", "Need", "Notice", "Tension"] as const;

const STATUS_OPTIONS = [
  { value: "", label: "All" },
  { value: "staged", label: "Staged" },
  { value: "live", label: "Published" },
  { value: "rejected", label: "Rejected" },
];

type SortKey = "type" | "title" | "confidence" | "sourceUrl" | "contentDate" | "extractedAt" | "reviewStatus";
type SortDir = "asc" | "desc";

const PAGE_SIZES = [25, 50, 100] as const;

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
    reviewStatus: (s.reviewStatus as string) ?? "live",
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

  const handleSort = (key: SortKey) => {
    if (sortKey === key) {
      setSortDir(sortDir === "asc" ? "desc" : "asc");
    } else {
      setSortKey(key);
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

  const sortIndicator = (key: SortKey) =>
    sortKey === key ? (sortDir === "asc" ? " \u2191" : " \u2193") : "";

  const SortHeader = ({ k, label, className = "" }: { k: SortKey; label: string; className?: string }) => (
    <th
      className={`px-4 py-2 font-medium cursor-pointer select-none hover:text-foreground ${className}`}
      onClick={() => handleSort(k)}
    >
      {label}{sortIndicator(k)}
    </th>
  );

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

      {loading ? (
        <p className="text-muted-foreground">Loading signals...</p>
      ) : filtered.length === 0 ? (
        <p className="text-muted-foreground">No signals match the current filters.</p>
      ) : (
        <>
          <div className="rounded-lg border border-border overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border bg-muted/50 text-left text-muted-foreground">
                  <SortHeader k="type" label="Type" />
                  <SortHeader k="title" label="Title" />
                  <SortHeader k="confidence" label="Confidence" className="text-right" />
                  <SortHeader k="sourceUrl" label="Source URL" />
                  <SortHeader k="contentDate" label="Content Date" />
                  <SortHeader k="extractedAt" label="Extracted At" />
                  <SortHeader k="reviewStatus" label="Status" />
                </tr>
              </thead>
              <tbody>
                {pageSlice.map((s) => (
                  <tr key={s.id} className="border-b border-border/50 hover:bg-muted/30">
                    <td className="px-4 py-2">
                      <span className={`px-2 py-0.5 rounded-full text-xs border ${SIGNAL_TYPE_COLORS[s.type] ?? "bg-secondary"}`}>
                        {s.type}
                      </span>
                    </td>
                    <td className="px-4 py-2 max-w-[300px] truncate">
                      <Link to={`/signals/${s.id}`} className="text-blue-400 hover:underline">
                        {s.title}
                      </Link>
                    </td>
                    <td className="px-4 py-2 text-right tabular-nums">
                      {(s.confidence * 100).toFixed(0)}%
                    </td>
                    <td className="px-4 py-2 max-w-[200px] truncate">
                      {s.sourceUrl ? (
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
                        <span className="text-muted-foreground/50">—</span>
                      )}
                    </td>
                    <td className="px-4 py-2 text-muted-foreground tabular-nums whitespace-nowrap">
                      {s.contentDate
                        ? new Date(s.contentDate).toLocaleDateString()
                        : "—"}
                    </td>
                    <td className="px-4 py-2 text-muted-foreground tabular-nums whitespace-nowrap">
                      {new Date(s.extractedAt).toLocaleDateString()}
                    </td>
                    <td className="px-4 py-2">
                      <ReviewStatusBadge status={s.reviewStatus} wasCorrected={s.wasCorrected} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {/* Pagination */}
          <div className="flex items-center justify-between text-sm">
            <div className="flex items-center gap-2 text-muted-foreground">
              <span>
                {pageStart + 1}–{Math.min(pageStart + pageSize, filtered.length)} of {filtered.length}
              </span>
              <select
                value={pageSize}
                onChange={(e) => { setPageSize(Number(e.target.value)); setPage(0); }}
                className="px-2 py-1 rounded border border-input bg-background text-sm"
              >
                {PAGE_SIZES.map((s) => (
                  <option key={s} value={s}>{s} / page</option>
                ))}
              </select>
            </div>
            <div className="flex items-center gap-1">
              <button
                onClick={() => setPage(0)}
                disabled={safePageIndex === 0}
                className="px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground disabled:opacity-30"
              >
                First
              </button>
              <button
                onClick={() => setPage(safePageIndex - 1)}
                disabled={safePageIndex === 0}
                className="px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground disabled:opacity-30"
              >
                Prev
              </button>
              <span className="px-2 text-muted-foreground">
                Page {safePageIndex + 1} of {totalPages}
              </span>
              <button
                onClick={() => setPage(safePageIndex + 1)}
                disabled={safePageIndex >= totalPages - 1}
                className="px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground disabled:opacity-30"
              >
                Next
              </button>
              <button
                onClick={() => setPage(totalPages - 1)}
                disabled={safePageIndex >= totalPages - 1}
                className="px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground disabled:opacity-30"
              >
                Last
              </button>
            </div>
          </div>
        </>
      )}
    </div>
  );
}
