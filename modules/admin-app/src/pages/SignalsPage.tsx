import { Link } from "react-router";
import { useQuery } from "@apollo/client";
import { SIGNALS_RECENT } from "@/graphql/queries";

export function SignalsPage() {
  const { data, loading } = useQuery(SIGNALS_RECENT, {
    variables: { limit: 100 },
  });

  if (loading) return <p className="text-muted-foreground">Loading signals...</p>;

  const signals = data?.signalsRecent ?? [];

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-semibold">Signals</h1>
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-border text-left text-muted-foreground">
              <th className="pb-2 font-medium">Title</th>
              <th className="pb-2 font-medium">Type</th>
              <th className="pb-2 font-medium">Confidence</th>
              <th className="pb-2 font-medium">City</th>
              <th className="pb-2 font-medium">Created</th>
            </tr>
          </thead>
          <tbody>
            {signals.map(
              (s: {
                id: string;
                title: string;
                signalType: string;
                confidence: number;
                city: string;
                createdAt: string;
              }) => (
                <tr key={s.id} className="border-b border-border/50 hover:bg-accent/30">
                  <td className="py-2">
                    <Link to={`/signals/${s.id}`} className="hover:underline">
                      {s.title}
                    </Link>
                  </td>
                  <td className="py-2">
                    <span className="px-2 py-0.5 rounded-full text-xs bg-secondary">
                      {s.signalType}
                    </span>
                  </td>
                  <td className="py-2">{(s.confidence * 100).toFixed(0)}%</td>
                  <td className="py-2">{s.city}</td>
                  <td className="py-2 text-muted-foreground">
                    {new Date(s.createdAt).toLocaleDateString()}
                  </td>
                </tr>
              ),
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
