import { Link } from "react-router";
import { useQuery } from "@apollo/client";
import { SIGNALS_WITHOUT_LOCATION } from "@/graphql/queries";

function getSignalFields(s: Record<string, unknown>) {
  return {
    id: s.id as string,
    title: s.title as string,
    confidence: s.confidence as number,
    extractedAt: s.extractedAt as string,
    contentDate: (s.contentDate as string) ?? null,
    sourceUrl: (s.sourceUrl as string) ?? null,
    locationName: (s.locationName as string) ?? null,
    __typename: s.__typename as string,
  };
}

const formatDate = (d: string) =>
  new Date(d).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });

const typeName = (tn: string) => tn.replace("Gql", "").replace("Signal", "");

const typeColor: Record<string, string> = {
  Gathering: "bg-blue-500/10 text-blue-400",
  Aid: "bg-green-500/10 text-green-400",
  Need: "bg-orange-500/10 text-orange-400",
  Notice: "bg-purple-500/10 text-purple-400",
  Tension: "bg-red-500/10 text-red-400",
};

export function DanglingSignalsPage() {
  const { data, loading } = useQuery(SIGNALS_WITHOUT_LOCATION, {
    variables: { limit: 500 },
  });

  const signals = data?.signalsWithoutLocation ?? [];

  if (loading) return <p className="text-muted-foreground">Loading...</p>;

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-xl font-semibold">Signals Without Location</h1>
        <p className="text-sm text-muted-foreground mt-1">
          {signals.length} signals have no lat/lng set
        </p>
      </div>

      {signals.length === 0 ? (
        <p className="text-muted-foreground">All signals have locations.</p>
      ) : (
        <div className="rounded-lg border border-border overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border bg-muted/50">
                <th className="text-left px-4 py-2 font-medium">Type</th>
                <th className="text-left px-4 py-2 font-medium">Title</th>
                <th className="text-left px-4 py-2 font-medium">Location Name</th>
                <th className="text-right px-4 py-2 font-medium">Confidence</th>
                <th className="text-left px-4 py-2 font-medium">Source</th>
                <th className="text-left px-4 py-2 font-medium">Extracted</th>
              </tr>
            </thead>
            <tbody>
              {signals.map((raw: Record<string, unknown>) => {
                const s = getSignalFields(raw);
                const tn = typeName(s.__typename);
                return (
                  <tr key={s.id} className="border-b border-border last:border-0 hover:bg-muted/30">
                    <td className="px-4 py-2">
                      <span className={`text-xs px-2 py-0.5 rounded-full ${typeColor[tn] ?? "bg-secondary"}`}>
                        {tn}
                      </span>
                    </td>
                    <td className="px-4 py-2 max-w-[300px]">
                      <Link to={`/signals/${s.id}`} className="text-blue-400 hover:underline">
                        {s.title}
                      </Link>
                    </td>
                    <td className="px-4 py-2 text-muted-foreground truncate max-w-[150px]">
                      {s.locationName ?? "—"}
                    </td>
                    <td className="px-4 py-2 text-right tabular-nums">
                      {(s.confidence * 100).toFixed(0)}%
                    </td>
                    <td className="px-4 py-2 text-muted-foreground truncate max-w-[200px]">
                      {s.sourceUrl ? (
                        <a href={s.sourceUrl} target="_blank" rel="noopener noreferrer" className="hover:underline">
                          {new URL(s.sourceUrl).hostname}
                        </a>
                      ) : "—"}
                    </td>
                    <td className="px-4 py-2 text-muted-foreground whitespace-nowrap">
                      {formatDate(s.extractedAt)}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
