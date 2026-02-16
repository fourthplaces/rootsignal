"use client";

import { useState, useEffect, useRef, useCallback } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";

interface Source {
  id: string;
  name: string;
  sourceType: string;
  url: string | null;
  handle: string | null;
  nextRunAt: string | null;
  consecutiveMisses: number;
  lastScrapedAt: string | null;
  isActive: boolean;
  entityId: string | null;
  signalCount: number;
}

function formatTimeUntil(dateStr: string | null): string {
  if (!dateStr) return "Now";
  const diff = new Date(dateStr).getTime() - Date.now();
  const absDiff = Math.abs(diff);
  const seconds = Math.floor(absDiff / 1000);
  const minutes = Math.floor(seconds / 60);
  const hours = Math.floor(minutes / 60);
  const days = Math.floor(hours / 24);

  let label: string;
  if (days > 0) label = `${days}d ${hours % 24}h`;
  else if (hours > 0) label = `${hours}h ${minutes % 60}m`;
  else if (minutes > 0) label = `${minutes}m`;
  else label = `${seconds}s`;

  return diff <= 0 ? `${label} overdue` : `in ${label}`;
}

const SOURCE_TYPES = [
  { value: "website", label: "Website" },
  { value: "web_search", label: "Web Search" },
  { value: "instagram", label: "Instagram" },
  { value: "facebook", label: "Facebook" },
  { value: "x", label: "X" },
  { value: "tiktok", label: "TikTok" },
  { value: "gofundme", label: "GoFundMe" },
];

interface WorkflowInfo {
  workflowType: string;
  stage: string | null;
}

const WORKFLOW_LABELS: Record<string, string> = {
  ScrapeWorkflow: "Scrape",
};

const STAGE_LABELS: Record<string, string> = {
  scraping: "Scraping pages",
  extracting: "Extracting signals",
  discovering: "Discovering sources",
};

function formatWorkflow(w: WorkflowInfo): string {
  const type = WORKFLOW_LABELS[w.workflowType] || w.workflowType;
  const stage = w.stage ? STAGE_LABELS[w.stage] || w.stage : null;
  return stage ? `${type}: ${stage}` : type;
}

async function fetchAllActiveWorkflows(): Promise<Record<string, WorkflowInfo[]>> {
  try {
    const res = await fetch("/api/graphql", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        query: `query { activeWorkflows { workflowType sourceId status stage } }`,
      }),
    });
    const data = await res.json();
    const workflows = data.data?.activeWorkflows ?? [];
    const map: Record<string, WorkflowInfo[]> = {};
    for (const w of workflows) {
      if (!map[w.sourceId]) map[w.sourceId] = [];
      map[w.sourceId].push({ workflowType: w.workflowType, stage: w.stage });
    }
    return map;
  } catch {
    return {};
  }
}

