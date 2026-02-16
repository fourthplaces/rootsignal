"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";

const STATUS_COLORS: Record<string, string> = {
  pending: "bg-yellow-100 text-yellow-800",
  in_progress: "bg-blue-100 text-blue-800",
  completed: "bg-green-100 text-green-800",
  linked: "bg-purple-100 text-purple-800",
};

const STATUS_LABELS: Record<string, string> = {
  pending: "Pending",
  in_progress: "Investigating",
  completed: "Complete",
  linked: "Linked",
};

export function InvestigateButton({
  signalId,
  investigationStatus,
}: {
  signalId: string;
  investigationStatus: string | null;
}) {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const router = useRouter();

  const canTrigger =
    !investigationStatus ||
    investigationStatus === "completed" ||
    investigationStatus === "linked";

  async function handleClick() {
    setLoading(true);
    setError(null);
    try {
      const res = await fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `mutation($signalId: UUID!) { triggerInvestigation(signalId: $signalId) { workflowId status } }`,
          variables: { signalId },
        }),
      });
      const data = await res.json();
      if (data.errors) throw new Error(data.errors[0].message);
      router.refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to trigger");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="flex items-center gap-3">
      {investigationStatus && (
        <span
          className={`inline-block rounded-full px-2 py-0.5 text-xs font-medium ${
            STATUS_COLORS[investigationStatus] || "bg-gray-100"
          }`}
        >
          {STATUS_LABELS[investigationStatus] || investigationStatus}
        </span>
      )}
      {canTrigger && (
        <button
          onClick={handleClick}
          disabled={loading}
          className="rounded bg-indigo-700 px-3 py-1.5 text-sm text-white hover:bg-indigo-800 disabled:opacity-50"
        >
          {loading ? "Triggering..." : "Investigate"}
        </button>
      )}
      {investigationStatus === "in_progress" && (
        <span className="inline-flex items-center gap-1.5 text-sm text-blue-600">
          <span className="inline-block h-2 w-2 animate-pulse rounded-full bg-blue-500" />
          In progress
        </span>
      )}
      {error && <span className="text-sm text-red-600">{error}</span>}
    </div>
  );
}
