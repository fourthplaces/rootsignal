import { Link } from "react-router";
import { useQuery } from "@apollo/client";
import { SIGNALS_RECENT } from "@/graphql/queries";

// Extract common fields from the union type signal
function getSignalFields(s: Record<string, unknown>) {
  return {
    id: s.id as string,
    title: s.title as string,
    confidence: s.confidence as number,
    extractedAt: s.extractedAt as string,
    __typename: s.__typename as string,
  };
}

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
              <th className="pb-2 font-medium">Extracted</th>
            </tr>
          </thead>
          <tbody>
            {signals.map((raw: Record<string, unknown>) => {
              const s = getSignalFields(raw);
              const typeName = s.__typename.replace("Gql", "").replace("Signal", "");
              return (
                <tr key={s.id} className="border-b border-border/50 hover:bg-accent/30">
                  <td className="py-2">
                    <Link to={`/signals/${s.id}`} className="hover:underline">
                      {s.title}
                    </Link>
                  </td>
                  <td className="py-2">
                    <span className="px-2 py-0.5 rounded-full text-xs bg-secondary">
                      {typeName}
                    </span>
                  </td>
                  <td className="py-2">{(s.confidence * 100).toFixed(0)}%</td>
                  <td className="py-2 text-muted-foreground">
                    {new Date(s.extractedAt).toLocaleDateString()}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
