"use client";

import { useState, useEffect, useCallback } from "react";

interface Observation {
  id: string;
  subjectType: string;
  subjectId: string;
  observationType: string;
  value: unknown;
  source: string;
  confidence: number;
  reviewStatus: string;
  observedAt: string;
}

export default function ObservationsPage() {
  const [observations, setObservations] = useState<Observation[]>([]);
  const [loading, setLoading] = useState(true);
  const [actionLoading, setActionLoading] = useState<string | null>(null);
  const [error, setError] = useState("");

  const fetchObservations = useCallback(async () => {
    setLoading(true);
    try {
      const res = await fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `query { pendingObservations(limit: 50) {
            id subjectType subjectId observationType value source confidence reviewStatus observedAt
          } }`,
        }),
      });
      const data = await res.json();
      if (data.errors) throw new Error(data.errors[0].message);
      setObservations(data.data.pendingObservations);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load observations");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchObservations();
  }, [fetchObservations]);

  async function handleReview(id: string, decision: "APPROVE" | "REJECT") {
    setActionLoading(id);
    setError("");
    try {
      const res = await fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `mutation ReviewObservation($id: UUID!, $decision: ReviewDecision!) {
            reviewObservation(id: $id, decision: $decision) { id reviewStatus }
          }`,
          variables: { id, decision },
        }),
      });
      const data = await res.json();
      if (data.errors) throw new Error(data.errors[0].message);
      setObservations((prev) => prev.filter((o) => o.id !== id));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Review failed");
    } finally {
      setActionLoading(null);
    }
  }

  return (
    <div>
      <h1 className="mb-6 text-2xl font-bold">Observation Review Queue</h1>

      {error && <p className="mb-4 text-sm text-red-600">{error}</p>}

      {loading ? (
        <p className="text-gray-500">Loading...</p>
      ) : observations.length === 0 ? (
        <div className="rounded-lg border border-gray-200 bg-white p-8 text-center">
          <p className="text-gray-500">No pending observations to review.</p>
          <p className="mt-2 text-sm text-gray-400">
            Observations will appear here as scrapers discover new data.
          </p>
        </div>
      ) : (
        <div className="space-y-3">
          {observations.map((obs) => (
            <div key={obs.id} className="rounded-lg border border-gray-200 bg-white p-4">
              <div className="flex items-start justify-between">
                <div className="min-w-0 flex-1">
                  <p className="text-sm font-medium">
                    {obs.observationType} on {obs.subjectType}
                  </p>
                  <p className="mt-1 text-xs text-gray-500">
                    Source: {obs.source} | Confidence: {(obs.confidence * 100).toFixed(0)}% | {new Date(obs.observedAt).toLocaleDateString()}
                  </p>
                  <pre className="mt-2 max-h-40 overflow-auto rounded bg-gray-50 p-2 text-xs">
                    {JSON.stringify(obs.value, null, 2)}
                  </pre>
                </div>
                <div className="ml-4 flex shrink-0 gap-2">
                  <button
                    onClick={() => handleReview(obs.id, "APPROVE")}
                    disabled={actionLoading === obs.id}
                    className="rounded bg-green-100 px-3 py-1.5 text-xs font-medium text-green-800 hover:bg-green-200 disabled:opacity-50"
                  >
                    Approve
                  </button>
                  <button
                    onClick={() => handleReview(obs.id, "REJECT")}
                    disabled={actionLoading === obs.id}
                    className="rounded bg-red-100 px-3 py-1.5 text-xs font-medium text-red-800 hover:bg-red-200 disabled:opacity-50"
                  >
                    Reject
                  </button>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
