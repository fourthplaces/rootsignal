import { headers } from "next/headers";
import { authedClient } from "@/lib/client";
import Link from "next/link";
import { DetectEntityButton } from "./detect-entity-button";
import { RunButton, SourceMoreMenu } from "./run-button";

interface Source {
  id: string;
  name: string;
  sourceType: string;
  url: string | null;
  handle: string | null;
  cadenceHours: number;
  lastScrapedAt: string | null;
  isActive: boolean;
  entityId: string | null;
  config: Record<string, unknown>;
  qualificationStatus: string;
  qualificationSummary: string | null;
  qualificationScore: number | null;
  createdAt: string;
}

const VERDICT_STYLES: Record<string, string> = {
  green: "bg-green-100 text-green-700",
  yellow: "bg-yellow-100 text-yellow-700",
  red: "bg-red-100 text-red-700",
  pending: "bg-gray-100 text-gray-500",
};

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
        id name sourceType url handle cadenceHours lastScrapedAt isActive entityId config
        qualificationStatus qualificationSummary qualificationScore createdAt
      }
    }`,
    { id },
  );

  const verdictStyle = VERDICT_STYLES[source.qualificationStatus] || VERDICT_STYLES.pending;

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
        </div>
        <div className="flex items-center gap-2">
          <RunButton sourceId={source.id} />
          <SourceMoreMenu sourceId={source.id} />
        </div>
      </div>

      {/* Qualification banner */}
      {source.qualificationStatus !== "pending" && (
        <div className={`mb-6 rounded-lg border p-4 ${
          source.qualificationStatus === "green" ? "border-green-200 bg-green-50" :
          source.qualificationStatus === "yellow" ? "border-yellow-200 bg-yellow-50" :
          "border-red-200 bg-red-50"
        }`}>
          <div className="flex items-center gap-3 mb-2">
            <span className={`rounded px-2 py-0.5 text-xs font-medium ${verdictStyle}`}>
              {source.qualificationStatus.toUpperCase()}
            </span>
            {source.qualificationScore !== null && (
              <span className="text-sm font-medium text-gray-700">
                Score: {source.qualificationScore}/100
              </span>
            )}
          </div>
          {source.qualificationSummary && (
            <p className="text-sm text-gray-700">{source.qualificationSummary}</p>
          )}
        </div>
      )}

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
            <dt className="text-sm font-medium text-gray-500">Cadence</dt>
            <dd className="col-span-2 text-sm text-gray-900">{source.cadenceHours} hours</dd>
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
            <dt className="text-sm font-medium text-gray-500">Qualification</dt>
            <dd className="col-span-2 text-sm">
              <span className={`rounded px-2 py-0.5 text-xs font-medium ${verdictStyle}`}>
                {source.qualificationStatus}
              </span>
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
    </div>
  );
}
