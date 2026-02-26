import { useQuery } from "@apollo/client";
import { ADMIN_DASHBOARD } from "@/graphql/queries";
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  PieChart,
  Pie,
  Cell,
  AreaChart,
  Area,
} from "recharts";

const COLORS = ["#8b5cf6", "#06b6d4", "#f59e0b", "#10b981", "#ef4444", "#ec4899"];

export function DashboardPage() {
  const region = "twincities";
  const { data, loading } = useQuery(ADMIN_DASHBOARD, {
    variables: { region },
  });

  if (loading) return <p className="text-muted-foreground">Loading dashboard...</p>;

  const d = data?.adminDashboard;
  if (!d) return <p className="text-muted-foreground">No data</p>;

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Dashboard</h1>
      </div>

      {/* Stat cards */}
      <div className="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-4">
        {[
          { label: "Signals", value: d.totalSignals },
          { label: "Actors", value: d.totalActors },
          { label: "Sources", value: d.activeSources },
          { label: "Tensions", value: d.totalTensions },
          {
            label: "Scout",
            value: d.scoutStatuses.find((s: { regionSlug: string }) => s.regionSlug === region)?.running
              ? "Running"
              : "Idle",
          },
        ].map((stat) => (
          <div key={stat.label} className="rounded-lg border border-border p-4">
            <p className="text-xs text-muted-foreground">{stat.label}</p>
            <p className="text-2xl font-semibold mt-1">{stat.value}</p>
          </div>
        ))}
      </div>

      {/* Signal volume chart */}
      <div className="rounded-lg border border-border p-4">
        <h2 className="text-sm font-medium mb-4">Signal Volume (7 day)</h2>
        <ResponsiveContainer width="100%" height={200}>
          <AreaChart data={d.signalVolumeByDay}>
            <XAxis dataKey="day" tick={{ fontSize: 11 }} />
            <YAxis tick={{ fontSize: 11 }} />
            <Tooltip />
            <Area type="monotone" dataKey="gatherings" stackId="1" fill="#8b5cf6" stroke="#8b5cf6" />
            <Area type="monotone" dataKey="aids" stackId="1" fill="#06b6d4" stroke="#06b6d4" />
            <Area type="monotone" dataKey="needs" stackId="1" fill="#f59e0b" stroke="#f59e0b" />
            <Area type="monotone" dataKey="notices" stackId="1" fill="#10b981" stroke="#10b981" />
            <Area type="monotone" dataKey="tensions" stackId="1" fill="#ef4444" stroke="#ef4444" />
          </AreaChart>
        </ResponsiveContainer>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {/* Type distribution */}
        <div className="rounded-lg border border-border p-4">
          <h2 className="text-sm font-medium mb-4">By Type</h2>
          <ResponsiveContainer width="100%" height={200}>
            <PieChart>
              <Pie
                data={d.countByType}
                dataKey="count"
                nameKey="signalType"
                cx="50%"
                cy="50%"
                outerRadius={80}
                label={({ signalType }: { signalType: string }) => signalType}
              >
                {d.countByType.map((_: unknown, i: number) => (
                  <Cell key={i} fill={COLORS[i % COLORS.length]} />
                ))}
              </Pie>
              <Tooltip />
            </PieChart>
          </ResponsiveContainer>
        </div>

      </div>

      {/* Unmet tensions table */}
      <div className="rounded-lg border border-border p-4">
        <h2 className="text-sm font-medium mb-4">Unmet Tensions</h2>
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border text-left text-muted-foreground">
                <th className="pb-2 font-medium">Title</th>
                <th className="pb-2 font-medium">Severity</th>
                <th className="pb-2 font-medium">Category</th>
                <th className="pb-2 font-medium">What Would Help</th>
              </tr>
            </thead>
            <tbody>
              {d.unmetTensions.map(
                (t: { title: string; severity: string; category: string; whatWouldHelp: string }, i: number) => (
                  <tr key={i} className="border-b border-border/50">
                    <td className="py-2">{t.title}</td>
                    <td className="py-2">{t.severity}</td>
                    <td className="py-2">{t.category}</td>
                    <td className="py-2 text-muted-foreground">{t.whatWouldHelp}</td>
                  </tr>
                ),
              )}
            </tbody>
          </table>
        </div>
      </div>

      {/* Source performance */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <div className="rounded-lg border border-border p-4">
          <h2 className="text-sm font-medium mb-4">Top Sources</h2>
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border text-left text-muted-foreground">
                <th className="pb-2 font-medium">Source</th>
                <th className="pb-2 font-medium">Signals</th>
                <th className="pb-2 font-medium">Weight</th>
              </tr>
            </thead>
            <tbody>
              {d.topSources.map((s: { name: string; signals: number; weight: number }, i: number) => (
                <tr key={i} className="border-b border-border/50">
                  <td className="py-1.5 truncate max-w-[200px]">{s.name}</td>
                  <td className="py-1.5">{s.signals}</td>
                  <td className="py-1.5">{s.weight.toFixed(2)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        <div className="rounded-lg border border-border p-4">
          <h2 className="text-sm font-medium mb-4">Bottom Sources</h2>
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border text-left text-muted-foreground">
                <th className="pb-2 font-medium">Source</th>
                <th className="pb-2 font-medium">Signals</th>
                <th className="pb-2 font-medium">Empty</th>
              </tr>
            </thead>
            <tbody>
              {d.bottomSources.map(
                (s: { name: string; signals: number; emptyRuns: number }, i: number) => (
                  <tr key={i} className="border-b border-border/50">
                    <td className="py-1.5 truncate max-w-[200px]">{s.name}</td>
                    <td className="py-1.5">{s.signals}</td>
                    <td className="py-1.5">{s.emptyRuns}</td>
                  </tr>
                ),
              )}
            </tbody>
          </table>
        </div>
      </div>

      {/* Extraction yield */}
      <div className="rounded-lg border border-border p-4">
        <h2 className="text-sm font-medium mb-4">Extraction Yield</h2>
        <ResponsiveContainer width="100%" height={200}>
          <BarChart data={d.extractionYield}>
            <XAxis dataKey="sourceLabel" tick={{ fontSize: 11 }} />
            <YAxis tick={{ fontSize: 11 }} />
            <Tooltip />
            <Bar dataKey="extracted" fill="#8b5cf6" name="Extracted" />
            <Bar dataKey="survived" fill="#06b6d4" name="Survived" />
            <Bar dataKey="corroborated" fill="#10b981" name="Corroborated" />
          </BarChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}
