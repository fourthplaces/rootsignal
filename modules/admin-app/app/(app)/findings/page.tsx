import { headers } from "next/headers";
import { authedClient } from "@/lib/client";
import Link from "next/link";

interface Finding {
  id: string;
  title: string;
  summary: string;
  status: string;
  validationStatus: string | null;
  signalVelocity: number | null;
  connectionCount: number;
  createdAt: string;
}

const STATUS_LABELS: Record<string, string> = {
  emerging: "Emerging",
  active: "Active",
  declining: "Declining",
  resolved: "Resolved",
};

const STATUS_COLORS: Record<string, string> = {
  emerging: "bg-yellow-100 text-yellow-800",
  active: "bg-red-100 text-red-800",
  declining: "bg-gray-100 text-gray-600",
  resolved: "bg-green-100 text-green-800",
};

export default async function FindingsPage({
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
  if (params.status) variables.status = params.status.toUpperCase();

  const { findings } = await api.query<{
    findings: {
      nodes: Finding[];
      totalCount: number;
    };
  }>(
    `query Findings($limit: Int, $offset: Int, $status: FindingStatus) {
      findings(limit: $limit, offset: $offset, status: $status) {
        nodes {
          id title summary status validationStatus
          signalVelocity connectionCount createdAt
        }
        totalCount
      }
    }`,
    variables,
  );

  const offset = parseInt(params.offset || "0");
  const activeStatus = params.status || null;

  return (
    <div>
      <div className="mb-6 flex items-center justify-between">
        <h1 className="text-2xl font-bold">Findings</h1>
        <span className="text-sm text-gray-500">
          {findings.totalCount} total
        </span>
      </div>

      {/* Status filter tabs */}
      <div className="mb-4 flex gap-2">
        <Link
          href="/findings"
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
            href={`/findings?status=${key}`}
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
                Title
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Signals
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Velocity
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                Date
              </th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-200">
            {findings.nodes.map((finding) => (
              <tr key={finding.id} className="hover:bg-gray-50">
                <td className="whitespace-nowrap px-4 py-3">
                  <span
                    className={`inline-block rounded-full px-2 py-0.5 text-xs font-medium ${
                      STATUS_COLORS[finding.status] || "bg-gray-100"
                    }`}
                  >
                    {STATUS_LABELS[finding.status] || finding.status}
                  </span>
                </td>
                <td className="max-w-md px-4 py-3">
                  <Link
                    href={`/findings/${finding.id}`}
                    className="text-sm text-blue-600 hover:underline"
                  >
                    {finding.title}
                  </Link>
                  <p className="mt-0.5 text-xs text-gray-500 line-clamp-1">
                    {finding.summary}
                  </p>
                </td>
                <td className="px-4 py-3 text-sm text-gray-500">
                  {finding.connectionCount}
                </td>
                <td className="px-4 py-3 text-sm text-gray-500">
                  {finding.signalVelocity
                    ? `${finding.signalVelocity.toFixed(1)}/day`
                    : "-"}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-xs text-gray-500">
                  {new Date(finding.createdAt).toLocaleDateString()}
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
            href={`/findings?offset=${Math.max(0, offset - 50)}${activeStatus ? `&status=${activeStatus}` : ""}`}
            className="text-sm text-blue-600 hover:underline"
          >
            Previous
          </Link>
        )}
        {findings.nodes.length === 50 && (
          <Link
            href={`/findings?offset=${offset + 50}${activeStatus ? `&status=${activeStatus}` : ""}`}
            className="ml-auto text-sm text-blue-600 hover:underline"
          >
            Next
          </Link>
        )}
      </div>
    </div>
  );
}
