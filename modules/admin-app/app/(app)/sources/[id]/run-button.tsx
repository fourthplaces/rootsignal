"use client";

import { useState, useRef, useEffect } from "react";

export function RunButton({ sourceId }: { sourceId: string }) {
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<string | null>(null);

  async function handleClick() {
    setLoading(true);
    setResult(null);
    try {
      const res = await fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `mutation Run($sourceId: UUID!) { triggerScrape(sourceId: $sourceId) { status } }`,
          variables: { sourceId },
        }),
      });
      const data = await res.json();
      if (data.errors) throw new Error(data.errors[0].message);
      setResult(data.data.triggerScrape.status);
    } catch (err) {
      setResult(err instanceof Error ? err.message : "failed");
    } finally {
      setLoading(false);
    }
  }

  return (
    <button
      onClick={handleClick}
      disabled={loading}
      className="rounded bg-green-700 px-4 py-2 text-sm text-white hover:bg-green-800 disabled:opacity-50"
    >
      {loading ? "Running..." : result ?? "Run"}
    </button>
  );
}

export function SourceMoreMenu({ sourceId }: { sourceId: string }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  return (
    <div className="relative" ref={ref}>
      <button
        onClick={() => setOpen(!open)}
        className="rounded border border-gray-300 px-2 py-2 text-sm text-gray-500 hover:bg-gray-50"
      >
        ...
      </button>
      {open && (
        <div className="absolute right-0 z-10 mt-1 w-48 rounded-md border border-gray-200 bg-white py-1 shadow-lg">
          <MutationItem
            label="Qualify"
            loadingLabel="Qualifying..."
            mutation={`mutation($id: UUID!) { triggerQualification(sourceId: $id) { status } }`}
            variables={{ id: sourceId }}
            onDone={() => setOpen(false)}
          />
        </div>
      )}
    </div>
  );
}

function MutationItem({
  label,
  loadingLabel,
  mutation,
  variables,
  onDone,
}: {
  label: string;
  loadingLabel: string;
  mutation: string;
  variables: Record<string, string>;
  onDone: () => void;
}) {
  const [loading, setLoading] = useState(false);

  async function handleClick() {
    setLoading(true);
    try {
      const res = await fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ query: mutation, variables }),
      });
      const data = await res.json();
      if (data.errors) throw new Error(data.errors[0].message);
    } catch (err) {
      console.error(`${label} failed:`, err);
    } finally {
      setLoading(false);
      onDone();
    }
  }

  return (
    <button
      onClick={handleClick}
      disabled={loading}
      className="block w-full px-4 py-2 text-left text-sm text-gray-700 hover:bg-gray-100 disabled:opacity-50"
    >
      {loading ? loadingLabel : label}
    </button>
  );
}
