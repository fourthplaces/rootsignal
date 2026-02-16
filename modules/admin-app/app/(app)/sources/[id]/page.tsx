import { headers } from "next/headers";
import { authedClient } from "@/lib/client";
import Link from "next/link";
import { DetectEntityButton } from "./detect-entity-button";
import { RunButton, SourceMoreMenu, WorkflowStatus } from "./run-button";
import { SourceTabs } from "./source-tabs";

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
  config: Record<string, unknown>;
  signalCount: number;
  createdAt: string;
}

interface PageSnapshot {
  id: string;
  pageUrl: string;
  url: string;
  contentHash: string;
  fetchedVia: string;
  contentPreview: string | null;
  crawledAt: string;
  scrapeStatus: string;
}

interface Signal {
  id: string;
  signalType: string;
  content: string;
  about: string | null;
  createdAt: string;
}

function formatTimeUntil(dateStr: string): string {
  const diff = new Date(dateStr).getTime() - Date.now();
  const absDiff = Math.abs(diff);
  const seconds = Math.floor(absDiff / 1000);
  const minutes = Math.floor(seconds / 60);
  const hours = Math.floor(minutes / 60);
  const days = Math.floor(hours / 24);

  let label: string;
  if (days > 0) label = `${days}d ${hours % 24}h`;
  else if (hours > 0) label = `${hours}h ${minutes % 60}m`;
  else if (minutes > 0) label = `${minutes}m`;
  else label = `${seconds}s`;

  return diff <= 0 ? `${label} overdue` : `in ${label}`;
}

