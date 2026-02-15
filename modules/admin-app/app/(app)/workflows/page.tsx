"use client";

import { useState, useEffect } from "react";

interface Source {
  id: string;
  name: string;
  sourceType: string;
  isActive: boolean;
}

interface TriggerResult {
  workflowId: string;
  status: string;
}

function StatusBadge({ result }: { result: TriggerResult | null }) {
  if (!result) return null;
  const ok = result.status === "triggered";
  return (
    <span
      className={`ml-3 inline-flex items-center rounded px-2 py-0.5 text-xs font-medium ${
        ok ? "bg-green-100 text-green-700" : "bg-red-100 text-red-700"
      }`}
    >
      {result.status} — {result.workflowId}
    </span>
  );
}

async function gqlMutate(query: string, variables?: Record<string, unknown>) {
  const res = await fetch("/api/graphql", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ query, variables }),
  });
  const data = await res.json();
  if (data.errors) throw new Error(data.errors[0].message);
  return data.data;
}

export default function WorkflowsPage() {
  const [sources, setSources] = useState<Source[]>([]);
  const [selectedSourceId, setSelectedSourceId] = useState("");
  const [snapshotId, setSnapshotId] = useState("");

  const [cycleResult, setCycleResult] = useState<TriggerResult | null>(null);
  const [scrapeResult, setScrapeResult] = useState<TriggerResult | null>(null);
  const [extractResult, setExtractResult] = useState<TriggerResult | null>(null);

  const [cycleLoading, setCycleLoading] = useState(false);
  const [scrapeLoading, setScrapeLoading] = useState(false);
  const [extractLoading, setExtractLoading] = useState(false);

  const [error, setError] = useState("");

  useEffect(() => {
    fetch("/api/graphql", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        query: `query { sources { id name sourceType isActive } }`,
      }),
    })
      .then((r) => r.json())
      .then((data) => {
        if (data.data?.sources) {
          setSources(data.data.sources);
          if (data.data.sources.length > 0) {
            setSelectedSourceId(data.data.sources[0].id);
          }
        }
      });
  }, []);

  async function triggerCycle() {
    setCycleLoading(true);
    setCycleResult(null);
    setError("");
    try {
      const data = await gqlMutate(
        `mutation { triggerScrapeCycle { workflowId status } }`,
      );
      setCycleResult(data.triggerScrapeCycle);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed");
    } finally {
      setCycleLoading(false);
    }
  }

  async function triggerScrape() {
    if (!selectedSourceId) return;
    setScrapeLoading(true);
    setScrapeResult(null);
    setError("");
    try {
      const data = await gqlMutate(
        `mutation TriggerScrape($sourceId: UUID!) {
          triggerScrape(sourceId: $sourceId) { workflowId status }
        }`,
        { sourceId: selectedSourceId },
      );
      setScrapeResult(data.triggerScrape);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed");
    } finally {
      setScrapeLoading(false);
    }
  }

  async function triggerExtraction() {
    if (!snapshotId.trim()) return;
    setExtractLoading(true);
    setExtractResult(null);
    setError("");
    try {
      const data = await gqlMutate(
        `mutation TriggerExtraction($snapshotId: UUID!) {
          triggerExtraction(snapshotId: $snapshotId) { workflowId status }
        }`,
        { snapshotId: snapshotId.trim() },
      );
      setExtractResult(data.triggerExtraction);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed");
    } finally {
      setExtractLoading(false);
    }
  }

  return (
    <div>
      <h1 className="mb-6 text-2xl font-bold">Workflows</h1>
      {error && <p className="mb-4 text-sm text-red-600">{error}</p>}

      <div className="space-y-6">
        {/* Scrape Cycle */}
        <section className="rounded-lg border border-gray-200 bg-white p-6">
          <h2 className="mb-2 text-lg font-semibold">Scrape Cycle</h2>
          <p className="mb-4 text-sm text-gray-500">
            Trigger a full scrape cycle for all sources that are due.
          </p>
          <div className="flex items-center">
            <button
              onClick={triggerCycle}
              disabled={cycleLoading}
              className="rounded bg-green-700 px-4 py-2 text-sm text-white hover:bg-green-800 disabled:opacity-50"
            >
              {cycleLoading ? "Running..." : "Run Scrape Cycle"}
            </button>
            <StatusBadge result={cycleResult} />
          </div>
        </section>

        {/* Scrape Source */}
        <section className="rounded-lg border border-gray-200 bg-white p-6">
          <h2 className="mb-2 text-lg font-semibold">Scrape Source</h2>
          <p className="mb-4 text-sm text-gray-500">
            Trigger a scrape for a specific source.
          </p>
          <div className="flex items-center gap-3">
            <select
              value={selectedSourceId}
              onChange={(e) => setSelectedSourceId(e.target.value)}
              className="rounded border border-gray-300 px-3 py-2 text-sm"
            >
              {sources.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name} ({s.sourceType}){!s.isActive ? " [inactive]" : ""}
                </option>
              ))}
            </select>
            <button
              onClick={triggerScrape}
              disabled={scrapeLoading || !selectedSourceId}
              className="rounded bg-green-700 px-4 py-2 text-sm text-white hover:bg-green-800 disabled:opacity-50"
            >
              {scrapeLoading ? "Running..." : "Scrape"}
            </button>
            <StatusBadge result={scrapeResult} />
          </div>
        </section>

        {/* Extract Snapshot */}
        <section className="rounded-lg border border-gray-200 bg-white p-6">
          <h2 className="mb-2 text-lg font-semibold">Extract Snapshot</h2>
          <p className="mb-4 text-sm text-gray-500">
            Trigger extraction for a specific snapshot.
          </p>
          <div className="flex items-center gap-3">
            <input
              value={snapshotId}
              onChange={(e) => setSnapshotId(e.target.value)}
              placeholder="Snapshot UUID"
              className="rounded border border-gray-300 px-3 py-2 text-sm"
            />
            <button
              onClick={triggerExtraction}
              disabled={extractLoading || !snapshotId.trim()}
              className="rounded bg-green-700 px-4 py-2 text-sm text-white hover:bg-green-800 disabled:opacity-50"
            >
              {extractLoading ? "Running..." : "Extract"}
            </button>
            <StatusBadge result={extractResult} />
          </div>
        </section>
      </div>
    </div>
  );
}
