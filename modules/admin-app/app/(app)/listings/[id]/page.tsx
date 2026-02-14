"use client";

import { useState, useEffect } from "react";
import { useParams, useRouter } from "next/navigation";

interface Listing {
  id: string;
  title: string;
  description: string | null;
  status: string;
  entityId: string | null;
  serviceId: string | null;
  sourceUrl: string | null;
  locationText: string | null;
  sourceLocale: string;
  freshnessScore: number;
  relevanceScore: number | null;
  createdAt: string;
  updatedAt: string;
  entity: { id: string; name: string } | null;
  service: { id: string; name: string } | null;
  tags: { id: string; kind: string; value: string }[];
  locations: { id: string; name: string | null; city: string | null; state: string | null }[];
}

export default function ListingDetailPage() {
  const params = useParams<{ id: string }>();
  const router = useRouter();
  const [listing, setListing] = useState<Listing | null>(null);
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [status, setStatus] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    fetch("/api/graphql", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        query: `query Listing($id: UUID!) {
          listing(id: $id) {
            id title description status entityId serviceId sourceUrl locationText
            sourceLocale freshnessScore relevanceScore createdAt updatedAt
            entity { id name } service { id name }
            tags { id kind value }
            locations { id name city state }
          }
        }`,
        variables: { id: params.id },
      }),
    })
      .then((r) => r.json())
      .then((data) => {
        const l = data.data.listing;
        setListing(l);
        setTitle(l.title);
        setDescription(l.description ?? "");
        setStatus(l.status);
      });
  }, [params.id]);

  async function handleSave() {
    setSaving(true);
    setError("");
    try {
      const res = await fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `mutation UpdateListing($id: UUID!, $input: UpdateListingInput!) {
            updateListing(id: $id, input: $input) { id title description status updatedAt }
          }`,
          variables: { id: params.id, input: { title, description: description || null, status } },
        }),
      });
      const data = await res.json();
      if (data.errors) throw new Error(data.errors[0].message);
      setListing((prev) => (prev ? { ...prev, ...data.data.updateListing } : prev));
      setEditing(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Save failed");
    } finally {
      setSaving(false);
    }
  }

  async function handleArchive() {
    if (!confirm("Archive this listing?")) return;
    try {
      await fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `mutation ArchiveListing($id: UUID!) { archiveListing(id: $id) }`,
          variables: { id: params.id },
        }),
      });
      router.push("/listings");
    } catch {
      setError("Archive failed");
    }
  }

  if (!listing) return <p className="text-gray-500">Loading...</p>;

  return (
    <div className="max-w-3xl">
      <div className="mb-6 flex items-center justify-between">
        <h1 className="text-2xl font-bold">
          {editing ? "Edit Listing" : listing.title}
        </h1>
        <div className="flex gap-2">
          {!editing && (
            <>
              <button
                onClick={() => setEditing(true)}
                className="rounded border border-gray-300 px-3 py-1.5 text-sm hover:bg-gray-50"
              >
                Edit
              </button>
              <button
                onClick={handleArchive}
                className="rounded border border-red-300 px-3 py-1.5 text-sm text-red-600 hover:bg-red-50"
              >
                Archive
              </button>
            </>
          )}
        </div>
      </div>

      {error && <p className="mb-4 text-sm text-red-600">{error}</p>}

      {editing ? (
        <div className="space-y-4 rounded-lg border border-gray-200 bg-white p-6">
          <div>
            <label className="block text-sm font-medium text-gray-700">Title</label>
            <input
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              className="mt-1 block w-full rounded border border-gray-300 px-3 py-2"
            />
          </div>
          <div>
            <label className="block text-sm font-medium text-gray-700">Description</label>
            <textarea
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              rows={4}
              className="mt-1 block w-full rounded border border-gray-300 px-3 py-2"
            />
          </div>
          <div>
            <label className="block text-sm font-medium text-gray-700">Status</label>
            <select
              value={status}
              onChange={(e) => setStatus(e.target.value)}
              className="mt-1 block w-full rounded border border-gray-300 px-3 py-2"
            >
              <option value="active">Active</option>
              <option value="draft">Draft</option>
              <option value="archived">Archived</option>
            </select>
          </div>
          <div className="flex gap-2">
            <button
              onClick={handleSave}
              disabled={saving}
              className="rounded bg-green-700 px-4 py-2 text-sm text-white hover:bg-green-800 disabled:opacity-50"
            >
              {saving ? "Saving..." : "Save"}
            </button>
            <button
              onClick={() => setEditing(false)}
              className="rounded border border-gray-300 px-4 py-2 text-sm hover:bg-gray-50"
            >
              Cancel
            </button>
          </div>
        </div>
      ) : (
        <div className="space-y-6">
          <div className="rounded-lg border border-gray-200 bg-white p-6">
            <dl className="grid grid-cols-2 gap-4">
              <div>
                <dt className="text-sm text-gray-500">Status</dt>
                <dd className="font-medium">{listing.status}</dd>
              </div>
              <div>
                <dt className="text-sm text-gray-500">Locale</dt>
                <dd>{listing.sourceLocale}</dd>
              </div>
              <div>
                <dt className="text-sm text-gray-500">Entity</dt>
                <dd>{listing.entity?.name ?? "-"}</dd>
              </div>
              <div>
                <dt className="text-sm text-gray-500">Service</dt>
                <dd>{listing.service?.name ?? "-"}</dd>
              </div>
              <div>
                <dt className="text-sm text-gray-500">Freshness</dt>
                <dd>{listing.freshnessScore.toFixed(2)}</dd>
              </div>
              <div>
                <dt className="text-sm text-gray-500">Source URL</dt>
                <dd className="truncate">
                  {listing.sourceUrl ? (
                    <a href={listing.sourceUrl} target="_blank" rel="noopener noreferrer" className="text-blue-600 hover:underline">
                      {listing.sourceUrl}
                    </a>
                  ) : "-"}
                </dd>
              </div>
            </dl>
            {listing.description && (
              <div className="mt-4 border-t border-gray-200 pt-4">
                <h3 className="mb-1 text-sm font-medium text-gray-500">Description</h3>
                <p className="whitespace-pre-wrap text-sm">{listing.description}</p>
              </div>
            )}
          </div>

          {listing.tags.length > 0 && (
            <div>
              <h3 className="mb-2 text-sm font-medium text-gray-500">Tags</h3>
              <div className="flex flex-wrap gap-1">
                {listing.tags.map((t) => (
                  <span key={t.id} className="rounded bg-gray-100 px-2 py-0.5 text-xs">
                    {t.kind}: {t.value}
                  </span>
                ))}
              </div>
            </div>
          )}

          {listing.locations.length > 0 && (
            <div>
              <h3 className="mb-2 text-sm font-medium text-gray-500">Locations</h3>
              <ul className="space-y-1 text-sm">
                {listing.locations.map((loc) => (
                  <li key={loc.id}>
                    {[loc.name, loc.city, loc.state].filter(Boolean).join(", ")}
                  </li>
                ))}
              </ul>
            </div>
          )}

          <p className="text-xs text-gray-400">
            Created {new Date(listing.createdAt).toLocaleString()} | Updated{" "}
            {new Date(listing.updatedAt).toLocaleString()}
          </p>
        </div>
      )}
    </div>
  );
}
