import { headers } from "next/headers";
import { authedClient } from "@/lib/client";
import Link from "next/link";

interface Listing {
  id: string;
  title: string;
  status: string;
  inLanguage: string;
  createdAt: string;
  entity: { id: string; name: string } | null;
}

interface PageInfo {
  hasNextPage: boolean;
  endCursor: string | null;
}

export default async function ListingsPage({
  searchParams,
}: {
  searchParams: Promise<{ after?: string; status?: string }>;
}) {
  const params = await searchParams;
  const headerStore = await headers();
  const api = authedClient(headerStore.get("cookie") ?? undefined);

  const variables: Record<string, unknown> = { first: 25 };
  if (params.after) variables.after = params.after;

  const { listings } = await api.query<{
    listings: {
      nodes: Listing[];
      pageInfo: PageInfo;
    };
  }>(
    `query Listings($first: Int, $after: String) {
      listings(first: $first, after: $after) {
        nodes { id title status inLanguage createdAt entity { id name } }
        pageInfo { hasNextPage endCursor }
      }
    }`,
    variables,
  );

  return (
    <div>
      <div className="mb-6 flex items-center justify-between">
        <h1 className="text-2xl font-bold">Listings</h1>
        <Link
          href="/listings/new"
          className="rounded bg-green-700 px-4 py-2 text-sm text-white hover:bg-green-800"
        >
          New Listing
        </Link>
      </div>

      <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
        <table className="min-w-full divide-y divide-gray-200">
          <thead className="bg-gray-50">
            <tr>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Title</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Entity</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Status</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Locale</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Created</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-200">
            {listings.nodes.map((l) => (
              <tr key={l.id} className="hover:bg-gray-50">
                <td className="px-4 py-3">
                  <Link href={`/listings/${l.id}`} className="text-green-700 hover:underline">
                    {l.title}
                  </Link>
                </td>
                <td className="px-4 py-3 text-sm text-gray-600">
                  {l.entity ? (
                    <Link href={`/entities/${l.entity.id}`} className="hover:underline">
                      {l.entity.name}
                    </Link>
                  ) : (
                    <span className="text-gray-400">-</span>
                  )}
                </td>
                <td className="px-4 py-3">
                  <StatusBadge status={l.status} />
                </td>
                <td className="px-4 py-3 text-sm text-gray-600">{l.inLanguage}</td>
                <td className="px-4 py-3 text-sm text-gray-500">
                  {new Date(l.createdAt).toLocaleDateString()}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {listings.pageInfo.hasNextPage && (
        <div className="mt-4">
          <Link
            href={`/listings?after=${listings.pageInfo.endCursor}`}
            className="text-sm text-green-700 hover:underline"
          >
            Load more
          </Link>
        </div>
      )}
    </div>
  );
}

function StatusBadge({ status }: { status: string }) {
  const colors: Record<string, string> = {
    active: "bg-green-100 text-green-800",
    archived: "bg-gray-100 text-gray-600",
    draft: "bg-yellow-100 text-yellow-800",
  };
  return (
    <span className={`rounded-full px-2 py-0.5 text-xs font-medium ${colors[status] || "bg-gray-100 text-gray-600"}`}>
      {status}
    </span>
  );
}
