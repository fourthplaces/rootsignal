import { useState, type ReactNode, type RefObject } from "react";

export type Column<T> = {
  key: string;
  label: string;
  sortable?: boolean;
  resizable?: boolean;
  defaultWidth?: number;
  align?: "left" | "right";
  className?: string;
  render: (row: T) => ReactNode;
};

type OffsetPagination = {
  mode: "offset";
  page: number;
  pageSize: number;
  totalRows: number;
  pageSizes?: number[];
  onPageChange: (page: number) => void;
  onPageSizeChange?: (size: number) => void;
};

type InfinitePagination = {
  mode: "infinite";
  visibleCount: number;
  totalRows: number;
  sentinelRef: RefObject<HTMLDivElement | null>;
  hasMore: boolean;
};

export type DataTableProps<T> = {
  columns: Column<T>[];
  data: T[];
  getRowKey: (row: T) => string;

  sortKey?: string;
  sortDir?: "asc" | "desc";
  onSort?: (key: string) => void;

  rowClassName?: (row: T) => string;
  onRowClick?: (row: T) => void;

  renderRowPrefix?: (row: T) => ReactNode;
  renderRowSuffix?: (row: T) => ReactNode;
  headerPrefix?: ReactNode;
  headerSuffix?: ReactNode;

  pagination?: OffsetPagination | InfinitePagination;

  loading?: boolean;
  emptyMessage?: string;
};

function SortIndicator({ active, dir }: { active: boolean; dir?: "asc" | "desc" }) {
  if (!active) return null;
  return <>{dir === "asc" ? " \u2191" : " \u2193"}</>;
}

