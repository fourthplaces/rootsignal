import { headers } from "next/headers";
import { authedClient } from "@/lib/client";

import { SourcesTable } from "./sources-table";

interface Source {
  id: string;
  name: string;
  sourceType: string;
  url: string | null;
  handle: string | null;
  nextRunAt: string | null;
  consecutiveMisses: number;
  lastScrapedAt: string | null;
  isActive: boolean;
  entityId: string | null;
  signalCount: string;
}

export default async function SourcesPage({
  searchParams,
}: {
  searchParams: Promise<{ type?: string; q?: string }>;
}) {
  const params = await searchParams;
  const headerStore = await headers();
  const api = authedClient(headerStore.get("cookie") ?? undefined);
  const searchQuery = params.q || null;

  let allSources: Source[];

  if (searchQuery) {
    const { searchSources } = await api.query<{ searchSources: Source[] }>(
      `query SearchSources($q: String!) {
        searchSources(q: $q) {
          id name sourceType url handle nextRunAt consecutiveMisses lastScrapedAt isActive entityId signalCount
        }
      }`,
      { q: searchQuery },
    );
    allSources = searchSources;
  } else {
    const { sources } = await api.query<{ sources: Source[] }>(
      `query Sources {
        sources {
          id name sourceType url handle nextRunAt consecutiveMisses lastScrapedAt isActive entityId signalCount
        }
      }`,
    );
    allSources = sources;
  }

  // Fetch active workflows from Restate
  const { activeWorkflows } = await api.query<{
    activeWorkflows: { workflowType: string; sourceId: string; status: string; stage: string | null }[];
  }>(
    `query ActiveWorkflows {
      activeWorkflows {
        workflowType sourceId status stage
      }
    }`,
  ).catch(() => ({ activeWorkflows: [] as { workflowType: string; sourceId: string; status: string; stage: string | null }[] }));

  // Build a map: sourceId -> workflow info
  const workflowsBySource: Record<string, { workflowType: string; stage: string | null }[]> = {};
  for (const w of activeWorkflows) {
    if (!workflowsBySource[w.sourceId]) workflowsBySource[w.sourceId] = [];
    workflowsBySource[w.sourceId].push({ workflowType: w.workflowType, stage: w.stage });
  }

  const activeType = params.type || null;
  const sources = activeType
    ? allSources.filter((s) => s.sourceType === activeType)
    : allSources;

  return (
    <div>
      <SourcesTable
        sources={sources}
        allSources={allSources}
        initialQuery={searchQuery ?? ""}
        activeType={activeType}
        workflowsBySource={workflowsBySource}
      />
    </div>
  );
}
