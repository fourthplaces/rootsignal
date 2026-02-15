"use client";

import { useState } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";

interface Source {
  id: string;
  name: string;
  sourceType: string;
  url: string | null;
  handle: string | null;
  cadenceHours: number;
  lastScrapedAt: string | null;
  isActive: boolean;
  entityId: string | null;
  qualificationStatus: string;
}

export function SourcesTable({ sources }: { sources: Source[] }) {
  const router = useRouter();
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [showConfirm, setShowConfirm] = useState(false);

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

  async function batchMutation(query: string, label: string) {
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
      router.refresh();
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

  function handleQualify() {
    batchMutation(
      `mutation QualifySources($ids: [UUID!]!) { qualifySources(ids: $ids) }`,
      "Qualify",
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

  return (
    <>
      {selected.size > 0 && (
        <div className="mb-4 flex items-center gap-3 rounded-lg border border-gray-200 bg-gray-50 px-4 py-2">
          <span className="text-sm font-medium text-gray-800">
            {selected.size} selected
          </span>
          <button
            onClick={handleQualify}
            disabled={busy}
            className="rounded bg-blue-600 px-3 py-1 text-sm text-white hover:bg-blue-700 disabled:opacity-50"
          >
            Qualify
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
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Cadence
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Last Scraped
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Qualification
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Active
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
                <td className="px-4 py-3 text-sm text-gray-600">
                  {s.cadenceHours}h
                </td>
                <td className="px-4 py-3 text-sm text-gray-500">
                  {s.lastScrapedAt
                    ? new Date(s.lastScrapedAt).toLocaleDateString()
                    : "Never"}
                </td>
                <td className="px-4 py-3">
                  <span
                    className={`rounded px-2 py-0.5 text-xs font-medium ${
                      s.qualificationStatus === "green"
                        ? "bg-green-100 text-green-700"
                        : s.qualificationStatus === "yellow"
                          ? "bg-yellow-100 text-yellow-700"
                          : s.qualificationStatus === "red"
                            ? "bg-red-100 text-red-700"
                            : "bg-gray-100 text-gray-500"
                    }`}
                  >
                    {s.qualificationStatus}
                  </span>
                </td>
                <td className="px-4 py-3">
                  {s.isActive ? (
                    <span className="text-green-600">Yes</span>
                  ) : (
                    <span className="text-gray-400">No</span>
                  )}
                </td>
              </tr>
            ))}
            {sources.length === 0 && (
              <tr>
                <td
                  colSpan={8}
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
