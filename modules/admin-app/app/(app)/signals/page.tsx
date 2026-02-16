import { headers } from "next/headers";
import { authedClient } from "@/lib/client";
import Link from "next/link";
import { RunInvestigationsButton } from "./run-investigations-button";

interface Signal {
  id: string;
  signalType: string;
  content: string;
  about: string | null;
  entityId: string | null;
  confidence: number;
  inLanguage: string;
  investigationStatus: string | null;
  createdAt: string;
}

const INV_STATUS_COLORS: Record<string, string> = {
  pending: "bg-yellow-100 text-yellow-800",
  in_progress: "bg-blue-100 text-blue-800",
  completed: "bg-green-100 text-green-800",
  linked: "bg-purple-100 text-purple-800",
};

const INV_STATUS_LABELS: Record<string, string> = {
  pending: "Pending",
  in_progress: "Investigating",
  completed: "Complete",
  linked: "Linked",
};

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

export default async function SignalsPage({
  searchParams,
}: {
  searchParams: Promise<{ type?: string; offset?: string }>;
}) {
  const params = await searchParams;
  const headerStore = await headers();
  const api = authedClient(headerStore.get("cookie") ?? undefined);

  const variables: Record<string, unknown> = {
    limit: 50,
    offset: parseInt(params.offset || "0"),
  };
  if (params.type) variables.type = params.type.toUpperCase();

  const { signals } = await api.query<{
    signals: {
      nodes: Signal[];
      totalCount: number;
    };
  }>(
    `query Signals($limit: Int, $offset: Int, $type: SignalType) {
      signals(limit: $limit, offset: $offset, type: $type) {
        nodes {
          id signalType content about entityId
          confidence inLanguage investigationStatus createdAt
        }
        totalCount
      }
    }`,
    variables,
  );

  const offset = parseInt(params.offset || "0");
  const activeType = params.type || null;

  return (
    <div>
      <div className="mb-6 flex items-center justify-between">
        <h1 className="text-2xl font-bold">Signals</h1>
        <div className="flex items-center gap-4">
          <RunInvestigationsButton />
          <span className="text-sm text-gray-500">
            {signals.totalCount} total
          </span>
        </div>
      </div>

      {/* Type filter tabs */}
      <div className="mb-4 flex gap-2">
        <Link
          href="/signals"
          className={`rounded-full px-3 py-1 text-sm ${
            !activeType ? "bg-gray-900 text-white" : "bg-gray-100 text-gray-700 hover:bg-gray-200"
          }`}
        >
          All
        </Link>
        {Object.entries(TYPE_LABELS).map(([key, label]) => (
          <Link
            key={key}
            href={`/signals?type=${key}`}
            className={`rounded-full px-3 py-1 text-sm ${
              activeType === key
                ? "bg-gray-900 text-white"
                : "bg-gray-100 text-gray-700 hover:bg-gray-200"
            }`}
          >
            {label}
          </Link>
        ))}
      </div>

      <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
        <table className="min-w-full divide-y divide-gray-200">
          <thead className="bg-gray-50">
            <tr>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Type
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Content
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                About
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Investigation
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Date
              </th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-200">
            {signals.nodes.map((signal) => (
              <tr key={signal.id} className="hover:bg-gray-50">
                <td className="whitespace-nowrap px-4 py-3">
                  <span
                    className={`inline-block rounded-full px-2 py-0.5 text-xs font-medium ${
                      TYPE_COLORS[signal.signalType] || "bg-gray-100"
                    }`}
                  >
                    {TYPE_LABELS[signal.signalType] || signal.signalType}
                  </span>
                </td>
                <td className="max-w-md px-4 py-3">
                  <Link
                    href={`/signals/${signal.id}`}
                    className="text-sm text-blue-600 hover:underline"
                  >
                    {signal.content.length > 120
                      ? signal.content.slice(0, 120) + "..."
                      : signal.content}
                  </Link>
                </td>
                <td className="px-4 py-3 text-sm text-gray-500">
                  {signal.about || "-"}
                </td>
                <td className="whitespace-nowrap px-4 py-3">
                  {signal.investigationStatus && (
                    <span
                      className={`inline-block rounded-full px-2 py-0.5 text-xs font-medium ${
                        INV_STATUS_COLORS[signal.investigationStatus] || "bg-gray-100"
                      }`}
                    >
                      {INV_STATUS_LABELS[signal.investigationStatus] || signal.investigationStatus}
                    </span>
                  )}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-xs text-gray-500">
                  {new Date(signal.createdAt).toLocaleDateString()}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* Pagination */}
      <div className="mt-4 flex justify-between">
        {offset > 0 && (
          <Link
            href={`/signals?offset=${Math.max(0, offset - 50)}${activeType ? `&type=${activeType}` : ""}`}
            className="text-sm text-blue-600 hover:underline"
          >
            Previous
          </Link>
        )}
        {signals.nodes.length === 50 && (
          <Link
            href={`/signals?offset=${offset + 50}${activeType ? `&type=${activeType}` : ""}`}
            className="ml-auto text-sm text-blue-600 hover:underline"
          >
            Next
          </Link>
        )}
      </div>
    </div>
  );
}
