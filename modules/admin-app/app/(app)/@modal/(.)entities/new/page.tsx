"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";

const ENTITY_TYPES = [
  { value: "nonprofit", label: "Nonprofit" },
  { value: "government", label: "Government" },
  { value: "business", label: "Business" },
  { value: "faith", label: "Faith" },
  { value: "other", label: "Other" },
] as const;

export default function NewEntityModal() {
  const router = useRouter();
  const [name, setName] = useState("");
  const [entityType, setEntityType] = useState<string>("nonprofit");
  const [description, setDescription] = useState("");
  const [website, setWebsite] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  function close() {
    router.back();
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setSaving(true);
    setError("");

    try {
      const res = await fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `mutation CreateEntity($input: CreateEntityInput!) {
            createEntity(input: $input) { id name }
          }`,
          variables: {
            input: {
              name,
              entityType,
              description: description || null,
              website: website || null,
            },
          },
        }),
      });
      const data = await res.json();
      if (data.errors) throw new Error(data.errors[0].message);
      router.push(`/entities/${data.data.createEntity.id}`);
      router.refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create entity");
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="fixed inset-0 bg-black/50" onClick={close} />
      <div className="relative w-full max-w-xl rounded-lg border border-gray-200 bg-white p-6 shadow-xl">
        <button
          onClick={close}
          className="absolute right-4 top-4 text-gray-400 hover:text-gray-600"
        >
          &times;
        </button>

        <h2 className="mb-6 text-2xl font-bold">New Entity</h2>
        {error && <p className="mb-4 text-sm text-red-600">{error}</p>}

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="block text-sm font-medium text-gray-700">Name</label>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              required
              className="mt-1 block w-full rounded border border-gray-300 px-3 py-2"
            />
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-700">Entity Type</label>
            <select
              value={entityType}
              onChange={(e) => setEntityType(e.target.value)}
              className="mt-1 block w-full rounded border border-gray-300 px-3 py-2"
            >
              {ENTITY_TYPES.map((t) => (
                <option key={t.value} value={t.value}>{t.label}</option>
              ))}
            </select>
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-700">Description</label>
            <textarea
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              rows={3}
              className="mt-1 block w-full rounded border border-gray-300 px-3 py-2"
            />
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-700">Website</label>
            <input
              type="url"
              value={website}
              onChange={(e) => setWebsite(e.target.value)}
              placeholder="https://example.org"
              className="mt-1 block w-full rounded border border-gray-300 px-3 py-2"
            />
          </div>

          <div className="flex gap-2">
            <button
              type="submit"
              disabled={saving}
              className="rounded bg-green-700 px-4 py-2 text-sm text-white hover:bg-green-800 disabled:opacity-50"
            >
              {saving ? "Creating..." : "Create Entity"}
            </button>
            <button
              type="button"
              onClick={close}
              className="rounded border border-gray-300 px-4 py-2 text-sm hover:bg-gray-50"
            >
              Cancel
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
