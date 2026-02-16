"use client";

import { useEffect, useState, useCallback } from "react";

interface Signal {
  id: string;
  signalType: string;
  content: string;
  about: string | null;
  entityId: string | null;
  sourceUrl: string | null;
  confidence: number;
  broadcastedAt: string | null;
  locations: {
    addressLocality: string | null;
    addressRegion: string | null;
    postalCode: string | null;
  }[];
}

interface Entity {
  id: string;
  name: string;
  entityType: string;
  description: string | null;
  website: string | null;
  locations: {
    addressLocality: string | null;
    addressRegion: string | null;
    postalCode: string | null;
  }[];
}

async function fetchDetail(entityType: string, entityId: string) {
  const isSignal = entityType === "signal";
  const query = isSignal
    ? `query($id: UUID!) {
        signal(id: $id) {
          id signalType content about entityId sourceUrl confidence broadcastedAt
          locations { addressLocality addressRegion postalCode }
        }
      }`
    : `query($id: UUID!) {
        entity(id: $id) {
          id name entityType description website
          locations { addressLocality addressRegion postalCode }
        }
      }`;

  const res = await fetch("/api/graphql", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ query, variables: { id: entityId } }),
  });
  const data = await res.json();
  if (data.errors) throw new Error(data.errors[0].message);
  return isSignal ? data.data.signal : data.data.entity;
}

export default function Sidebar({
  entityType,
  entityId,
  onClose,
}: {
  entityType: string;
  entityId: string;
  onClose: () => void;
}) {
  const [detail, setDetail] = useState<Signal | Entity | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    setLoading(true);
    setError("");
    try {
      const result = await fetchDetail(entityType, entityId);
      setDetail(result);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load details");
    } finally {
      setLoading(false);
    }
  }, [entityType, entityId]);

  useEffect(() => {
    load();
  }, [load]);

  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [onClose]);

  const isSignal = entityType === "signal";
  const detailUrl = isSignal ? `/signals/${entityId}` : `/entities/${entityId}`;
  const title = detail
    ? isSignal
      ? (detail as Signal).about || (detail as Signal).content.slice(0, 80)
      : (detail as Entity).name
    : "";
  const description = isSignal
    ? (detail as Signal | null)?.content
    : (detail as Entity | null)?.description;
  const location = detail?.locations?.[0];
  const locationStr = location
    ? [location.addressLocality, location.addressRegion, location.postalCode]
        .filter(Boolean)
        .join(", ")
    : null;

  return (
    <div className="absolute right-0 top-0 z-20 flex h-full w-80 flex-col border-l border-gray-200 bg-white shadow-lg">
      {/* Header */}
      <div className="flex items-center justify-between border-b border-gray-200 px-4 py-3">
        <span className="text-xs font-medium uppercase tracking-wider text-gray-400">
          {isSignal ? (detail as Signal | null)?.signalType || "signal" : entityType}
        </span>
        <button
          onClick={onClose}
          className="rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-600"
        >
          <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto p-4">
        {loading ? (
          <p className="text-sm text-gray-400">Loading...</p>
        ) : error ? (
          <p className="text-sm text-red-600">{error}</p>
        ) : detail ? (
          <div className="space-y-3">
            <h3 className="text-base font-semibold text-gray-900">{title}</h3>

            {isSignal && (detail as Signal).about && (
              <p className="text-sm text-gray-600">{(detail as Signal).content}</p>
            )}

            {!isSignal && description && (
              <p className="text-sm text-gray-600">{description}</p>
            )}

            {locationStr && (
              <div className="text-sm text-gray-500">
                <span className="font-medium text-gray-700">Location:</span> {locationStr}
              </div>
            )}

            {isSignal && (detail as Signal).broadcastedAt && (
              <div className="text-sm text-gray-500">
                <span className="font-medium text-gray-700">Date:</span>{" "}
                {new Date((detail as Signal).broadcastedAt!).toLocaleDateString()}
              </div>
            )}

            {isSignal && (
              <div className="text-sm text-gray-500">
                <span className="font-medium text-gray-700">Confidence:</span>{" "}
                {Math.round((detail as Signal).confidence * 100)}%
              </div>
            )}
          </div>
        ) : null}
      </div>

      {/* Footer */}
      <div className="border-t border-gray-200 p-4">
        <a
          href={detailUrl}
          target="_blank"
          rel="noopener noreferrer"
          className="block w-full rounded bg-green-700 px-4 py-2 text-center text-sm font-medium text-white hover:bg-green-800"
        >
          View {isSignal ? "Signal" : "Entity"} &rarr;
        </a>
      </div>
    </div>
  );
}
