import { headers } from "next/headers";
import { authedClient } from "@/lib/client";
import Link from "next/link";
import { ContentTabs } from "./content-tabs";

interface SnapshotDetail {
  id: string;
  sourceId: string | null;
  url: string;
  canonicalUrl: string;
  contentHash: string;
  fetchedVia: string;
  html: string | null;
  content: string | null;
  metadata: Record<string, unknown>;
  crawledAt: string;
  extractionStatus: string;
  extractionCompletedAt: string | null;
}

export default async function SnapshotDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  const headerStore = await headers();
  const api = authedClient(headerStore.get("cookie") ?? undefined);

  const { pageSnapshot: snapshot } = await api.query<{
    pageSnapshot: SnapshotDetail;
  }>(
    `query PageSnapshot($id: UUID!) {
      pageSnapshot(id: $id) {
        id sourceId url canonicalUrl contentHash fetchedVia html content
        metadata crawledAt extractionStatus extractionCompletedAt
      }
    }`,
    { id },
  );

  return (
    <div>
      <div className="mb-6 flex items-center gap-3">
        <Link
          href={snapshot.sourceId ? `/sources/${snapshot.sourceId}` : "/sources"}
          className="text-sm text-gray-500 hover:text-gray-700"
        >
          &larr; Source
        </Link>
        <h1 className="text-2xl font-bold truncate max-w-2xl" title={snapshot.url}>
          {snapshot.url.replace(/^https?:\/\//, "").slice(0, 80)}
        </h1>
      </div>

      <div className="rounded-lg border border-gray-200 bg-white">
        <dl className="divide-y divide-gray-200">
          <div className="grid grid-cols-4 gap-4 px-6 py-4">
            <dt className="text-sm font-medium text-gray-500">URL</dt>
            <dd className="col-span-3 text-sm text-gray-900">
              <a
                href={snapshot.url}
                target="_blank"
                rel="noopener noreferrer"
                className="text-green-700 hover:underline break-all"
              >
                {snapshot.url}
              </a>
            </dd>
          </div>
          {snapshot.canonicalUrl !== snapshot.url && (
            <div className="grid grid-cols-4 gap-4 px-6 py-4">
              <dt className="text-sm font-medium text-gray-500">Canonical URL</dt>
              <dd className="col-span-3 text-sm text-gray-900 break-all">
                {snapshot.canonicalUrl}
              </dd>
            </div>
          )}
          <div className="grid grid-cols-4 gap-4 px-6 py-4">
            <dt className="text-sm font-medium text-gray-500">Fetched Via</dt>
            <dd className="col-span-3 text-sm text-gray-900">{snapshot.fetchedVia}</dd>
          </div>
          <div className="grid grid-cols-4 gap-4 px-6 py-4">
            <dt className="text-sm font-medium text-gray-500">Crawled At</dt>
            <dd className="col-span-3 text-sm text-gray-900">
              {new Date(snapshot.crawledAt).toLocaleString()}
            </dd>
          </div>
          <div className="grid grid-cols-4 gap-4 px-6 py-4">
            <dt className="text-sm font-medium text-gray-500">Extraction</dt>
            <dd className="col-span-3 text-sm">
              <span
                className={`rounded px-2 py-0.5 text-xs font-medium ${
                  snapshot.extractionStatus === "completed"
                    ? "bg-green-100 text-green-700"
                    : "bg-yellow-100 text-yellow-700"
                }`}
              >
                {snapshot.extractionStatus}
              </span>
              {snapshot.extractionCompletedAt && (
                <span className="ml-2 text-xs text-gray-500">
                  {new Date(snapshot.extractionCompletedAt).toLocaleString()}
                </span>
              )}
            </dd>
          </div>
          <div className="grid grid-cols-4 gap-4 px-6 py-4">
            <dt className="text-sm font-medium text-gray-500">Content Hash</dt>
            <dd className="col-span-3 font-mono text-xs text-gray-500">
              {snapshot.contentHash}
            </dd>
          </div>
          {Object.keys(snapshot.metadata).length > 0 && (
            <div className="grid grid-cols-4 gap-4 px-6 py-4">
              <dt className="text-sm font-medium text-gray-500">Metadata</dt>
              <dd className="col-span-3">
                <pre className="rounded bg-gray-50 p-3 text-xs text-gray-800 overflow-x-auto">
                  {JSON.stringify(snapshot.metadata, null, 2)}
                </pre>
              </dd>
            </div>
          )}
        </dl>
      </div>

      {/* Content */}
      <div className="mt-8 min-w-0">
        <h2 className="mb-4 text-lg font-semibold">Content</h2>
        {snapshot.html ? (
          <div className="rounded-lg border border-gray-200 bg-white p-6 overflow-hidden">
            <ContentTabs
              html={snapshot.html}
              content={snapshot.content ?? ""}
            />
          </div>
        ) : snapshot.content ? (
          <div className="rounded-lg border border-gray-200 bg-white p-6 overflow-hidden">
            <pre className="whitespace-pre-wrap break-all text-sm text-gray-800 leading-relaxed overflow-x-auto max-w-full">
              {snapshot.content}
            </pre>
          </div>
        ) : (
          <p className="text-sm text-gray-500">No content available.</p>
        )}
      </div>
    </div>
  );
}
