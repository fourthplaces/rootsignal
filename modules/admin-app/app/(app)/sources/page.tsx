import { headers } from "next/headers";
import { authedClient } from "@/lib/client";
import Link from "next/link";

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
  qualificationStatus: string;
}

export default async function SourcesPage() {
  const headerStore = await headers();
  const api = authedClient(headerStore.get("cookie") ?? undefined);

  const { sources } = await api.query<{ sources: Source[] }>(
    `query Sources {
      sources {
        id name sourceType url handle cadenceHours lastScrapedAt isActive entityId qualificationStatus
      }
    }`,
  );

  return (
    <div>
      <div className="mb-6 flex items-center justify-between">
        <h1 className="text-2xl font-bold">Sources</h1>
        <Link
          href="/sources/new"
          className="rounded bg-green-700 px-4 py-2 text-sm text-white hover:bg-green-800"
        >
          New Source
        </Link>
      </div>

      <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
        <table className="min-w-full divide-y divide-gray-200">
          <thead className="bg-gray-50">
            <tr>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Name</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Type</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">URL / Handle</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Cadence</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Last Scraped</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Qualification</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Active</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-200">
            {sources.map((s) => (
              <tr key={s.id} className="hover:bg-gray-50">
                <td className="px-4 py-3 text-sm font-medium">
                  <Link href={`/sources/${s.id}`} className="text-green-700 hover:underline">
                    {s.name}
                  </Link>
                </td>
                <td className="px-4 py-3">
                  <span className="rounded bg-gray-100 px-2 py-0.5 text-xs text-gray-700">
                    {s.sourceType}
                  </span>
                </td>
                <td className="px-4 py-3 text-sm text-gray-600">
                  {s.url || s.handle || "—"}
                </td>
                <td className="px-4 py-3 text-sm text-gray-600">{s.cadenceHours}h</td>
                <td className="px-4 py-3 text-sm text-gray-500">
                  {s.lastScrapedAt
                    ? new Date(s.lastScrapedAt).toLocaleDateString()
                    : "Never"}
                </td>
                <td className="px-4 py-3">
                  <span className={`rounded px-2 py-0.5 text-xs font-medium ${
                    s.qualificationStatus === "green" ? "bg-green-100 text-green-700" :
                    s.qualificationStatus === "yellow" ? "bg-yellow-100 text-yellow-700" :
                    s.qualificationStatus === "red" ? "bg-red-100 text-red-700" :
                    "bg-gray-100 text-gray-500"
                  }`}>
                    {s.qualificationStatus}
                  </span>
                </td>
                <td className="px-4 py-3">
                  {s.isActive ? (
                    <span className="text-green-600">Yes</span>
                  ) : (
                    <span className="text-gray-400">No</span>
                  )}
                </td>
              </tr>
            ))}
            {sources.length === 0 && (
              <tr>
                <td colSpan={7} className="px-4 py-8 text-center text-sm text-gray-500">
                  No sources yet. Create one to start scraping.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
