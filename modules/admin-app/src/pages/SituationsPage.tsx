import { Link } from "react-router";
import { useQuery } from "@apollo/client";
import { SITUATIONS } from "@/graphql/queries";

const ARC_COLORS: Record<string, string> = {
  EMERGING: "bg-blue-500/20 text-blue-300",
  DEVELOPING: "bg-green-500/20 text-green-300",
  ACTIVE: "bg-orange-500/20 text-orange-300",
  COOLING: "bg-gray-500/20 text-gray-300",
  COLD: "bg-gray-500/20 text-gray-500",
};

export function SituationsPage() {
  const { data, loading } = useQuery(SITUATIONS, {
    variables: { limit: 50 },
  });

  if (loading) return <p className="text-muted-foreground">Loading situations...</p>;

  const situations = data?.situations ?? [];

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-semibold">Situations</h1>
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-border text-left text-muted-foreground">
              <th className="pb-2 font-medium">Headline</th>
              <th className="pb-2 font-medium">Arc</th>
              <th className="pb-2 font-medium">Temp</th>
              <th className="pb-2 font-medium">Clarity</th>
              <th className="pb-2 font-medium">Signals</th>
              <th className="pb-2 font-medium">Dispatches</th>
            </tr>
          </thead>
          <tbody>
            {situations.map(
              (s: {
                id: string;
                headline: string;
                arc: string;
                temperature: number;
                clarity: string;
                signalCount: number;
                dispatchCount: number;
                locationName: string | null;
              }) => (
                <tr key={s.id} className="border-b border-border/50 hover:bg-accent/30">
                  <td className="py-2 max-w-md">
                    <Link to={`/situations/${s.id}`} className="hover:underline line-clamp-1">
                      {s.headline}
                    </Link>
                    {s.locationName && (
                      <span className="text-xs text-muted-foreground ml-2">{s.locationName}</span>
                    )}
                  </td>
                  <td className="py-2">
                    <span className={`px-2 py-0.5 rounded-full text-xs ${ARC_COLORS[s.arc] ?? "bg-secondary"}`}>
                      {s.arc}
                    </span>
                  </td>
                  <td className="py-2 font-mono">{s.temperature.toFixed(2)}</td>
                  <td className="py-2 text-muted-foreground">{s.clarity}</td>
                  <td className="py-2">{s.signalCount}</td>
                  <td className="py-2">{s.dispatchCount}</td>
                </tr>
              ),
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
