import { headers } from "next/headers";
import { authedClient } from "@/lib/client";
import Link from "next/link";

interface Entity {
  id: string;
  name: string;
  entityType: string;
  verified: boolean;
  sourceLocale: string;
  createdAt: string;
}

export default async function EntitiesPage({
  searchParams,
}: {
  searchParams: Promise<{ after?: string }>;
}) {
  const params = await searchParams;
  const headerStore = await headers();
  const api = authedClient(headerStore.get("cookie") ?? undefined);

  const variables: Record<string, unknown> = { first: 25 };
  if (params.after) variables.after = params.after;

  const { entities } = await api.query<{
    entities: {
      nodes: Entity[];
      pageInfo: { hasNextPage: boolean; endCursor: string | null };
    };
  }>(
    `query Entities($first: Int, $after: String) {
      entities(first: $first, after: $after) {
        nodes { id name entityType verified sourceLocale createdAt }
        pageInfo { hasNextPage endCursor }
      }
    }`,
    variables,
  );

  return (
    <div>
      <h1 className="mb-6 text-2xl font-bold">Entities</h1>

      <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
        <table className="min-w-full divide-y divide-gray-200">
          <thead className="bg-gray-50">
            <tr>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Name</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Type</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Verified</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Locale</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Created</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-200">
            {entities.nodes.map((e) => (
              <tr key={e.id} className="hover:bg-gray-50">
                <td className="px-4 py-3">
                  <Link href={`/entities/${e.id}`} className="text-green-700 hover:underline">
                    {e.name}
                  </Link>
                </td>
                <td className="px-4 py-3 text-sm text-gray-600">{e.entityType}</td>
                <td className="px-4 py-3">
                  {e.verified ? (
                    <span className="text-green-600">Yes</span>
                  ) : (
                    <span className="text-gray-400">No</span>
                  )}
                </td>
                <td className="px-4 py-3 text-sm text-gray-600">{e.sourceLocale}</td>
                <td className="px-4 py-3 text-sm text-gray-500">
                  {new Date(e.createdAt).toLocaleDateString()}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {entities.pageInfo.hasNextPage && (
        <div className="mt-4">
          <Link
            href={`/entities?after=${entities.pageInfo.endCursor}`}
            className="text-sm text-green-700 hover:underline"
          >
            Load more
          </Link>
        </div>
      )}
    </div>
  );
}
