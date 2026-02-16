import { headers } from "next/headers";
import { authedClient } from "@/lib/client";
import Link from "next/link";

interface Investigation {
  id: string;
  subjectType: string;
  subjectId: string;
  trigger: string;
  status: string;
  summary: string | null;
  summaryConfidence: number | null;
  startedAt: string | null;
  completedAt: string | null;
  createdAt: string;
}

const STATUS_LABELS: Record<string, string> = {
  pending: "Pending",
  running: "Running",
  completed: "Completed",
  failed: "Failed",
};

const STATUS_COLORS: Record<string, string> = {
  pending: "bg-yellow-100 text-yellow-800",
  running: "bg-blue-100 text-blue-800",
  completed: "bg-green-100 text-green-800",
  failed: "bg-red-100 text-red-800",
};

function formatDuration(startedAt: string | null, completedAt: string | null): string {
  if (!startedAt) return "-";
  const start = new Date(startedAt).getTime();
  const end = completedAt ? new Date(completedAt).getTime() : Date.now();
  const seconds = Math.round((end - start) / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = seconds % 60;
  return `${minutes}m ${remainingSeconds}s`;
}

export default async function InvestigationsPage({
  searchParams,
}: {
  searchParams: Promise<{ status?: string; offset?: string }>;
}) {
  const params = await searchParams;
  const headerStore = await headers();
  const api = authedClient(headerStore.get("cookie") ?? undefined);

  const variables: Record<string, unknown> = {
    limit: 50,
    offset: parseInt(params.offset || "0"),
  };
  if (params.status) variables.status = params.status;

  const { investigations, investigationCount } = await api.query<{
    investigations: Investigation[];
    investigationCount: number;
  }>(
    `query Investigations($status: String, $limit: Int!, $offset: Int!) {
      investigations(status: $status, limit: $limit, offset: $offset) {
        id subjectType subjectId trigger status summary
        summaryConfidence startedAt completedAt createdAt
      }
      investigationCount(status: $status)
    }`,
    variables,
  );

  const offset = parseInt(params.offset || "0");
  const activeStatus = params.status || null;

  return (
    <div>
      <div className="mb-6 flex items-center justify-between">
        <h1 className="text-2xl font-bold">Investigations</h1>
        <span className="text-sm text-gray-500">
          {investigationCount} total
        </span>
      </div>

      {/* Status filter tabs */}
      <div className="mb-4 flex gap-2">
        <Link
          href="/investigations"
          className={`rounded-full px-3 py-1 text-sm ${
            !activeStatus
              ? "bg-gray-900 text-white"
              : "bg-gray-100 text-gray-700 hover:bg-gray-200"
          }`}
        >
          All
        </Link>
        {Object.entries(STATUS_LABELS).map(([key, label]) => (
          <Link
            key={key}
            href={`/investigations?status=${key}`}
            className={`rounded-full px-3 py-1 text-sm ${
              activeStatus === key
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
                Status
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Trigger
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Summary
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Confidence
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Duration
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Created
              </th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-200">
            {investigations.map((inv) => (
              <tr key={inv.id} className="hover:bg-gray-50">
                <td className="whitespace-nowrap px-4 py-3">
                  <span
                    className={`inline-block rounded-full px-2 py-0.5 text-xs font-medium ${
                      STATUS_COLORS[inv.status] || "bg-gray-100"
                    }`}
                  >
                    {STATUS_LABELS[inv.status] || inv.status}
                  </span>
                </td>
                <td className="px-4 py-3">
                  <Link
                    href={`/investigations/${inv.id}`}
                    className="text-sm text-blue-600 hover:underline"
                  >
                    {inv.trigger.length > 50
                      ? inv.trigger.slice(0, 50) + "..."
                      : inv.trigger}
                  </Link>
                  <div className="text-xs text-gray-400">
                    {inv.subjectType}:{" "}
                    <Link
                      href={`/${inv.subjectType}s/${inv.subjectId}`}
                      className="text-blue-500 hover:underline"
                    >
                      {inv.subjectId.slice(0, 8)}...
                    </Link>
                  </div>
                </td>
                <td className="max-w-xs px-4 py-3 text-sm text-gray-600">
                  {inv.summary
                    ? inv.summary.length > 100
                      ? inv.summary.slice(0, 100) + "..."
                      : inv.summary
                    : "-"}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-500">
                  {inv.summaryConfidence != null
                    ? `${Math.round(inv.summaryConfidence * 100)}%`
                    : "-"}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-500">
                  {formatDuration(inv.startedAt, inv.completedAt)}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-xs text-gray-500">
                  {new Date(inv.createdAt).toLocaleDateString()}
                </td>
              </tr>
            ))}
            {investigations.length === 0 && (
              <tr>
                <td colSpan={6} className="px-4 py-8 text-center text-sm text-gray-500">
                  No investigations found
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {/* Pagination */}
      <div className="mt-4 flex justify-between">
        {offset > 0 && (
          <Link
            href={`/investigations?offset=${Math.max(0, offset - 50)}${activeStatus ? `&status=${activeStatus}` : ""}`}
            className="text-sm text-blue-600 hover:underline"
          >
            Previous
          </Link>
        )}
        {investigations.length === 50 && (
          <Link
            href={`/investigations?offset=${offset + 50}${activeStatus ? `&status=${activeStatus}` : ""}`}
            className="ml-auto text-sm text-blue-600 hover:underline"
          >
            Next
          </Link>
        )}
      </div>
    </div>
  );
}
