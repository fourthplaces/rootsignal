import { useState, useMemo, useCallback, useEffect, useRef } from "react";
import { Link, useSearchParams, useNavigate } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import { ADMIN_REGION_SOURCES } from "@/graphql/queries";
import { ADD_SOURCE, UPDATE_SOURCE, DELETE_SOURCE, RUN_SCOUT_SOURCE } from "@/graphql/mutations";
import { DataTable, type Column } from "@/components/DataTable";
import { InvestigateDrawer } from "@/components/InvestigateDrawer";
import { PromptDialog } from "@/components/PromptDialog";

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

const PAGE_SIZE_INCREMENT = 50;

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

function BatchToolbar({
  count,
  onActivate,
  onDeactivate,
  onSetWeight,
  onSetPenalty,
  onDelete,
  onInvestigate,
  onClear,
  busy,
}: {
  count: number;
  onActivate: () => void;
  onDeactivate: () => void;
  onSetWeight: () => void;
  onSetPenalty: () => void;
  onDelete: () => void;
  onInvestigate: () => void;
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
      <div className="h-4 w-px bg-border" />
      <button
        onClick={onInvestigate}
        className="text-xs px-2 py-1 rounded border border-amber-500/30 text-amber-400 hover:text-amber-300 hover:bg-amber-500/10"
      >
        Investigate
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


const ACTIVE_FILTERS = ["all", "active", "inactive"] as const;
type ActiveFilter = (typeof ACTIVE_FILTERS)[number];

export function SourcesPage() {
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const rawFilter = searchParams.get("filter");
  const activeFilter: ActiveFilter = ACTIVE_FILTERS.includes(rawFilter as ActiveFilter) ? (rawFilter as ActiveFilter) : "all";
  const setActiveFilter = (f: ActiveFilter) => setSearchParams((prev) => { prev.set("filter", f); return prev; }, { replace: true });

  // Search with debounce
  const [searchInput, setSearchInput] = useState("");
  const [debouncedSearch, setDebouncedSearch] = useState("");

  useEffect(() => {
    const timer = setTimeout(() => setDebouncedSearch(searchInput), 300);
    return () => clearTimeout(timer);
  }, [searchInput]);

  const { data, loading, refetch } = useQuery(ADMIN_REGION_SOURCES, {
    variables: { search: debouncedSearch || undefined },
  });
  const sources: Source[] = data?.adminRegionSources ?? [];

  const [updateSource] = useMutation(UPDATE_SOURCE);
  const [deleteSource] = useMutation(DELETE_SOURCE);
  const [addSource] = useMutation(ADD_SOURCE);
  const [runScoutSource] = useMutation(RUN_SCOUT_SOURCE);

  // Add source modal
  const [showAdd, setShowAdd] = useState(false);
  const [sourceUrl, setSourceUrl] = useState("");
  const [addError, setAddError] = useState<string | null>(null);

  // Sort
  const [sortKey, setSortKey] = useState<SortKey>("signalsProduced");
  const [sortDir, setSortDir] = useState<SortDir>("desc");

  // Selection
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [batchBusy, setBatchBusy] = useState(false);
  const lastSelectedIndexRef = useRef<number | null>(null);

  // Infinite scroll
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE_INCREMENT);
  const sentinelRef = useRef<HTMLDivElement>(null);

  // Investigation drawer
  const [investigateIds, setInvestigateIds] = useState<string[] | null>(null);

  // Dialogs
  const [dialog, setDialog] = useState<{
    type: "delete" | "delete-batch" | "set-weight" | "set-penalty";
    id?: string;
  } | null>(null);

  const handleSort = (key: string) => {
    const k = key as SortKey;
    if (sortKey === k) {
      setSortDir(sortDir === "asc" ? "desc" : "asc");
    } else {
      setSortKey(k);
      setSortDir("desc");
    }
    setVisibleCount(PAGE_SIZE_INCREMENT);
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

  // Visible slice
  const visibleSlice = filtered.slice(0, visibleCount);
  const hasMore = visibleCount < filtered.length;

  // IntersectionObserver for infinite scroll
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting && hasMore) {
          setVisibleCount((prev) => prev + PAGE_SIZE_INCREMENT);
        }
      },
      { rootMargin: "200px" },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [hasMore]);

  // IDs on visible rows (for select-all)
  const visibleIds = useMemo(() => new Set(visibleSlice.map((s) => s.id)), [visibleSlice]);
  const allPageSelected = visibleSlice.length > 0 && visibleSlice.every((s) => selected.has(s.id));
  const somePageSelected = visibleSlice.some((s) => selected.has(s.id));

  const toggleSelectAll = useCallback(() => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (allPageSelected) {
        visibleIds.forEach((id) => next.delete(id));
      } else {
        visibleIds.forEach((id) => next.add(id));
      }
      return next;
    });
  }, [allPageSelected, visibleIds]);

  const toggleSelect = useCallback((id: string, checked: boolean, shiftKey: boolean) => {
    const clickedIndex = visibleSlice.findIndex((s) => s.id === id);
    if (shiftKey && lastSelectedIndexRef.current != null && clickedIndex !== -1) {
      const from = Math.min(lastSelectedIndexRef.current, clickedIndex);
      const to = Math.max(lastSelectedIndexRef.current, clickedIndex);
      setSelected((prev) => {
        const next = new Set(prev);
        for (let i = from; i <= to; i++) {
          next.add(visibleSlice[i].id);
        }
        return next;
      });
    } else {
      setSelected((prev) => {
        const next = new Set(prev);
        if (checked) next.add(id);
        else next.delete(id);
        return next;
      });
    }
    if (clickedIndex !== -1) lastSelectedIndexRef.current = clickedIndex;
  }, [visibleSlice]);

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
      const { data: result } = await addSource({
        variables: { url: sourceUrl },
      });
      const newId = result?.addSource?.sourceId;
      setSourceUrl("");
      setShowAdd(false);
      if (newId) {
        navigate(`/sources/${newId}`);
      } else {
        refetch();
      }
    } catch (err: unknown) {
      setAddError(err instanceof Error ? err.message : "Failed to add source");
    }
  };

  // Column definitions
  const columns: Column<Source>[] = useMemo(
    () => [
      {
        key: "canonicalValue",
        label: "Source",
        resizable: true,
        defaultWidth: 260,
        render: (s) => (
          <span className="inline-flex items-center gap-1.5 truncate" title={s.canonicalValue}>
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
        ),
      },
      {
        key: "sourceLabel",
        label: "Type",
        resizable: true,
        defaultWidth: 80,
        render: (s) => <span className="text-muted-foreground">{s.sourceLabel}</span>,
      },
      {
        key: "active",
        label: "Status",
        resizable: true,
        defaultWidth: 80,
        render: (s) => (
          <StatusToggle source={s} onUpdate={handleUpdate} />
        ),
      },
      {
        key: "weight",
        label: "Weight",
        resizable: true,
        defaultWidth: 70,
        render: (s) => (
          <EditableCell value={s.weight} onSave={(v) => handleUpdate(s.id, { weight: v })} />
        ),
      },
      {
        key: "qualityPenalty",
        label: "Penalty",
        resizable: true,
        defaultWidth: 70,
        render: (s) => (
          <EditableCell value={s.qualityPenalty} onSave={(v) => handleUpdate(s.id, { qualityPenalty: v })} />
        ),
      },
      {
        key: "effectiveWeight",
        label: "Eff. Wt",
        resizable: true,
        defaultWidth: 70,
        render: (s) => <span className="tabular-nums">{s.effectiveWeight.toFixed(2)}</span>,
      },
      {
        key: "signalsProduced",
        label: "Signals",
        resizable: true,
        defaultWidth: 70,
        align: "right",
        render: (s) => <span className="tabular-nums">{s.signalsProduced}</span>,
      },
      {
        key: "cadenceHours",
        label: "Cadence",
        resizable: true,
        defaultWidth: 80,
        render: (s) => <span className="tabular-nums text-muted-foreground">{s.cadenceHours}h</span>,
      },
      {
        key: "lastScraped",
        label: "Last Scraped",
        resizable: true,
        defaultWidth: 120,
        render: (s) => <span className="text-muted-foreground whitespace-nowrap">{formatDate(s.lastScraped)}</span>,
      },
      {
        key: "discoveryMethod",
        label: "Discovery",
        resizable: true,
        defaultWidth: 100,
        render: (s) => <span className="text-muted-foreground text-xs">{s.discoveryMethod}</span>,
      },
    ],
    // handleUpdate is stable enough — it only closes over refetch/updateSource which don't change
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <span className="text-sm text-muted-foreground">{sources.length} sources</span>
        <button
          onClick={() => setShowAdd(!showAdd)}
          className="px-3 py-1.5 rounded-md bg-primary text-primary-foreground text-sm hover:bg-primary/90"
        >
          Add Source
        </button>
      </div>

      {showAdd && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <form onSubmit={handleAdd} className="bg-card border border-border rounded-lg p-6 max-w-md w-full space-y-4">
            <h2 className="font-semibold">Add Source</h2>
            <p className="text-sm text-muted-foreground">Enter a URL or search query (e.g. <code className="text-xs bg-muted px-1 py-0.5 rounded">site:linktr.ee mutual aid Minneapolis</code>)</p>
            <input
              type="text"
              value={sourceUrl}
              onChange={(e) => setSourceUrl(e.target.value)}
              placeholder="https://... or search query"
              className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm"
              autoFocus
              required
            />
            {addError && <p className="text-xs text-red-400">{addError}</p>}
            <div className="flex gap-2 justify-end">
              <button
                type="button"
                onClick={() => { setShowAdd(false); setAddError(null); }}
                className="px-3 py-1.5 rounded-md border border-border text-sm text-muted-foreground hover:text-foreground"
              >
                Cancel
              </button>
              <button
                type="submit"
                className="px-3 py-1.5 rounded-md bg-primary text-primary-foreground text-sm hover:bg-primary/90"
              >
                Add
              </button>
            </div>
          </form>
        </div>
      )}

      {/* Filter */}
      <div className="flex items-center gap-2">
        <input
          type="text"
          value={searchInput}
          onChange={(e) => { setSearchInput(e.target.value); setVisibleCount(PAGE_SIZE_INCREMENT); }}
          placeholder="Search sources..."
          className="px-3 py-1.5 rounded-md border border-input bg-background text-sm w-64"
        />
        <div className="h-4 w-px bg-border" />
        {(["all", "active", "inactive"] as const).map((f) => (
          <button
            key={f}
            onClick={() => { setActiveFilter(f); setVisibleCount(PAGE_SIZE_INCREMENT); }}
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
          onInvestigate={() => setInvestigateIds([...selected])}
          onClear={clearSelection}
        />
      )}

      <DataTable<Source>
        columns={columns}
        data={visibleSlice}
        getRowKey={(s) => s.id}
        sortKey={sortKey}
        sortDir={sortDir}
        onSort={handleSort}
        rowClassName={(s) => (!s.active ? "opacity-50" : "")}
        loading={loading}
        emptyMessage="No sources match the current filter."
        headerPrefix={
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
        }
        renderRowPrefix={(s) => (
          <td className="px-4 py-2">
            <input
              type="checkbox"
              checked={selected.has(s.id)}
              onClick={(e) => {
                const target = e.target as HTMLInputElement;
                toggleSelect(s.id, target.checked, e.shiftKey);
              }}
              readOnly
              className="rounded border-border"
            />
          </td>
        )}
        headerSuffix={
          <th className="px-4 py-2 font-medium text-right overflow-hidden" style={{ width: 120 }}>Actions</th>
        }
        renderRowSuffix={(s) => (
          <td className="px-4 py-2 text-right space-x-2">
            <button
              onClick={async () => {
                try {
                  await runScoutSource({ variables: { sourceIds: [s.id] } });
                } catch { /* error shown by Apollo */ }
              }}
              className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50"
            >
              Scout
            </button>
            <Link
              to={`/events?q=${encodeURIComponent(s.canonicalValue)}`}
              className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50"
            >
              Events
            </Link>
            <button
              onClick={() => setDialog({ type: "delete", id: s.id })}
              className="text-xs px-2 py-1 rounded border border-red-500/30 text-red-400 hover:text-red-300 hover:bg-red-500/10"
            >
              Delete
            </button>
          </td>
        )}
        pagination={{
          mode: "infinite",
          visibleCount: visibleSlice.length,
          totalRows: filtered.length,
          sentinelRef,
          hasMore,
        }}
      />

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

      {/* Investigation drawer */}
      {investigateIds && (
        <div className="fixed inset-0 z-50 flex">
          <div
            className="flex-1 bg-black/40"
            onClick={() => setInvestigateIds(null)}
          />
          <div className="w-[520px] bg-card border-l border-border flex flex-col">
            <InvestigateDrawer
              key={investigateIds.join(",")}
              investigation={{
                mode: "sources",
                sourceIds: investigateIds,
                sourceLabel: `${investigateIds.length} source${investigateIds.length === 1 ? "" : "s"}`,
              }}
              onClose={() => {
                setInvestigateIds(null);
                refetch();
              }}
            />
          </div>
        </div>
      )}
    </div>
  );
}

function StatusToggle({
  source: s,
  onUpdate,
}: {
  source: Source;
  onUpdate: (id: string, fields: { active?: boolean }) => Promise<void>;
}) {
  const [toggling, setToggling] = useState(false);

  const handleToggle = async () => {
    setToggling(true);
    await onUpdate(s.id, { active: !s.active });
    setToggling(false);
  };

  return (
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
  );
}
