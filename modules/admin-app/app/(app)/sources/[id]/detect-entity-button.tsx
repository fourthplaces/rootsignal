"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";

export function DetectEntityButton({ sourceId }: { sourceId: string }) {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const router = useRouter();

  async function handleClick() {
    setLoading(true);
    setError(null);
    try {
      const res = await fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `mutation DetectEntity($sourceId: UUID!) {
            detectSourceEntity(sourceId: $sourceId) { id name entityType }
          }`,
          variables: { sourceId },
        }),
      });
      const data = await res.json();
      if (data.errors) throw new Error(data.errors[0].message);
      router.refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "failed");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="flex items-center gap-2">
      <button
        onClick={handleClick}
        disabled={loading}
        className="rounded bg-blue-600 px-3 py-1 text-xs text-white hover:bg-blue-700 disabled:opacity-50"
      >
        {loading ? "Detecting..." : "Detect Entity (AI)"}
      </button>
      {error && <span className="text-xs text-red-600">{error}</span>}
    </div>
  );
}
