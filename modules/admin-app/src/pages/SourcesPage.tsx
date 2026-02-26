import { useState, useMemo, useCallback } from "react";
import { Link } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import { ADMIN_REGION_SOURCES } from "@/graphql/queries";
import { ADD_SOURCE, UPDATE_SOURCE, DELETE_SOURCE } from "@/graphql/mutations";

type Source = {
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
  active: boolean;
};

type SortKey = keyof Source;
type SortDir = "asc" | "desc";

const PAGE_SIZES = [25, 50, 100] as const;

const formatDate = (d: string | null) => {
  if (!d) return "Never";
  return new Date(d).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
};

function EditableCell({
  value,
  onSave,
}: {
  value: number;
  onSave: (v: number) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(String(value));

  const commit = () => {
    const num = parseFloat(draft);
    if (!isNaN(num) && num !== value) {
      onSave(num);
    }
    setEditing(false);
  };

  if (editing) {
    return (
      <input
        type="number"
        step="0.01"
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
          if (e.key === "Escape") setEditing(false);
        }}
        className="w-20 px-1 py-0.5 rounded border border-input bg-background text-sm tabular-nums"
        autoFocus
      />
    );
  }

  return (
    <button
      onClick={() => {
        setDraft(String(value));
        setEditing(true);
      }}
      className="tabular-nums hover:underline cursor-pointer"
      title="Click to edit"
    >
      {value.toFixed(2)}
    </button>
  );
}

function SourceRow({
  source: s,
  selected,
  onSelect,
  onUpdate,
  onDelete,
}: {
  source: Source;
  selected: boolean;
  onSelect: (id: string, checked: boolean) => void;
  onUpdate: (id: string, fields: { active?: boolean; weight?: number; qualityPenalty?: number }) => Promise<void>;
  onDelete: (id: string) => void;
}) {
  const [toggling, setToggling] = useState(false);

  const handleToggle = async () => {
    setToggling(true);
    await onUpdate(s.id, { active: !s.active });
    setToggling(false);
  };

  return (
    <tr className={`border-b border-border last:border-0 hover:bg-muted/30 ${!s.active ? "opacity-50" : ""}`}>
      <td className="px-4 py-2">
        <input
          type="checkbox"
          checked={selected}
          onChange={(e) => onSelect(s.id, e.target.checked)}
          className="rounded border-border"
        />
      </td>
      <td className="px-4 py-2 max-w-[260px] truncate" title={s.canonicalValue}>
        <span className="inline-flex items-center gap-1.5">
          <Link
            to={`/sources/${s.id}`}
            className="text-blue-400 hover:underline truncate"
          >
            {s.canonicalValue}
          </Link>
          {s.url && (
            <a
              href={s.url}
              target="_blank"
              rel="noopener noreferrer"
              className="text-muted-foreground hover:text-foreground shrink-0"
              title="Open externally"
            >
              <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" />
              </svg>
            </a>
          )}
        </span>
      </td>
      <td className="px-4 py-2 text-muted-foreground">{s.sourceLabel}</td>
      <td className="px-4 py-2">
        <button
          onClick={handleToggle}
          disabled={toggling}
          className={`text-xs px-2 py-0.5 rounded-full border ${
            s.active
              ? "bg-green-900/30 text-green-400 border-green-500/30"
              : "bg-muted text-muted-foreground border-border"
          } disabled:opacity-50`}
        >
          {s.active ? "Active" : "Inactive"}
        </button>
      </td>
      <td className="px-4 py-2">
        <EditableCell
          value={s.weight}
          onSave={(v) => onUpdate(s.id, { weight: v })}
        />
      </td>
      <td className="px-4 py-2">
        <EditableCell
          value={s.qualityPenalty}
          onSave={(v) => onUpdate(s.id, { qualityPenalty: v })}
        />
      </td>
      <td className="px-4 py-2 tabular-nums">{s.effectiveWeight.toFixed(2)}</td>
      <td className="px-4 py-2 tabular-nums text-right">{s.signalsProduced}</td>
      <td className="px-4 py-2 text-muted-foreground tabular-nums">{s.cadenceHours}h</td>
      <td className="px-4 py-2 text-muted-foreground whitespace-nowrap">{formatDate(s.lastScraped)}</td>
      <td className="px-4 py-2 text-muted-foreground text-xs">{s.discoveryMethod}</td>
      <td className="px-4 py-2 text-right">
        <button
          onClick={() => onDelete(s.id)}
          className="text-xs px-2 py-1 rounded border border-red-500/30 text-red-400 hover:text-red-300 hover:bg-red-500/10"
        >
          Delete
        </button>
      </td>
    </tr>
  );
}

