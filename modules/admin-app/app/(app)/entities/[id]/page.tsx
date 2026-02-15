"use client";

import { useState, useEffect } from "react";
import { useParams, useRouter } from "next/navigation";
import Link from "next/link";

interface Entity {
  id: string;
  name: string;
  entityType: string;
  description: string | null;
  website: string | null;
  telephone: string | null;
  email: string | null;
  verified: boolean;
  inLanguage: string;
  createdAt: string;
  updatedAt: string;
  tags: { id: string; kind: string; value: string }[];
  locations: { id: string; name: string | null; addressLocality: string | null; addressRegion: string | null }[];
  services: { id: string; name: string; status: string }[];
  listings: { id: string; title: string; status: string }[];
  signals: { id: string; signalType: string; content: string; about: string | null; createdAt: string }[];
  sources: { id: string; name: string; sourceType: string; url: string | null; isActive: boolean; signalCount: number }[];
}

export default function EntityDetailPage() {
  const params = useParams<{ id: string }>();
  const router = useRouter();
  const [entity, setEntity] = useState<Entity | null>(null);
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [website, setWebsite] = useState("");
  const [saving, setSaving] = useState(false);
  const [discovering, setDiscovering] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    // Fetch entity and signals in parallel
    Promise.all([
      fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `query Entity($id: UUID!) {
            entity(id: $id) {
              id name entityType description website telephone email verified
              inLanguage createdAt updatedAt
              tags { id kind value }
              locations { id name addressLocality addressRegion }
              services { id name status }
              listings { id title status }
              sources { id name sourceType url isActive signalCount }
            }
          }`,
          variables: { id: params.id },
        }),
      }).then((r) => r.json()),
      fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `query EntitySignals($entityId: ID!) {
            signals(entityId: $entityId, limit: 50) {
              nodes { id signalType content about createdAt }
            }
          }`,
          variables: { entityId: params.id },
        }),
      }).then((r) => r.json()),
    ]).then(([entityData, signalsData]) => {
      const e = entityData.data.entity;
      const signals = signalsData.data?.signals?.nodes ?? [];
      setEntity({ ...e, signals });
      setName(e.name);
      setDescription(e.description ?? "");
      setWebsite(e.website ?? "");
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
          query: `mutation UpdateEntity($id: UUID!, $input: UpdateEntityInput!) {
            updateEntity(id: $id, input: $input) { id name description website updatedAt }
          }`,
          variables: {
            id: params.id,
            input: { name, description: description || null, website: website || null },
          },
        }),
      });
      const data = await res.json();
      if (data.errors) throw new Error(data.errors[0].message);
      setEntity((prev) => (prev ? { ...prev, ...data.data.updateEntity } : prev));
      setEditing(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Save failed");
    } finally {
      setSaving(false);
    }
  }

  async function reloadEntity() {
    const res = await fetch("/api/graphql", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        query: `query Entity($id: UUID!) {
          entity(id: $id) {
            id name entityType description website telephone email verified
            inLanguage createdAt updatedAt
            tags { id kind value }
            locations { id name addressLocality addressRegion }
            services { id name status }
            listings { id title status }
            sources { id name sourceType url isActive signalCount }
          }
        }`,
        variables: { id: params.id },
      }),
    });
    const data = await res.json();
    if (data.data?.entity) {
      const e = data.data.entity;
      setEntity(e);
      setName(e.name);
      setDescription(e.description ?? "");
      setWebsite(e.website ?? "");
    }
  }

  async function handleDiscoverSocial() {
    setDiscovering(true);
    setError("");
    try {
      const res = await fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `mutation DiscoverSocial($entityId: UUID!) {
            discoverSocialLinks(entityId: $entityId) { id name sourceType url handle isActive }
          }`,
          variables: { entityId: params.id },
        }),
      });
      const data = await res.json();
      if (data.errors) throw new Error(data.errors[0].message);
      await reloadEntity();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Discovery failed");
    } finally {
      setDiscovering(false);
    }
  }

  async function handleArchive() {
    if (!confirm("Archive this entity? It must have no active listings.")) return;
    try {
      const res = await fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `mutation ArchiveEntity($id: UUID!) { archiveEntity(id: $id) }`,
          variables: { id: params.id },
        }),
      });
      const data = await res.json();
      if (data.errors) throw new Error(data.errors[0].message);
      router.push("/entities");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Archive failed");
    }
  }

  if (!entity) return <p className="text-gray-500">Loading...</p>;

  return (
    <div className="max-w-3xl">
      <div className="mb-6 flex items-center justify-between">
        <h1 className="text-2xl font-bold">{editing ? "Edit Entity" : entity.name}</h1>
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
            <label className="block text-sm font-medium text-gray-700">Name</label>
            <input value={name} onChange={(e) => setName(e.target.value)} className="mt-1 block w-full rounded border border-gray-300 px-3 py-2" />
          </div>
          <div>
            <label className="block text-sm font-medium text-gray-700">Description</label>
            <textarea value={description} onChange={(e) => setDescription(e.target.value)} rows={4} className="mt-1 block w-full rounded border border-gray-300 px-3 py-2" />
          </div>
          <div>
            <label className="block text-sm font-medium text-gray-700">Website</label>
            <input type="url" value={website} onChange={(e) => setWebsite(e.target.value)} className="mt-1 block w-full rounded border border-gray-300 px-3 py-2" />
          </div>
          <div className="flex gap-2">
            <button onClick={handleSave} disabled={saving} className="rounded bg-green-700 px-4 py-2 text-sm text-white hover:bg-green-800 disabled:opacity-50">
              {saving ? "Saving..." : "Save"}
            </button>
            <button onClick={() => setEditing(false)} className="rounded border border-gray-300 px-4 py-2 text-sm hover:bg-gray-50">Cancel</button>
          </div>
        </div>
      ) : (
        <div className="space-y-6">
          <div className="rounded-lg border border-gray-200 bg-white p-6">
            <dl className="grid grid-cols-2 gap-4">
              <div><dt className="text-sm text-gray-500">Type</dt><dd className="font-medium">{entity.entityType}</dd></div>
              <div><dt className="text-sm text-gray-500">Verified</dt><dd>{entity.verified ? "Yes" : "No"}</dd></div>
              <div><dt className="text-sm text-gray-500">Language</dt><dd>{entity.inLanguage}</dd></div>
              <div><dt className="text-sm text-gray-500">Phone</dt><dd>{entity.telephone ?? "-"}</dd></div>
              <div><dt className="text-sm text-gray-500">Email</dt><dd>{entity.email ?? "-"}</dd></div>
              <div><dt className="text-sm text-gray-500">Website</dt>
                <dd>{entity.website ? <a href={entity.website} target="_blank" rel="noopener noreferrer" className="text-blue-600 hover:underline">{entity.website}</a> : "-"}</dd>
              </div>
            </dl>
            {entity.description && (
              <div className="mt-4 border-t border-gray-200 pt-4">
                <h3 className="mb-1 text-sm font-medium text-gray-500">Description</h3>
                <p className="whitespace-pre-wrap text-sm">{entity.description}</p>
              </div>
            )}
          </div>

          {entity.services.length > 0 && (
            <div>
              <h3 className="mb-2 text-sm font-medium text-gray-500">Services</h3>
              <ul className="space-y-1">
                {entity.services.map((s) => (
                  <li key={s.id} className="flex items-center justify-between rounded border border-gray-200 bg-white px-3 py-2 text-sm">
                    <span>{s.name}</span>
                    <span className="rounded-full bg-gray-100 px-2 py-0.5 text-xs">{s.status}</span>
                  </li>
                ))}
              </ul>
            </div>
          )}

          {entity.listings.length > 0 && (
            <div>
              <h3 className="mb-2 text-sm font-medium text-gray-500">Listings</h3>
              <ul className="space-y-1">
                {entity.listings.map((l) => (
                  <li key={l.id} className="rounded border border-gray-200 bg-white px-3 py-2 text-sm">
                    <Link href={`/listings/${l.id}`} className="text-green-700 hover:underline">{l.title}</Link>
                    <span className="ml-2 text-xs text-gray-400">{l.status}</span>
                  </li>
                ))}
              </ul>
            </div>
          )}

          {entity.signals.length > 0 && (
            <div>
              <h3 className="mb-2 text-sm font-medium text-gray-500">Signals ({entity.signals.length})</h3>
              <ul className="space-y-1">
                {entity.signals.map((s) => (
                  <li key={s.id} className="rounded border border-gray-200 bg-white px-3 py-2 text-sm">
                    <div className="flex items-center gap-2">
                      <span className={`inline-block rounded-full px-2 py-0.5 text-xs font-medium ${
                        s.signalType === "ask" ? "bg-orange-100 text-orange-800" :
                        s.signalType === "give" ? "bg-green-100 text-green-800" :
                        s.signalType === "event" ? "bg-blue-100 text-blue-800" :
                        "bg-gray-100 text-gray-800"
                      }`}>{s.signalType}</span>
                      <Link href={`/signals/${s.id}`} className="text-green-700 hover:underline">
                        {s.content.length > 100 ? s.content.slice(0, 100) + "..." : s.content}
                      </Link>
                    </div>
                    {s.about && <span className="ml-14 text-xs text-gray-400">{s.about}</span>}
                  </li>
                ))}
              </ul>
            </div>
          )}

          <div>
            <div className="mb-2 flex items-center justify-between">
              <h3 className="text-sm font-medium text-gray-500">Sources</h3>
              <button
                onClick={handleDiscoverSocial}
                disabled={discovering}
                className="rounded border border-green-300 px-3 py-1 text-xs text-green-700 hover:bg-green-50 disabled:opacity-50"
              >
                {discovering ? "Discovering..." : "Discover Social"}
              </button>
            </div>
            {entity.sources.length > 0 && (
              <ul className="space-y-1">
                {entity.sources.map((s) => (
                  <li key={s.id} className="flex items-center justify-between rounded border border-gray-200 bg-white px-3 py-2 text-sm">
                    <div>
                      <Link href={`/sources/${s.id}`} className="text-green-700 hover:underline">{s.name}</Link>
                      {s.url && (
                        <a href={s.url} target="_blank" rel="noopener noreferrer" className="ml-2 text-xs text-blue-500 hover:underline">{s.url}</a>
                      )}
                    </div>
                    <div className="flex items-center gap-2">
                      <span className="rounded-full bg-gray-100 px-2 py-0.5 text-xs">{s.sourceType}</span>
                      <span className={`rounded-full px-2 py-0.5 text-xs ${s.isActive ? "bg-green-100 text-green-700" : "bg-gray-100 text-gray-500"}`}>
                        {s.isActive ? "active" : "inactive"}
                      </span>
                      {s.signalCount > 0 && (
                        <span className="rounded-full bg-green-100 px-2 py-0.5 text-xs text-green-700">
                          {s.signalCount} signal{s.signalCount === 1 ? "" : "s"}
                        </span>
                      )}
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </div>

          <p className="text-xs text-gray-400">
            Created {new Date(entity.createdAt).toLocaleString()} | Updated {new Date(entity.updatedAt).toLocaleString()}
          </p>
        </div>
      )}
    </div>
  );
}