export default async function SourceDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  const headerStore = await headers();
  const api = authedClient(headerStore.get("cookie") ?? undefined);

  const { source } = await api.query<{ source: Source }>(
    `query Source($id: UUID!) {
      source(id: $id) {
        id name sourceType url handle nextRunAt consecutiveMisses lastScrapedAt isActive entityId config
        signalCount createdAt
      }
    }`,
    { id },
  );

  const [{ sourcePageSnapshots: snapshots }, { activeWorkflows }, { signals: signalsConnection }] = await Promise.all([
    api.query<{ sourcePageSnapshots: PageSnapshot[] }>(
      `query Snapshots($sourceId: UUID!) {
        sourcePageSnapshots(sourceId: $sourceId) {
          id pageUrl url contentHash fetchedVia contentPreview crawledAt scrapeStatus
        }
      }`,
      { sourceId: id },
    ),
    api.query<{
      activeWorkflows: { workflowType: string; sourceId: string; status: string; stage: string | null; createdAt: string | null }[];
    }>(
      `query ActiveWorkflows($sourceId: UUID!) {
        activeWorkflows(sourceId: $sourceId) {
          workflowType sourceId status stage createdAt
        }
      }`,
      { sourceId: id },
    ).catch(() => ({ activeWorkflows: [] as { workflowType: string; sourceId: string; status: string; stage: string | null; createdAt: string | null }[] })),
    api.query<{ signals: { nodes: Signal[]; totalCount: number } }>(
      `query SourceSignals($sourceId: UUID!) {
        signals(sourceId: $sourceId, limit: 100) {
          nodes { id signalType content about createdAt }
          totalCount
        }
      }`,
      { sourceId: id },
    ).catch(() => ({ signals: { nodes: [] as Signal[], totalCount: 0 } })),
  ]);
  const signals = signalsConnection.nodes;

  return (
    <div>
      <div className="mb-6 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <Link href="/sources" className="text-sm text-gray-500 hover:text-gray-700">
            &larr; Sources
          </Link>
          <h1 className="text-2xl font-bold">{source.name}</h1>
          <span className="rounded bg-gray-100 px-2 py-0.5 text-xs text-gray-700">
            {source.sourceType}
          </span>
          {source.signalCount > 0 && (
            <span className="rounded bg-green-100 px-2 py-0.5 text-xs font-medium text-green-700">
              {source.signalCount} signal{source.signalCount === 1 ? "" : "s"}
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          <RunButton sourceId={source.id} />
          <SourceMoreMenu sourceId={source.id} />
        </div>
      </div>

      {/* Live workflow status (polls automatically) */}
      <div className="mb-6">
        <WorkflowStatus sourceId={source.id} initialWorkflows={activeWorkflows} />
      </div>

      <div className="rounded-lg border border-gray-200 bg-white">
        <dl className="divide-y divide-gray-200">
          <div className="grid grid-cols-3 gap-4 px-6 py-4">
            <dt className="text-sm font-medium text-gray-500">URL / Handle</dt>
            <dd className="col-span-2 text-sm text-gray-900">
              {source.url ? (
                <a href={source.url} target="_blank" rel="noopener noreferrer" className="text-green-700 hover:underline">
                  {source.url}
                </a>
              ) : source.handle ? (
                source.handle
              ) : (
                "—"
              )}
            </dd>
          </div>
          <div className="grid grid-cols-3 gap-4 px-6 py-4">
            <dt className="text-sm font-medium text-gray-500">Next Run</dt>
            <dd className="col-span-2 text-sm text-gray-900">
              {source.nextRunAt ? formatTimeUntil(source.nextRunAt) : "Now"}
              {source.consecutiveMisses > 0 && (
                <span className="ml-2 text-xs text-orange-500">
                  ({source.consecutiveMisses} consecutive miss{source.consecutiveMisses === 1 ? "" : "es"})
                </span>
              )}
            </dd>
          </div>
          <div className="grid grid-cols-3 gap-4 px-6 py-4">
            <dt className="text-sm font-medium text-gray-500">Last Scraped</dt>
            <dd className="col-span-2 text-sm text-gray-900">
              {source.lastScrapedAt
                ? new Date(source.lastScrapedAt).toLocaleString()
                : "Never"}
            </dd>
          </div>
          <div className="grid grid-cols-3 gap-4 px-6 py-4">
            <dt className="text-sm font-medium text-gray-500">Active</dt>
            <dd className="col-span-2 text-sm">
              {source.isActive ? (
                <span className="text-green-600">Yes</span>
              ) : (
                <span className="text-gray-400">No</span>
              )}
            </dd>
          </div>
          <div className="grid grid-cols-3 gap-4 px-6 py-4">
            <dt className="text-sm font-medium text-gray-500">Signals</dt>
            <dd className="col-span-2 text-sm">
              {source.signalCount > 0 ? (
                <span className="rounded bg-green-100 px-2 py-0.5 text-xs font-medium text-green-700">
                  {source.signalCount} signal{source.signalCount === 1 ? "" : "s"}
                </span>
              ) : (
                <span className="text-gray-400">No signals yet</span>
              )}
            </dd>
          </div>
          <div className="grid grid-cols-3 gap-4 px-6 py-4">
            <dt className="text-sm font-medium text-gray-500">Entity ID</dt>
            <dd className="col-span-2 text-sm text-gray-900">
              {source.entityId ? (
                <Link href={`/entities/${source.entityId}`} className="text-green-700 hover:underline">
                  {source.entityId}
                </Link>
              ) : (
                <div className="flex items-center gap-3">
                  <span className="text-gray-400">—</span>
                  <DetectEntityButton sourceId={source.id} />
                </div>
              )}
            </dd>
          </div>
          <div className="grid grid-cols-3 gap-4 px-6 py-4">
            <dt className="text-sm font-medium text-gray-500">Created</dt>
            <dd className="col-span-2 text-sm text-gray-900">
              {new Date(source.createdAt).toLocaleString()}
            </dd>
          </div>
          <div className="grid grid-cols-3 gap-4 px-6 py-4">
            <dt className="text-sm font-medium text-gray-500">Config</dt>
            <dd className="col-span-2">
              <pre className="rounded bg-gray-50 p-3 text-xs text-gray-800">
                {JSON.stringify(source.config, null, 2)}
              </pre>
            </dd>
          </div>
        </dl>
      </div>
      <SourceTabs snapshots={snapshots} signals={signals} />
    </div>
  );
}
