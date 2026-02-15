import { headers } from "next/headers";
import { authedClient } from "@/lib/client";
import Link from "next/link";

interface Signal {
  id: string;
  signalType: string;
  content: string;
  about: string | null;
  entityId: string | null;
  sourceUrl: string | null;
  sourceCitationUrl: string | null;
  institutionalSource: string | null;
  confidence: number;
  inLanguage: string;
  createdAt: string;
  updatedAt: string;
}

const TYPE_LABELS: Record<string, string> = {
  ask: "Ask",
  give: "Give",
  event: "Event",
  informative: "Informative",
};

const TYPE_COLORS: Record<string, string> = {
  ask: "bg-orange-100 text-orange-800",
  give: "bg-green-100 text-green-800",
  event: "bg-blue-100 text-blue-800",
  informative: "bg-gray-100 text-gray-800",
};

export default async function SignalDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  const headerStore = await headers();
  const api = authedClient(headerStore.get("cookie") ?? undefined);

  const { signal } = await api.query<{ signal: Signal }>(
    `query Signal($id: ID!) {
      signal(id: $id) {
        id signalType content about entityId
        sourceUrl sourceCitationUrl institutionalSource
        confidence inLanguage createdAt updatedAt
      }
    }`,
    { id },
  );

  return (
    <div className="mx-auto max-w-3xl">
      <div className="mb-4">
        <Link href="/signals" className="text-sm text-blue-600 hover:underline">
          &larr; Back to Signals
        </Link>
      </div>

      <div className="rounded-lg border border-gray-200 bg-white p-6">
        <div className="mb-4 flex items-center gap-3">
          <span
            className={`inline-block rounded-full px-3 py-1 text-sm font-medium ${
              TYPE_COLORS[signal.signalType] || "bg-gray-100"
            }`}
          >
            {TYPE_LABELS[signal.signalType] || signal.signalType}
          </span>
          <span className="text-sm text-gray-400">
            Confidence: {Math.round(signal.confidence * 100)}%
          </span>
          <span className="text-sm text-gray-400">{signal.inLanguage}</span>
        </div>

        <p className="mb-4 text-lg">{signal.content}</p>

        {signal.about && (
          <div className="mb-4">
            <span className="text-sm font-medium text-gray-500">About:</span>
            <span className="ml-2 text-sm">{signal.about}</span>
          </div>
        )}

        {signal.entityId && (
          <div className="mb-4">
            <Link
              href={`/entities/${signal.entityId}`}
              className="text-sm text-blue-600 hover:underline"
            >
              View Entity &rarr;
            </Link>
          </div>
        )}

        <div className="mt-6 border-t border-gray-100 pt-4">
          <h3 className="mb-2 text-sm font-medium text-gray-500">
            Provenance
          </h3>
          <dl className="grid grid-cols-2 gap-2 text-sm">
            {signal.institutionalSource && (
              <>
                <dt className="text-gray-500">Source</dt>
                <dd>{signal.institutionalSource}</dd>
              </>
            )}
            {signal.sourceUrl && (
              <>
                <dt className="text-gray-500">URL</dt>
                <dd>
                  <a
                    href={signal.sourceUrl}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-blue-600 hover:underline"
                  >
                    {signal.sourceUrl.length > 60
                      ? signal.sourceUrl.slice(0, 60) + "..."
                      : signal.sourceUrl}
                  </a>
                </dd>
              </>
            )}
            {signal.sourceCitationUrl && (
              <>
                <dt className="text-gray-500">Citation</dt>
                <dd>
                  <a
                    href={signal.sourceCitationUrl}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-blue-600 hover:underline"
                  >
                    Government source
                  </a>
                </dd>
              </>
            )}
            <dt className="text-gray-500">Created</dt>
            <dd>{new Date(signal.createdAt).toLocaleString()}</dd>
            <dt className="text-gray-500">Updated</dt>
            <dd>{new Date(signal.updatedAt).toLocaleString()}</dd>
          </dl>
        </div>
      </div>
    </div>
  );
}
