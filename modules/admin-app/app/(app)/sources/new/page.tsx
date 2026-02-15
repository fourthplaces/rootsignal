"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";

const SOURCE_TYPES = [
  { value: "website", label: "Website" },
  { value: "web_search", label: "Web Search" },
  { value: "instagram", label: "Instagram" },
  { value: "facebook", label: "Facebook" },
  { value: "x", label: "X (Twitter)" },
  { value: "tiktok", label: "TikTok" },
  { value: "gofundme", label: "GoFundMe" },
] as const;

type SourceType = (typeof SOURCE_TYPES)[number]["value"];

export default function NewSourcePage() {
  const router = useRouter();
  const [sourceType, setSourceType] = useState<SourceType>("website");
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [handle, setHandle] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [maxResults, setMaxResults] = useState("10");
  const [cadenceHours, setCadenceHours] = useState("24");
  const [entityId, setEntityId] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setSaving(true);
    setError("");

    const config: Record<string, unknown> = {};
    if (sourceType === "web_search") {
      config.search_query = searchQuery;
      config.max_results = parseInt(maxResults, 10);
    }

    const input: Record<string, unknown> = {
      name,
      sourceType,
      cadenceHours: parseInt(cadenceHours, 10),
      config: Object.keys(config).length > 0 ? config : null,
    };

    if (sourceType === "website") input.url = url || null;
    if (["instagram", "facebook", "x", "tiktok"].includes(sourceType)) input.handle = handle || null;
    if (entityId) input.entityId = entityId;

    try {
      const res = await fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `mutation CreateSource($input: CreateSourceInput!) {
            createSource(input: $input) { id name }
          }`,
          variables: { input },
        }),
      });
      const data = await res.json();
      if (data.errors) throw new Error(data.errors[0].message);
      router.push("/sources");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create source");
    } finally {
      setSaving(false);
    }
  }

  const isSocial = ["instagram", "facebook", "x", "tiktok"].includes(sourceType);

  return (
    <div className="max-w-xl">
      <h1 className="mb-6 text-2xl font-bold">New Source</h1>
      {error && <p className="mb-4 text-sm text-red-600">{error}</p>}

      <form onSubmit={handleSubmit} className="space-y-4 rounded-lg border border-gray-200 bg-white p-6">
        <div>
          <label className="block text-sm font-medium text-gray-700">Source Type</label>
          <select
            value={sourceType}
            onChange={(e) => setSourceType(e.target.value as SourceType)}
            className="mt-1 block w-full rounded border border-gray-300 px-3 py-2"
          >
            {SOURCE_TYPES.map((t) => (
              <option key={t.value} value={t.value}>{t.label}</option>
            ))}
          </select>
        </div>

        <div>
          <label className="block text-sm font-medium text-gray-700">Name</label>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
            className="mt-1 block w-full rounded border border-gray-300 px-3 py-2"
          />
        </div>

        {sourceType === "website" && (
          <div>
            <label className="block text-sm font-medium text-gray-700">URL</label>
            <input
              type="url"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="https://example.com"
              className="mt-1 block w-full rounded border border-gray-300 px-3 py-2"
            />
          </div>
        )}

        {sourceType === "web_search" && (
          <>
            <div>
              <label className="block text-sm font-medium text-gray-700">Search Query</label>
              <input
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                required
                placeholder="e.g. third places community spaces Minneapolis"
                className="mt-1 block w-full rounded border border-gray-300 px-3 py-2"
              />
            </div>
            <div>
              <label className="block text-sm font-medium text-gray-700">Max Results</label>
              <input
                type="number"
                value={maxResults}
                onChange={(e) => setMaxResults(e.target.value)}
                min={1}
                max={100}
                className="mt-1 block w-full rounded border border-gray-300 px-3 py-2"
              />
            </div>
          </>
        )}

        {isSocial && (
          <div>
            <label className="block text-sm font-medium text-gray-700">Handle</label>
            <input
              value={handle}
              onChange={(e) => setHandle(e.target.value)}
              placeholder="@username"
              className="mt-1 block w-full rounded border border-gray-300 px-3 py-2"
            />
          </div>
        )}

        <div>
          <label className="block text-sm font-medium text-gray-700">Cadence (hours)</label>
          <input
            type="number"
            value={cadenceHours}
            onChange={(e) => setCadenceHours(e.target.value)}
            min={1}
            className="mt-1 block w-full rounded border border-gray-300 px-3 py-2"
          />
        </div>

        {sourceType !== "web_search" && (
          <div>
            <label className="block text-sm font-medium text-gray-700">Entity ID (optional)</label>
            <input
              value={entityId}
              onChange={(e) => setEntityId(e.target.value)}
              placeholder="UUID"
              className="mt-1 block w-full rounded border border-gray-300 px-3 py-2"
            />
          </div>
        )}

        <div className="flex gap-2">
          <button
            type="submit"
            disabled={saving}
            className="rounded bg-green-700 px-4 py-2 text-sm text-white hover:bg-green-800 disabled:opacity-50"
          >
            {saving ? "Creating..." : "Create Source"}
          </button>
          <button
            type="button"
            onClick={() => router.back()}
            className="rounded border border-gray-300 px-4 py-2 text-sm hover:bg-gray-50"
          >
            Cancel
          </button>
        </div>
      </form>
    </div>
  );
}
