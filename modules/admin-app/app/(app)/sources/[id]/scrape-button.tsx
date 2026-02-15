"use client";

import { useState } from "react";

export function ScrapeButton({ sourceId }: { sourceId: string }) {
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<{ status: string; workflowId: string } | null>(null);

  async function handleClick() {
    setLoading(true);
    setResult(null);
    try {
      const res = await fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `mutation TriggerScrape($sourceId: UUID!) {
            triggerScrape(sourceId: $sourceId) { workflowId status }
          }`,
          variables: { sourceId },
        }),
      });
      const data = await res.json();
      if (data.errors) throw new Error(data.errors[0].message);
      setResult(data.data.triggerScrape);
    } catch {
      setResult({ status: "failed", workflowId: "" });
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="flex items-center gap-3">
      <button
        onClick={handleClick}
        disabled={loading}
        className="rounded bg-green-700 px-4 py-2 text-sm text-white hover:bg-green-800 disabled:opacity-50"
      >
        {loading ? "Running..." : "Run Scrape"}
      </button>
      {result && (
        <span
          className={`text-xs font-medium ${
            result.status === "triggered" ? "text-green-600" : "text-red-600"
          }`}
        >
          {result.status}
        </span>
      )}
    </div>
  );
}
