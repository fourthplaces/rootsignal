"use client";

import { useEffect, useState, useCallback } from "react";

interface Signal {
  id: string;
  title: string;
  description: string | null;
  status: string;
  signalDomain: string | null;
  category: string | null;
  url: string | null;
  entityName: string | null;
  locations: {
    addressLocality: string | null;
    addressRegion: string | null;
    postalCode: string | null;
  }[];
  tags: { kind: string; value: string }[];
}

interface Entity {
  id: string;
  name: string;
  description: string | null;
  entityType: string;
  status: string;
  url: string | null;
  locations: {
    addressLocality: string | null;
    addressRegion: string | null;
    postalCode: string | null;
  }[];
  tags: { kind: string; value: string }[];
}

interface ClusterDetail {
  id: string;
  clusterType: string;
  representativeContent: string;
  representativeAbout: string | null;
  representativeSignalType: string;
  representativeConfidence: number;
  representativeBroadcastedAt: string | null;
  signals: {
    id: string;
    signalType: string;
    content: string;
    confidence: number;
    broadcastedAt: string | null;
  }[];
  entities: {
    id: string;
    name: string;
    entityType: string;
  }[];
}

const SIGNAL_TYPE_COLORS: Record<string, string> = {
  ask: "#ef4444",
  give: "#22c55e",
  event: "#a855f7",
  informative: "#3b82f6",
};