export function SourcesTable({
  sources,
  allSources,
  initialQuery = "",
  activeType,
  workflowsBySource: initialWorkflows = {},
}: {
  sources: Source[];
  allSources: Source[];
  initialQuery?: string;
  activeType: string | null;
  workflowsBySource?: Record<string, WorkflowInfo[]>;
}) {
  const router = useRouter();
  const [liveWorkflows, setLiveWorkflows] = useState(initialWorkflows);
  const hadWorkflows = useRef(Object.keys(initialWorkflows).length > 0);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const hasActive = Object.keys(liveWorkflows).length > 0;

  // Start/stop polling based on active workflows
  useEffect(() => {
    if (!hasActive && !hadWorkflows.current) return;

    async function poll() {
      const map = await fetchAllActiveWorkflows();
      setLiveWorkflows(map);
      if (Object.keys(map).length === 0) {
        hadWorkflows.current = false;
        if (intervalRef.current) clearInterval(intervalRef.current);
        intervalRef.current = null;
        router.refresh();
      }
    }

    hadWorkflows.current = true;
    intervalRef.current = setInterval(poll, 2000);
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, [hasActive, router]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [showConfirm, setShowConfirm] = useState(false);
  const [searchInput, setSearchInput] = useState(initialQuery);
  const [showAddModal, setShowAddModal] = useState(false);
  const [addInput, setAddInput] = useState("");
  const [addSaving, setAddSaving] = useState(false);
  const [addError, setAddError] = useState("");

  const allSelected = sources.length > 0 && selected.size === sources.length;

  function toggleAll() {
    if (allSelected) {
      setSelected(new Set());
    } else {
      setSelected(new Set(sources.map((s) => s.id)));
    }
  }

  function toggleOne(id: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }

  function startPolling() {
    hadWorkflows.current = true;
    // Trigger a fresh poll immediately
    fetchAllActiveWorkflows().then(setLiveWorkflows);
  }

  async function batchMutation(query: string, label: string, triggersWorkflow = false) {
    setBusy(true);
    try {
      const res = await fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query,
          variables: { ids: Array.from(selected) },
        }),
      });
      const data = await res.json();
      if (data.errors) throw new Error(data.errors[0].message);
      setSelected(new Set());
      if (triggersWorkflow) {
        startPolling();
      } else {
        router.refresh();
      }
    } catch (err) {
      console.error(`${label} failed:`, err);
    } finally {
      setBusy(false);
      setShowConfirm(false);
    }
  }

  function handleDelete() {
    batchMutation(
      `mutation DeleteSources($ids: [UUID!]!) { deleteSources(ids: $ids) }`,
      "Delete",
    );
  }

  function handleScrape() {
    batchMutation(
      `mutation ScrapeSources($ids: [UUID!]!) { scrapeSources(ids: $ids) }`,
      "Scrape",
      true,
    );
  }

  function handleActivate() {
    batchMutation(
      `mutation ActivateSources($ids: [UUID!]!) { activateSources(ids: $ids) }`,
      "Activate",
    );
  }

  function handleDeactivate() {
    batchMutation(
      `mutation DeactivateSources($ids: [UUID!]!) { deactivateSources(ids: $ids) }`,
      "Deactivate",
    );
  }

  function handleSearch(e: React.FormEvent) {
    e.preventDefault();
    const params = new URLSearchParams(window.location.search);
    if (searchInput.trim()) {
      params.set("q", searchInput.trim());
    } else {
      params.delete("q");
    }
    const qs = params.toString();
    router.push(`/sources${qs ? `?${qs}` : ""}`);
  }

  async function handleAddSource(e: React.FormEvent) {
    e.preventDefault();
    if (!addInput.trim()) return;
    setAddSaving(true);
    setAddError("");

    try {
      const res = await fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `mutation AddSource($input: String!) {
            addSource(input: $input) { id name sourceType }
          }`,
          variables: { input: addInput.trim() },
        }),
      });
      const data = await res.json();
      if (data.errors) throw new Error(data.errors[0].message);
      setShowAddModal(false);
      setAddInput("");
      router.refresh();
    } catch (err) {
      setAddError(err instanceof Error ? err.message : "Failed to add source");
    } finally {
      setAddSaving(false);
    }
  }

  return (
    <>
      <div className="mb-6 flex items-center justify-between">
        <h1 className="text-2xl font-bold">Sources</h1>
        <button
          onClick={() => setShowAddModal(true)}
          className="rounded bg-green-700 px-4 py-2 text-sm text-white hover:bg-green-800"
        >
          Add Source
        </button>
      </div>

      <div className="mb-4 flex flex-wrap gap-2">
        <Link
          href={initialQuery ? `/sources?q=${encodeURIComponent(initialQuery)}` : "/sources"}
          className={`rounded-full px-3 py-1 text-sm font-medium ${
            !activeType
              ? "bg-green-700 text-white"
              : "bg-gray-100 text-gray-600 hover:bg-gray-200"
          }`}
        >
          All ({allSources.length})
        </Link>
        {SOURCE_TYPES.map((t) => {
          const count = allSources.filter((s) => s.sourceType === t.value).length;
          if (count === 0) return null;
          const href = initialQuery
            ? `/sources?q=${encodeURIComponent(initialQuery)}&type=${t.value}`
            : `/sources?type=${t.value}`;
          return (
            <Link
              key={t.value}
              href={href}
              className={`rounded-full px-3 py-1 text-sm font-medium ${
                activeType === t.value
                  ? "bg-green-700 text-white"
                  : "bg-gray-100 text-gray-600 hover:bg-gray-200"
              }`}
            >
              {t.label} ({count})
            </Link>
          );
        })}
      </div>

      {showAddModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="w-full max-w-md rounded-lg bg-white p-6 shadow-xl">
            <h3 className="text-lg font-semibold">Add Source</h3>
            {addError && <p className="mt-2 text-sm text-red-600">{addError}</p>}
            <form onSubmit={handleAddSource} className="mt-4">
              <input
                value={addInput}
                onChange={(e) => setAddInput(e.target.value)}
                placeholder="https://example.com or a search query"
                required
                autoFocus
                className="block w-full rounded border border-gray-300 px-3 py-2"
              />
              <p className="mt-1 text-xs text-gray-500">
                Paste a URL or type a search query.
              </p>
              <div className="mt-4 flex justify-end gap-2">
                <button
                  type="button"
                  onClick={() => { setShowAddModal(false); setAddInput(""); setAddError(""); }}
                  className="rounded border border-gray-300 px-4 py-2 text-sm hover:bg-gray-50"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  disabled={addSaving}
                  className="rounded bg-green-700 px-4 py-2 text-sm text-white hover:bg-green-800 disabled:opacity-50"
                >
                  {addSaving ? "Adding..." : "Add"}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      <form onSubmit={handleSearch} className="mb-4 flex gap-2">
        <input
          type="text"
          value={searchInput}
          onChange={(e) => setSearchInput(e.target.value)}
          placeholder="Search sources by topic (e.g. food pantry, youth programs)..."
          className="flex-1 rounded-lg border border-gray-300 px-4 py-2 text-sm focus:border-green-500 focus:outline-none focus:ring-1 focus:ring-green-500"
        />
        <button
          type="submit"
          className="rounded-lg bg-green-700 px-4 py-2 text-sm text-white hover:bg-green-800"
        >
          Search
        </button>
        {initialQuery && (
          <button
            type="button"
            onClick={() => {
              setSearchInput("");
              router.push("/sources");
            }}
            className="rounded-lg border border-gray-300 px-4 py-2 text-sm text-gray-600 hover:bg-gray-50"
          >
            Clear
          </button>
        )}
      </form>

      {selected.size > 0 && (
        <div className="sticky top-0 z-10 mb-4 flex items-center gap-3 rounded-lg border border-gray-200 bg-gray-50 px-4 py-2 shadow-sm">
          <span className="text-sm font-medium text-gray-800">
            {selected.size} selected
          </span>
          <button
            onClick={handleScrape}
            disabled={busy}
            className="rounded bg-indigo-600 px-3 py-1 text-sm text-white hover:bg-indigo-700 disabled:opacity-50"
          >
            Run
          </button>
          <button
            onClick={handleActivate}
            disabled={busy}
            className="rounded bg-green-600 px-3 py-1 text-sm text-white hover:bg-green-700 disabled:opacity-50"
          >
            Activate
          </button>
          <button
            onClick={handleDeactivate}
            disabled={busy}
            className="rounded bg-yellow-600 px-3 py-1 text-sm text-white hover:bg-yellow-700 disabled:opacity-50"
          >
            Deactivate
          </button>
          <button
            onClick={() => setShowConfirm(true)}
            disabled={busy}
            className="rounded bg-red-600 px-3 py-1 text-sm text-white hover:bg-red-700 disabled:opacity-50"
          >
            Delete
          </button>
          <button
            onClick={() => setSelected(new Set())}
            className="text-sm text-gray-500 hover:text-gray-700"
          >
            Clear
          </button>
        </div>
      )}

      {showConfirm && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="w-full max-w-sm rounded-lg bg-white p-6 shadow-xl">
            <h3 className="text-lg font-semibold">Delete sources?</h3>
            <p className="mt-2 text-sm text-gray-600">
              This will permanently delete {selected.size} source
              {selected.size === 1 ? "" : "s"} and all associated data. This
              cannot be undone.
            </p>
            <div className="mt-4 flex justify-end gap-2">
              <button
                onClick={() => setShowConfirm(false)}
                disabled={busy}
                className="rounded border border-gray-300 px-4 py-2 text-sm hover:bg-gray-50"
              >
                Cancel
              </button>
              <button
                onClick={handleDelete}
                disabled={busy}
                className="rounded bg-red-600 px-4 py-2 text-sm text-white hover:bg-red-700 disabled:opacity-50"
              >
                {busy ? "Deleting..." : "Delete"}
              </button>
            </div>
          </div>
        </div>
      )}

      <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
        <table className="min-w-full divide-y divide-gray-200">
          <thead className="bg-gray-50">
            <tr>
              <th className="w-10 px-4 py-3">
                <input
                  type="checkbox"
                  checked={allSelected}
                  onChange={toggleAll}
                  className="rounded border-gray-300"
                />
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Name
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Type
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                URL / Handle
              </th>
              <th className="w-36 px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Next Run
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Last Scraped
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Signals
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Active
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Status
              </th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-200">
            {sources.map((s) => (
              <tr key={s.id} className="hover:bg-gray-50">
                <td className="px-4 py-3">
                  <input
                    type="checkbox"
                    checked={selected.has(s.id)}
                    onChange={() => toggleOne(s.id)}
                    className="rounded border-gray-300"
                  />
                </td>
                <td className="px-4 py-3 text-sm font-medium">
                  <Link
                    href={`/sources/${s.id}`}
                    className="text-green-700 hover:underline"
                  >
                    {s.name}
                  </Link>
                </td>
                <td className="px-4 py-3">
                  <span className="rounded bg-gray-100 px-2 py-0.5 text-xs text-gray-700">
                    {s.sourceType}
                  </span>
                </td>
                <td className="px-4 py-3 text-sm text-gray-600">
                  {s.url || s.handle || "—"}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                  {formatTimeUntil(s.nextRunAt)}
                  {s.consecutiveMisses > 0 && (
                    <span className="ml-1 text-xs text-orange-500">
                      ({s.consecutiveMisses} miss{s.consecutiveMisses === 1 ? "" : "es"})
                    </span>
                  )}
                </td>
                <td className="px-4 py-3 text-sm text-gray-500">
                  {s.lastScrapedAt
                    ? new Date(s.lastScrapedAt).toLocaleDateString()
                    : "Never"}
                </td>
                <td className="px-4 py-3">
                  {s.signalCount > 0 ? (
                    <span className="rounded bg-green-100 px-2 py-0.5 text-xs font-medium text-green-700">
                      {s.signalCount}
                    </span>
                  ) : (
                    <span className="text-xs text-gray-400">0</span>
                  )}
                </td>
                <td className="px-4 py-3">
                  {s.isActive ? (
                    <span className="text-green-600">Yes</span>
                  ) : (
                    <span className="text-gray-400">No</span>
                  )}
                </td>
                <td className="px-4 py-3">
                  {liveWorkflows[s.id] ? (
                    <div className="flex flex-col gap-0.5">
                      {liveWorkflows[s.id].map((w, j) => (
                        <span key={j} className="inline-flex items-center gap-1 text-xs text-indigo-600">
                          <span className="inline-block h-2 w-2 animate-pulse rounded-full bg-indigo-500" />
                          {formatWorkflow(w)}
                        </span>
                      ))}
                    </div>
                  ) : null}
                </td>
              </tr>
            ))}
            {sources.length === 0 && (
              <tr>
                <td
                  colSpan={9}
                  className="px-4 py-8 text-center text-sm text-gray-500"
                >
                  No sources yet. Create one to start scraping.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </>
  );
}
