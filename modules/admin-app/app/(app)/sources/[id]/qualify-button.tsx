"use client";

import { useState } from "react";

export function QualifyButton({ sourceId }: { sourceId: string }) {
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<{ status: string } | null>(null);

  async function handleClick() {
    setLoading(true);
    setResult(null);
    try {
      const res = await fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `mutation TriggerQualification($sourceId: UUID!) {
            triggerQualification(sourceId: $sourceId) { workflowId status }
          }`,
          variables: { sourceId },
        }),
      });
      const data = await res.json();
      if (data.errors) throw new Error(data.errors[0].message);
      setResult(data.data.triggerQualification);
    } catch {
      setResult({ status: "failed" });
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="flex items-center gap-2">
      <button
        onClick={handleClick}
        disabled={loading}
        className="rounded border border-yellow-600 px-4 py-2 text-sm text-yellow-700 hover:bg-yellow-50 disabled:opacity-50"
      >
        {loading ? "Evaluating..." : "Evaluate Source"}
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
