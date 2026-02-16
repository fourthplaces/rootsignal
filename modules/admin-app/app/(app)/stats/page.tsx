import { headers } from "next/headers";
import { authedClient } from "@/lib/client";

interface TagCount {
  value: string;
  count: number;
}

interface SignalStats {
  totalSignals: number;
  totalSources: number;
  totalSnapshots: number;
  totalExtractions: number;
  totalEntities: number;
  recent7D: number;
  signalsByType: TagCount[];
  signalsByDomain: TagCount[];
}

export default async function StatsPage() {
  const headerStore = await headers();
  const api = authedClient(headerStore.get("cookie") ?? undefined);

  const { signalStats } = await api.query<{ signalStats: SignalStats }>(
    `query {
      signalStats {
        totalSignals totalSources totalSnapshots
        totalExtractions totalEntities recent7D
        signalsByType { value count }
        signalsByDomain { value count }
      }
    }`,
  );

  const stats = signalStats;

  return (
    <div>
      <h1 className="mb-6 text-2xl font-bold">Statistics</h1>

      <div className="mb-8 grid grid-cols-2 gap-4 sm:grid-cols-4">
        <Stat label="Total Signals" value={stats.totalSignals} />
        <Stat label="Entities" value={stats.totalEntities} />
        <Stat label="Sources" value={stats.totalSources} />
        <Stat label="Snapshots" value={stats.totalSnapshots} />
        <Stat label="Extractions" value={stats.totalExtractions} />
        <Stat label="New (7d)" value={stats.recent7D} />
      </div>

      <div className="grid gap-6 lg:grid-cols-2">
        <BreakdownTable title="By Signal Type" data={stats.signalsByType} />
        <BreakdownTable title="By Signal Domain" data={stats.signalsByDomain} />
      </div>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4">
      <p className="text-sm text-gray-500">{label}</p>
      <p className="text-2xl font-bold">{value.toLocaleString()}</p>
    </div>
  );
}

function BreakdownTable({ title, data }: { title: string; data: TagCount[] }) {
  if (data.length === 0) return null;

  const total = data.reduce((sum, d) => sum + d.count, 0);

  return (
    <div className="rounded-lg border border-gray-200 bg-white">
      <h3 className="border-b border-gray-200 px-4 py-3 font-medium">{title}</h3>
      <table className="min-w-full">
        <tbody className="divide-y divide-gray-100">
          {data.map((d) => (
            <tr key={d.value}>
              <td className="px-4 py-2 text-sm">{d.value}</td>
              <td className="px-4 py-2 text-right text-sm font-medium">{d.count}</td>
              <td className="px-4 py-2 text-right text-xs text-gray-400">
                {((d.count / total) * 100).toFixed(1)}%
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
