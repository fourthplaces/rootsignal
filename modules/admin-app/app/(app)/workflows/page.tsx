"use client";

import { useState } from "react";

const WORKFLOW_ACTIONS = [
  {
    id: "scrape",
    label: "Trigger Scrape",
    description: "Scrape a single source by ID",
    mutation: `mutation TriggerScrape($sourceId: UUID!) { triggerScrape(sourceId: $sourceId) { success message } }`,
    fields: [{ name: "sourceId", label: "Source ID", type: "text", placeholder: "UUID of source" }],
  },
  {
    id: "scrape_cycle",
    label: "Trigger Scrape Cycle",
    description: "Run a full scrape cycle for all due sources",
    mutation: `mutation { triggerScrapeCycle { success message } }`,
    fields: [],
  },
  {
    id: "extraction",
    label: "Trigger Extraction",
    description: "Extract data from a snapshot",
    mutation: `mutation TriggerExtraction($snapshotId: UUID!) { triggerExtraction(snapshotId: $snapshotId) { success message } }`,
    fields: [{ name: "snapshotId", label: "Snapshot ID", type: "text", placeholder: "UUID of snapshot" }],
  },
  {
    id: "translation",
    label: "Trigger Translation",
    description: "Translate a listing to a target locale",
    mutation: `mutation TriggerTranslation($listingId: UUID!, $targetLocale: String!) { triggerTranslation(listingId: $listingId, targetLocale: $targetLocale) { success message } }`,
    fields: [
      { name: "listingId", label: "Listing ID", type: "text", placeholder: "UUID of listing" },
      { name: "targetLocale", label: "Target Locale", type: "text", placeholder: "e.g. es, fr" },
    ],
  },
];

export default function WorkflowsPage() {
  const [results, setResults] = useState<Record<string, { success: boolean; message: string } | null>>({});
  const [loading, setLoading] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function handleTrigger(actionId: string, mutation: string, variables: Record<string, string>) {
    setLoading(actionId);
    setError(null);
    setResults((prev) => ({ ...prev, [actionId]: null }));

    try {
      const res = await fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ query: mutation, variables }),
      });
      const data = await res.json();
      if (data.errors) throw new Error(data.errors[0].message);

      const result = Object.values(data.data)[0] as { success: boolean; message: string };
      setResults((prev) => ({ ...prev, [actionId]: result }));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Trigger failed");
    } finally {
      setLoading(null);
    }
  }

  return (
    <div>
      <h1 className="mb-6 text-2xl font-bold">Workflows</h1>

      {error && <p className="mb-4 text-sm text-red-600">{error}</p>}

      <div className="space-y-4">
        {WORKFLOW_ACTIONS.map((action) => (
          <WorkflowCard
            key={action.id}
            action={action}
            result={results[action.id] ?? null}
            loading={loading === action.id}
            onTrigger={(vars) => handleTrigger(action.id, action.mutation, vars)}
          />
        ))}
      </div>
    </div>
  );
}

function WorkflowCard({
  action,
  result,
  loading,
  onTrigger,
}: {
  action: (typeof WORKFLOW_ACTIONS)[number];
  result: { success: boolean; message: string } | null;
  loading: boolean;
  onTrigger: (variables: Record<string, string>) => void;
}) {
  const [values, setValues] = useState<Record<string, string>>({});

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    onTrigger(values);
  }

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4">
      <h3 className="font-medium">{action.label}</h3>
      <p className="mb-3 text-sm text-gray-500">{action.description}</p>

      <form onSubmit={handleSubmit} className="flex flex-wrap items-end gap-3">
        {action.fields.map((f) => (
          <div key={f.name}>
            <label className="block text-xs font-medium text-gray-600">{f.label}</label>
            <input
              type={f.type}
              placeholder={f.placeholder}
              value={values[f.name] ?? ""}
              onChange={(e) => setValues((prev) => ({ ...prev, [f.name]: e.target.value }))}
              required
              className="mt-1 rounded border border-gray-300 px-2 py-1.5 text-sm"
            />
          </div>
        ))}
        <button
          type="submit"
          disabled={loading}
          className="rounded bg-blue-600 px-4 py-1.5 text-sm text-white hover:bg-blue-700 disabled:opacity-50"
        >
          {loading ? "Running..." : "Run"}
        </button>
      </form>

      {result && (
        <p className={`mt-2 text-sm ${result.success ? "text-green-600" : "text-red-600"}`}>
          {result.message}
        </p>
      )}
    </div>
  );
}