function BatchToolbar({
  count,
  onActivate,
  onDeactivate,
  onSetWeight,
  onSetPenalty,
  onDelete,
  onClear,
  busy,
}: {
  count: number;
  onActivate: () => void;
  onDeactivate: () => void;
  onSetWeight: () => void;
  onSetPenalty: () => void;
  onDelete: () => void;
  onClear: () => void;
  busy: boolean;
}) {
  return (
    <div className="flex items-center gap-2 p-3 rounded-lg border border-blue-500/30 bg-blue-500/5">
      <span className="text-sm font-medium">{count} selected</span>
      <div className="h-4 w-px bg-border" />
      <button
        onClick={onActivate}
        disabled={busy}
        className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50 disabled:opacity-50"
      >
        Activate
      </button>
      <button
        onClick={onDeactivate}
        disabled={busy}
        className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50 disabled:opacity-50"
      >
        Deactivate
      </button>
      <button
        onClick={onSetWeight}
        disabled={busy}
        className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50 disabled:opacity-50"
      >
        Set Weight...
      </button>
      <button
        onClick={onSetPenalty}
        disabled={busy}
        className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50 disabled:opacity-50"
      >
        Set Penalty...
      </button>
      <button
        onClick={onDelete}
        disabled={busy}
        className="text-xs px-2 py-1 rounded border border-red-500/30 text-red-400 hover:text-red-300 hover:bg-red-500/10 disabled:opacity-50"
      >
        Delete
      </button>
      <div className="flex-1" />
      <button
        onClick={onClear}
        className="text-xs text-muted-foreground hover:text-foreground"
      >
        Clear selection
      </button>
      {busy && <span className="text-xs text-muted-foreground">Working...</span>}
    </div>
  );
}

function PromptDialog({
  title,
  description,
  onConfirm,
  onCancel,
  inputType,
}: {
  title: string;
  description: string;
  onConfirm: (value: string) => void;
  onCancel: () => void;
  inputType?: "number" | "confirm";
}) {
  const [value, setValue] = useState("");
  const isConfirm = inputType === "confirm";

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-card border border-border rounded-lg p-6 max-w-sm space-y-4">
        <h2 className="font-semibold">{title}</h2>
        <p className="text-sm text-muted-foreground">{description}</p>
        {!isConfirm && (
          <input
            type="number"
            step="0.01"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && value) onConfirm(value);
              if (e.key === "Escape") onCancel();
            }}
            className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm"
            autoFocus
          />
        )}
        <div className="flex gap-2 justify-end">
          <button
            onClick={onCancel}
            className="px-3 py-1.5 rounded-md border border-border text-sm text-muted-foreground hover:text-foreground"
          >
            Cancel
          </button>
          <button
            onClick={() => onConfirm(value)}
            disabled={!isConfirm && !value}
            className={`px-3 py-1.5 rounded-md text-sm text-white disabled:opacity-50 ${
              isConfirm ? "bg-red-600 hover:bg-red-700" : "bg-primary hover:bg-primary/90"
            }`}
          >
            {isConfirm ? "Delete" : "Apply"}
          </button>
        </div>
      </div>
    </div>
  );
}

