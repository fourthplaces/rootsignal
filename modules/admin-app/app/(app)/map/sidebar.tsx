"use client";

import { useEffect, useState, useCallback } from "react";

interface Listing {
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

async function fetchDetail(entityType: string, entityId: string) {
  const isListing = entityType === "listing";
  const query = isListing
    ? `query($id: UUID!) {
        listing(id: $id) {
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
  return isListing ? data.data.listing : data.data.entity;
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
  const [detail, setDetail] = useState<Listing | Entity | null>(null);
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

  const isListing = entityType === "listing";
  const detailUrl = isListing ? `/listings/${entityId}` : `/entities/${entityId}`;
  const title = detail
    ? isListing
      ? (detail as Listing).title
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

            {isListing && (detail as Listing).entityName && (
              <div className="text-sm text-gray-500">
                <span className="font-medium text-gray-700">Organization:</span>{" "}
                {(detail as Listing).entityName}
              </div>
            )}

            {isListing && (detail as Listing).signalDomain && (
              <div className="text-sm text-gray-500">
                <span className="font-medium text-gray-700">Domain:</span>{" "}
                {(detail as Listing).signalDomain}
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
          View {isListing ? "Listing" : "Entity"} &rarr;
        </a>
      </div>
    </div>
  );
}
