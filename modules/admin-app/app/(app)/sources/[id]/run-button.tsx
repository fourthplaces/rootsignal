"use client";

import { useState, useRef, useEffect, useCallback } from "react";
import { useRouter } from "next/navigation";

interface ActiveWorkflow {
  workflowType: string;
  sourceId: string;
  status: string;
  stage: string | null;
  createdAt: string | null;
}

const WORKFLOW_LABELS: Record<string, string> = {
  ScrapeWorkflow: "Scrape",
};

const STAGE_LABELS: Record<string, string> = {
  scraping: "Scraping pages",
  extracting: "Extracting listings",
  discovering: "Discovering sources",
  completed: "Completed",
};

async function fetchActiveWorkflows(sourceId: string): Promise<ActiveWorkflow[]> {
  try {
    const res = await fetch("/api/graphql", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        query: `query($sourceId: UUID!) {
          activeWorkflows(sourceId: $sourceId) {
            workflowType sourceId status stage createdAt
          }
        }`,
        variables: { sourceId },
      }),
    });
    const data = await res.json();
    return data.data?.activeWorkflows ?? [];
  } catch {
    return [];
  }
}

export function WorkflowStatus({
  sourceId,
  initialWorkflows = [],
}: {
  sourceId: string;
  initialWorkflows?: ActiveWorkflow[];
}) {
  const [workflows, setWorkflows] = useState<ActiveWorkflow[]>(initialWorkflows);
  const [polling, setPolling] = useState(initialWorkflows.length > 0);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const router = useRouter();

  const startPolling = useCallback(() => {
    setPolling(true);
  }, []);

  useEffect(() => {
    if (!polling) return;

    async function poll() {
      const active = await fetchActiveWorkflows(sourceId);
      setWorkflows(active);
      if (active.length === 0) {
        setPolling(false);
        router.refresh();
      }
    }

    poll();
    intervalRef.current = setInterval(poll, 2000);

    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, [polling, sourceId, router]);

  // Expose startPolling via a ref-like pattern for the RunButton
  // We use a global event instead to keep things simple
  useEffect(() => {
    function handleStart() {
      startPolling();
    }
    window.addEventListener(`workflow-started-${sourceId}`, handleStart);
    return () => window.removeEventListener(`workflow-started-${sourceId}`, handleStart);
  }, [sourceId, startPolling]);

  if (workflows.length === 0) return null;

  return (
    <div className="flex items-center gap-3 rounded-lg border border-indigo-200 bg-indigo-50 px-4 py-3">
      <span className="inline-block h-2 w-2 animate-pulse rounded-full bg-indigo-500" />
      <div className="flex flex-col gap-1">
        {workflows.map((w, i) => {
          const workflowLabel = WORKFLOW_LABELS[w.workflowType] || w.workflowType;
          const stageLabel = w.stage ? STAGE_LABELS[w.stage] || w.stage : w.status;
          const elapsed = w.createdAt
            ? `${Math.round((Date.now() - new Date(w.createdAt).getTime()) / 1000)}s`
            : null;
          return (
            <span key={i} className="text-sm text-indigo-700">
              <span className="font-medium">{workflowLabel}:</span>{" "}
              {stageLabel}
              {elapsed && <span className="ml-2 text-xs text-indigo-400">{elapsed}</span>}
            </span>
          );
        })}
      </div>
    </div>
  );
}

export function RunButton({ sourceId }: { sourceId: string }) {
  const [loading, setLoading] = useState(false);

  async function handleClick() {
    setLoading(true);
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
      // Signal WorkflowStatus to start polling
      window.dispatchEvent(new Event(`workflow-started-${sourceId}`));
    } catch (err) {
      console.error("Run failed:", err);
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
      {loading ? "Starting..." : "Run"}
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

  function handleDone() {
    setOpen(false);
    window.dispatchEvent(new Event(`workflow-started-${sourceId}`));
  }

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
          <DeleteSourceItem sourceId={sourceId} />
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

function DeleteSourceItem({ sourceId }: { sourceId: string }) {
  const [loading, setLoading] = useState(false);
  const router = useRouter();

  async function handleClick() {
    if (!confirm("Delete this source? This cannot be undone.")) return;

    setLoading(true);
    try {
      const res = await fetch("/api/graphql", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query: `mutation($ids: [UUID!]!) { deleteSources(ids: $ids) }`,
          variables: { ids: [sourceId] },
        }),
      });
      const data = await res.json();
      if (data.errors) throw new Error(data.errors[0].message);
      router.push("/sources");
    } catch (err) {
      console.error("Delete failed:", err);
      setLoading(false);
    }
  }

  return (
    <button
      onClick={handleClick}
      disabled={loading}
      className="block w-full px-4 py-2 text-left text-sm text-red-600 hover:bg-red-50 disabled:opacity-50"
    >
      {loading ? "Deleting..." : "Delete"}
    </button>
  );
}