export function DataTable<T>({
  columns,
  data,
  getRowKey,
  sortKey,
  sortDir,
  onSort,
  rowClassName,
  onRowClick,
  renderRowPrefix,
  renderRowSuffix,
  headerPrefix,
  headerSuffix,
  pagination,
  loading,
  emptyMessage = "No data.",
}: DataTableProps<T>) {
  const [colWidths, setColWidths] = useState<Record<string, number>>({});

  const hasResizable = columns.some((c) => c.resizable);

  const handleResizeStart = (key: string, startX: number, startWidth: number) => {
    const onMove = (e: MouseEvent) => {
      setColWidths((prev) => ({ ...prev, [key]: Math.max(40, startWidth + e.clientX - startX) }));
    };
    const onUp = () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  };

  if (loading) {
    return <p className="text-muted-foreground">Loading...</p>;
  }

  if (data.length === 0) {
    return <p className="text-muted-foreground">{emptyMessage}</p>;
  }

  return (
    <>
      <div className="rounded-lg border border-border overflow-x-auto">
        <table className={`w-full text-sm${hasResizable ? " table-fixed" : ""}`}>
          <thead>
            <tr className="border-b border-border bg-muted/50 text-left text-muted-foreground">
              {headerPrefix}
              {columns.map((col) => {
                const sortable = col.sortable !== false;
                const width = col.resizable
                  ? colWidths[col.key] ?? col.defaultWidth
                  : col.defaultWidth;

                return (
                  <th
                    key={col.key}
                    className={`px-4 py-2 font-medium overflow-hidden${
                      sortable ? " cursor-pointer select-none hover:text-foreground" : ""
                    }${col.resizable ? " relative group/th" : ""}${
                      col.align === "right" ? " text-right" : ""
                    }${col.className ? ` ${col.className}` : ""}`}
                    style={width ? { width } : undefined}
                    onClick={sortable && onSort ? () => onSort(col.key) : undefined}
                  >
                    {col.label}
                    <SortIndicator active={sortKey === col.key} dir={sortDir} />
                    {col.resizable && (
                      <div
                        onMouseDown={(e) => {
                          e.preventDefault();
                          e.stopPropagation();
                          handleResizeStart(
                            col.key,
                            e.clientX,
                            colWidths[col.key] ?? col.defaultWidth ?? 100,
                          );
                        }}
                        className="absolute right-0 top-0 bottom-0 w-1 cursor-col-resize opacity-0 group-hover/th:opacity-100 bg-border hover:bg-blue-500 transition-opacity"
                      />
                    )}
                  </th>
                );
              })}
              {headerSuffix}
            </tr>
          </thead>
          <tbody>
            {data.map((row) => (
              <tr
                key={getRowKey(row)}
                className={`border-b border-border last:border-0 hover:bg-muted/30${
                  rowClassName ? ` ${rowClassName(row)}` : ""
                }${onRowClick ? " cursor-pointer" : ""}`}
                onClick={onRowClick ? () => onRowClick(row) : undefined}
              >
                {renderRowPrefix?.(row)}
                {columns.map((col) => (
                  <td
                    key={col.key}
                    className={`px-4 py-2 overflow-hidden text-ellipsis whitespace-nowrap${col.align === "right" ? " text-right" : ""}${
                      col.className ? ` ${col.className}` : ""
                    }`}
                  >
                    {col.render(row)}
                  </td>
                ))}
                {renderRowSuffix?.(row)}
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {pagination?.mode === "infinite" && (
        <div
          ref={pagination.sentinelRef}
          className="flex items-center justify-center py-2 text-sm text-muted-foreground"
        >
          {pagination.hasMore
            ? `Showing ${pagination.visibleCount} of ${pagination.totalRows}`
            : `${pagination.totalRows} total`}
        </div>
      )}

      {pagination?.mode === "offset" && (
        <OffsetPaginationBar
          page={pagination.page}
          pageSize={pagination.pageSize}
          totalRows={pagination.totalRows}
          pageSizes={pagination.pageSizes}
          onPageChange={pagination.onPageChange}
          onPageSizeChange={pagination.onPageSizeChange}
        />
      )}
    </>
  );
}

function OffsetPaginationBar({
  page,
  pageSize,
  totalRows,
  pageSizes,
  onPageChange,
  onPageSizeChange,
}: {
  page: number;
  pageSize: number;
  totalRows: number;
  pageSizes?: number[];
  onPageChange: (page: number) => void;
  onPageSizeChange?: (size: number) => void;
}) {
  const totalPages = Math.max(1, Math.ceil(totalRows / pageSize));
  const safePage = Math.min(page, totalPages - 1);
  const start = safePage * pageSize;

  return (
    <div className="flex items-center justify-between text-sm">
      <div className="flex items-center gap-2 text-muted-foreground">
        <span>
          {start + 1}&ndash;{Math.min(start + pageSize, totalRows)} of {totalRows}
        </span>
        {pageSizes && onPageSizeChange && (
          <select
            value={pageSize}
            onChange={(e) => onPageSizeChange(Number(e.target.value))}
            className="px-2 py-1 rounded border border-input bg-background text-sm"
          >
            {pageSizes.map((s) => (
              <option key={s} value={s}>
                {s} / page
              </option>
            ))}
          </select>
        )}
      </div>
      <div className="flex items-center gap-1">
        <button
          onClick={() => onPageChange(0)}
          disabled={safePage === 0}
          className="px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground disabled:opacity-30"
        >
          First
        </button>
        <button
          onClick={() => onPageChange(safePage - 1)}
          disabled={safePage === 0}
          className="px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground disabled:opacity-30"
        >
          Prev
        </button>
        <span className="px-2 text-muted-foreground">
          Page {safePage + 1} of {totalPages}
        </span>
        <button
          onClick={() => onPageChange(safePage + 1)}
          disabled={safePage >= totalPages - 1}
          className="px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground disabled:opacity-30"
        >
          Next
        </button>
        <button
          onClick={() => onPageChange(totalPages - 1)}
          disabled={safePage >= totalPages - 1}
          className="px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground disabled:opacity-30"
        >
          Last
        </button>
      </div>
    </div>
  );
}