export function SourcesPage() {
  const { data, loading, refetch } = useQuery(ADMIN_REGION_SOURCES);
  const sources: Source[] = data?.adminRegionSources ?? [];

  const [updateSource] = useMutation(UPDATE_SOURCE);
  const [deleteSource] = useMutation(DELETE_SOURCE);
  const [addSource] = useMutation(ADD_SOURCE);

  // Add source form
  const [showAdd, setShowAdd] = useState(false);
  const [sourceUrl, setSourceUrl] = useState("");
  const [sourceReason, setSourceReason] = useState("");
  const [addError, setAddError] = useState<string | null>(null);

  // Filter
  const [activeFilter, setActiveFilter] = useState<"all" | "active" | "inactive">("all");

  // Sort
  const [sortKey, setSortKey] = useState<SortKey>("signalsProduced");
  const [sortDir, setSortDir] = useState<SortDir>("desc");

  // Selection
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [batchBusy, setBatchBusy] = useState(false);

  // Pagination
  const [page, setPage] = useState(0);
  const [pageSize, setPageSize] = useState<number>(50);

  // Dialogs
  const [dialog, setDialog] = useState<{
    type: "delete" | "delete-batch" | "set-weight" | "set-penalty";
    id?: string;
  } | null>(null);

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
    let list = [...sources];
    if (activeFilter === "active") list = list.filter((s) => s.active);
    if (activeFilter === "inactive") list = list.filter((s) => !s.active);
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
      if (typeof av === "boolean" && typeof bv === "boolean") {
        return sortDir === "asc" ? Number(av) - Number(bv) : Number(bv) - Number(av);
      }
      return 0;
    });
    return list;
  }, [sources, activeFilter, sortKey, sortDir]);

  // Pagination derived
  const totalPages = Math.max(1, Math.ceil(filtered.length / pageSize));
  const safePageIndex = Math.min(page, totalPages - 1);
  const pageStart = safePageIndex * pageSize;
  const pageSlice = filtered.slice(pageStart, pageStart + pageSize);

  // IDs on current page (for select-all)
  const pageIds = useMemo(() => new Set(pageSlice.map((s) => s.id)), [pageSlice]);
  const allPageSelected = pageSlice.length > 0 && pageSlice.every((s) => selected.has(s.id));
  const somePageSelected = pageSlice.some((s) => selected.has(s.id));

  const toggleSelectAll = useCallback(() => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (allPageSelected) {
        pageIds.forEach((id) => next.delete(id));
      } else {
        pageIds.forEach((id) => next.add(id));
      }
      return next;
    });
  }, [allPageSelected, pageIds]);

  const toggleSelect = useCallback((id: string, checked: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (checked) next.add(id);
      else next.delete(id);
      return next;
    });
  }, []);

  const clearSelection = useCallback(() => setSelected(new Set()), []);

  const handleUpdate = async (
    id: string,
    fields: { active?: boolean; weight?: number; qualityPenalty?: number },
  ) => {
    await updateSource({ variables: { id, ...fields } });
    refetch();
  };

  const handleDelete = async (id: string) => {
    await deleteSource({ variables: { id } });
    setDialog(null);
    setSelected((prev) => {
      const next = new Set(prev);
      next.delete(id);
      return next;
    });
    refetch();
  };

  // Batch operations
  const batchUpdate = async (fields: { active?: boolean; weight?: number; qualityPenalty?: number }) => {
    setBatchBusy(true);
    const ids = [...selected];
    await Promise.all(
      ids.map((id) => updateSource({ variables: { id, ...fields } })),
    );
    refetch();
    setBatchBusy(false);
  };

  const batchDelete = async () => {
    setBatchBusy(true);
    const ids = [...selected];
    await Promise.all(ids.map((id) => deleteSource({ variables: { id } })));
    setDialog(null);
    clearSelection();
    refetch();
    setBatchBusy(false);
  };

  const handleAdd = async (e: React.FormEvent) => {
    e.preventDefault();
    setAddError(null);
    try {
      await addSource({
        variables: { url: sourceUrl, reason: sourceReason || undefined },
      });
      setSourceUrl("");
      setSourceReason("");
      setShowAdd(false);
      refetch();
    } catch (err: unknown) {
      setAddError(err instanceof Error ? err.message : "Failed to add source");
    }
  };

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
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Sources ({sources.length})</h1>
        <button
          onClick={() => setShowAdd(!showAdd)}
          className="px-3 py-1.5 rounded-md bg-primary text-primary-foreground text-sm hover:bg-primary/90"
        >
          Add Source
        </button>
      </div>

      {showAdd && (
        <form onSubmit={handleAdd} className="space-y-2 p-4 rounded-lg border border-border bg-card">
          <input
            type="url"
            value={sourceUrl}
            onChange={(e) => setSourceUrl(e.target.value)}
            placeholder="https://..."
            className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm"
            required
          />
          <input
            type="text"
            value={sourceReason}
            onChange={(e) => setSourceReason(e.target.value)}
            placeholder="Reason (optional)"
            className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm"
          />
          <div className="flex gap-2">
            <button
              type="submit"
              className="px-4 py-2 rounded-md bg-primary text-primary-foreground text-sm hover:bg-primary/90"
            >
              Add
            </button>
            <button
              type="button"
              onClick={() => setShowAdd(false)}
              className="px-4 py-2 rounded-md border border-border text-sm text-muted-foreground hover:text-foreground"
            >
              Cancel
            </button>
          </div>
          {addError && <p className="text-xs text-red-400">{addError}</p>}
        </form>
      )}

      {/* Filter */}
      <div className="flex gap-2">
        {(["all", "active", "inactive"] as const).map((f) => (
          <button
            key={f}
            onClick={() => { setActiveFilter(f); setPage(0); }}
            className={`px-3 py-1.5 rounded-md text-sm transition-colors ${
              activeFilter === f
                ? "bg-accent text-accent-foreground"
                : "text-muted-foreground hover:text-foreground hover:bg-accent/50"
            }`}
          >
            {f.charAt(0).toUpperCase() + f.slice(1)}
            {f === "all" && ` (${sources.length})`}
            {f === "active" && ` (${sources.filter((s) => s.active).length})`}
            {f === "inactive" && ` (${sources.filter((s) => !s.active).length})`}
          </button>
        ))}
      </div>

      {/* Batch toolbar */}
      {selected.size > 0 && (
        <BatchToolbar
          count={selected.size}
          busy={batchBusy}
          onActivate={() => batchUpdate({ active: true })}
          onDeactivate={() => batchUpdate({ active: false })}
          onSetWeight={() => setDialog({ type: "set-weight" })}
          onSetPenalty={() => setDialog({ type: "set-penalty" })}
          onDelete={() => setDialog({ type: "delete-batch" })}
          onClear={clearSelection}
        />
      )}

      {loading ? (
        <p className="text-muted-foreground">Loading sources...</p>
      ) : filtered.length === 0 ? (
        <p className="text-muted-foreground">No sources match the current filter.</p>
      ) : (
        <>
          <div className="rounded-lg border border-border overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border bg-muted/50 text-left text-muted-foreground">
                  <th className="px-4 py-2 w-8">
                    <input
                      type="checkbox"
                      checked={allPageSelected}
                      ref={(el) => {
                        if (el) el.indeterminate = somePageSelected && !allPageSelected;
                      }}
                      onChange={toggleSelectAll}
                      className="rounded border-border"
                    />
                  </th>
                  <SortHeader k="canonicalValue" label="Source" />
                  <SortHeader k="sourceLabel" label="Type" />
                  <SortHeader k="active" label="Status" />
                  <SortHeader k="weight" label="Weight" />
                  <SortHeader k="qualityPenalty" label="Penalty" />
                  <SortHeader k="effectiveWeight" label="Eff. Wt" />
                  <SortHeader k="signalsProduced" label="Signals" className="text-right" />
                  <SortHeader k="cadenceHours" label="Cadence" />
                  <SortHeader k="lastScraped" label="Last Scraped" />
                  <SortHeader k="discoveryMethod" label="Discovery" />
                  <th className="px-4 py-2 font-medium text-right">Actions</th>
                </tr>
              </thead>
              <tbody>
                {pageSlice.map((s) => (
                  <SourceRow
                    key={s.id}
                    source={s}
                    selected={selected.has(s.id)}
                    onSelect={toggleSelect}
                    onUpdate={handleUpdate}
                    onDelete={(id) => setDialog({ type: "delete", id })}
                  />
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

      {/* Single delete confirmation */}
      {dialog?.type === "delete" && dialog.id && (
        <PromptDialog
          title="Delete Source?"
          description="This will permanently remove this source and all its relationships. This cannot be undone."
          inputType="confirm"
          onCancel={() => setDialog(null)}
          onConfirm={() => handleDelete(dialog.id!)}
        />
      )}

      {/* Batch delete confirmation */}
      {dialog?.type === "delete-batch" && (
        <PromptDialog
          title={`Delete ${selected.size} Sources?`}
          description={`This will permanently remove ${selected.size} source(s) and all their relationships. This cannot be undone.`}
          inputType="confirm"
          onCancel={() => setDialog(null)}
          onConfirm={batchDelete}
        />
      )}

      {/* Set weight dialog */}
      {dialog?.type === "set-weight" && (
        <PromptDialog
          title={`Set Weight for ${selected.size} Sources`}
          description="Enter the new weight value to apply to all selected sources."
          onCancel={() => setDialog(null)}
          onConfirm={(v) => {
            const num = parseFloat(v);
            if (!isNaN(num)) {
              batchUpdate({ weight: num });
              setDialog(null);
            }
          }}
        />
      )}

      {/* Set penalty dialog */}
      {dialog?.type === "set-penalty" && (
        <PromptDialog
          title={`Set Quality Penalty for ${selected.size} Sources`}
          description="Enter the new quality penalty value to apply to all selected sources."
          onCancel={() => setDialog(null)}
          onConfirm={(v) => {
            const num = parseFloat(v);
            if (!isNaN(num)) {
              batchUpdate({ qualityPenalty: num });
              setDialog(null);
            }
          }}
        />
      )}
    </div>
  );
}