async function fetchDetail(entityType: string, entityId: string) {
  const isSignal = entityType === "signal";
  const query = isSignal
    ? `query($id: UUID!) {
        signal(id: $id) {
          id title description status signalDomain category url entityName
          locations { addressLocality addressRegion postalCode }
          tags { kind value }
        }
      }`
    : `query($id: UUID!) {
        entity(id: $id) {
          id name description entityType status url
          locations { addressLocality addressRegion postalCode }
          tags { kind value }
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

async function fetchClusterDetail(clusterId: string): Promise<ClusterDetail> {
  const query = `query($id: UUID!) {
    signalCluster(id: $id) {
      id clusterType
      representativeContent representativeAbout
      representativeSignalType representativeConfidence
      representativeBroadcastedAt
      signals { id signalType content confidence broadcastedAt }
      entities { id name entityType }
    }
  }`;

  const res = await fetch("/api/graphql", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ query, variables: { id: clusterId } }),
  });
  const data = await res.json();
  if (data.errors) throw new Error(data.errors[0].message);
  return data.data.signalCluster;
}

function ClusterSidebar({ clusterId, onClose }: { clusterId: string; onClose: () => void }) {
  const [detail, setDetail] = useState<ClusterDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [showAll, setShowAll] = useState(false);

  useEffect(() => {
    setLoading(true);
    setError("");
    fetchClusterDetail(clusterId)
      .then(setDetail)
      .catch((err) => setError(err instanceof Error ? err.message : "Failed to load cluster"))
      .finally(() => setLoading(false));
  }, [clusterId]);

  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [onClose]);

  const signals = detail?.signals ?? [];
  const displayedSignals = showAll ? signals : signals.slice(0, 10);

  const signalCounts = signals.reduce(
    (acc, s) => { acc[s.signalType] = (acc[s.signalType] || 0) + 1; return acc; },
    {} as Record<string, number>,
  );

  return (
    <div className="absolute right-0 top-0 z-20 flex h-full w-80 flex-col border-l border-gray-200 bg-white shadow-lg">
      <div className="flex items-center justify-between border-b border-gray-200 px-4 py-3">
        <span className="text-xs font-medium uppercase tracking-wider text-gray-400">
          cluster
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

      <div className="flex-1 overflow-y-auto p-4">
        {loading ? (
          <p className="text-sm text-gray-400">Loading...</p>
        ) : error ? (
          <p className="text-sm text-red-600">{error}</p>
        ) : detail ? (
          <div className="space-y-4">
            {/* Theme / About */}
            <div>
              {detail.representativeAbout && (
                <h3 className="text-base font-semibold text-gray-900">{detail.representativeAbout}</h3>
              )}
              <p className="mt-1 text-sm text-gray-600">{detail.representativeContent}</p>
            </div>

            {/* Signal type counts */}
            <div className="flex flex-wrap gap-1.5">
              {Object.entries(signalCounts).map(([type, count]) => (
                <span
                  key={type}
                  className="inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-xs font-medium text-white"
                  style={{ backgroundColor: SIGNAL_TYPE_COLORS[type] || "#6b7280" }}
                >
                  {type} {count}
                </span>
              ))}
              <span className="rounded-full bg-gray-100 px-2.5 py-0.5 text-xs font-medium text-gray-600">
                {signals.length} signals
              </span>
            </div>

            {/* Linked entities */}
            {detail.entities.length > 0 && (
              <div>
                <h4 className="mb-1 text-xs font-medium uppercase tracking-wider text-gray-400">Entities</h4>
                <div className="space-y-1">
                  {detail.entities.map((e) => (
                    <a
                      key={e.id}
                      href={`/entities/${e.id}`}
                      className="block text-sm text-green-700 hover:text-green-900 hover:underline"
                    >
                      {e.name}
                      <span className="ml-1 text-xs text-gray-400">{e.entityType.replace("_", " ")}</span>
                    </a>
                  ))}
                </div>
              </div>
            )}

            {/* Member signals */}
            <div>
              <h4 className="mb-2 text-xs font-medium uppercase tracking-wider text-gray-400">Signals</h4>
              <div className="space-y-2">
                {displayedSignals.map((s) => (
                  <a
                    key={s.id}
                    href={`/signals/${s.id}`}
                    className="block rounded border border-gray-100 p-2 hover:bg-gray-50"
                  >
                    <div className="flex items-center gap-1.5">
                      <span
                        className="inline-block h-2 w-2 rounded-full"
                        style={{ backgroundColor: SIGNAL_TYPE_COLORS[s.signalType] || "#6b7280" }}
                      />
                      <span className="text-xs font-medium text-gray-500">{s.signalType}</span>
                      {s.broadcastedAt && (
                        <span className="ml-auto text-xs text-gray-400">
                          {new Date(s.broadcastedAt).toLocaleDateString()}
                        </span>
                      )}
                    </div>
                    <p className="mt-1 line-clamp-2 text-sm text-gray-700">{s.content}</p>
                  </a>
                ))}
              </div>
              {!showAll && signals.length > 10 && (
                <button
                  onClick={() => setShowAll(true)}
                  className="mt-2 text-sm text-green-700 hover:text-green-900 hover:underline"
                >
                  Show all {signals.length} signals
                </button>
              )}
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
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

  // Delegate to ClusterSidebar for cluster entity type
  if (entityType === "cluster") {
    return <ClusterSidebar clusterId={entityId} onClose={onClose} />;
  }

  const isSignal = entityType === "signal";
  const detailUrl = isSignal ? `/signals/${entityId}` : `/entities/${entityId}`;
  const title = detail
    ? isSignal
      ? (detail as Signal).title
      : (detail as Entity).name
    : "";
  const description = detail?.description;
  const status = detail?.status;
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
          {entityType}
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

            {status && (
              <span
                className={`inline-block rounded px-2 py-0.5 text-xs font-medium ${
                  status === "active"
                    ? "bg-green-100 text-green-800"
                    : status === "archived"
                      ? "bg-gray-100 text-gray-600"
                      : "bg-yellow-100 text-yellow-800"
                }`}
              >
                {status}
              </span>
            )}

            {description && (
              <p className="text-sm text-gray-600">{description}</p>
            )}

            {locationStr && (
              <div className="text-sm text-gray-500">
                <span className="font-medium text-gray-700">Location:</span> {locationStr}
              </div>
            )}

            {isSignal && (detail as Signal).entityName && (
              <div className="text-sm text-gray-500">
                <span className="font-medium text-gray-700">Organization:</span>{" "}
                {(detail as Signal).entityName}
              </div>
            )}

            {isSignal && (detail as Signal).signalDomain && (
              <div className="text-sm text-gray-500">
                <span className="font-medium text-gray-700">Domain:</span>{" "}
                {(detail as Signal).signalDomain}
              </div>
            )}

            {detail.tags.length > 0 && (
              <div className="flex flex-wrap gap-1">
                {detail.tags.map((t, i) => (
                  <span
                    key={i}
                    className="rounded bg-gray-100 px-2 py-0.5 text-xs text-gray-600"
                  >
                    {t.value}
                  </span>
                ))}
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
