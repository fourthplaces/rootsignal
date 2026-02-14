import { headers } from "next/headers";
import { authedClient } from "@/lib/client";
import Link from "next/link";

export default async function DashboardPage() {
  const headerStore = await headers();
  const api = authedClient(headerStore.get("cookie") ?? undefined);

  const { listingStats } = await api.query<{
    listingStats: {
      totalListings: number;
      activeListings: number;
      totalEntities: number;
      totalSources: number;
      totalExtractions: number;
      recent7D: number;
    };
  }>(`query { listingStats { totalListings activeListings totalEntities totalSources totalExtractions recent7D } }`);

  const stats = listingStats;

  return (
    <div>
      <h1 className="mb-6 text-2xl font-bold">Dashboard</h1>
      <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4">
        <StatCard label="Total Listings" value={stats.totalListings} />
        <StatCard label="Active Listings" value={stats.activeListings} />
        <StatCard label="Entities" value={stats.totalEntities} />
        <StatCard label="Sources" value={stats.totalSources} />
        <StatCard label="Extractions" value={stats.totalExtractions} />
        <StatCard label="New (7d)" value={stats.recent7D} />
      </div>

      <h2 className="mb-4 mt-8 text-lg font-semibold">Quick Actions</h2>
      <div className="flex flex-wrap gap-3">
        <Link
          href="/observations"
          className="rounded bg-yellow-100 px-4 py-2 text-sm font-medium text-yellow-800 hover:bg-yellow-200"
        >
          Review Observations
        </Link>
        <Link
          href="/listings/new"
          className="rounded bg-green-100 px-4 py-2 text-sm font-medium text-green-800 hover:bg-green-200"
        >
          New Listing
        </Link>
        <Link
          href="/workflows"
          className="rounded bg-blue-100 px-4 py-2 text-sm font-medium text-blue-800 hover:bg-blue-200"
        >
          Workflows
        </Link>
      </div>
    </div>
  );
}

function StatCard({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4">
      <p className="text-sm text-gray-500">{label}</p>
      <p className="text-2xl font-bold">{value.toLocaleString()}</p>
    </div>
  );
}
