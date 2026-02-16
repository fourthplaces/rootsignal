"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";

export function RunInvestigationsButton() {
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const router = useRouter();

  async function handleClick() {
    setLoading(true);
    setResult(null);
    try {
      const res = await fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `mutation { runPendingInvestigations }`,
        }),
      });
      const data = await res.json();
      if (data.errors) throw new Error(data.errors[0].message);
      const count = data.data.runPendingInvestigations;
      setResult(count > 0 ? `Triggered ${count} investigation${count > 1 ? "s" : ""}` : "No pending signals to investigate");
      router.refresh();
    } catch (err) {
      setResult(err instanceof Error ? err.message : "Failed");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="flex items-center gap-2">
      <button
        onClick={handleClick}
        disabled={loading}
        className="rounded bg-indigo-700 px-3 py-1.5 text-sm text-white hover:bg-indigo-800 disabled:opacity-50"
      >
        {loading ? "Running..." : "Run Investigations"}
      </button>
      {result && (
        <span className="text-sm text-gray-500">{result}</span>
      )}
    </div>
  );
}
