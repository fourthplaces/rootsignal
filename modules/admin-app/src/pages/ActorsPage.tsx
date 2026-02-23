import { useQuery } from "@apollo/client";
import { ACTORS_IN_BOUNDS } from "@/graphql/queries";

// Default Twin Cities bounding box
const DEFAULT_BOUNDS = {
  minLat: 44.85,
  maxLat: 45.10,
  minLng: -93.45,
  maxLng: -93.15,
};

type Actor = {
  id: string;
  name: string;
  actorType: string;
  description: string | null;
  signalCount: number;
  locationName: string | null;
};

export function ActorsPage() {
  const { data, loading } = useQuery(ACTORS_IN_BOUNDS, {
    variables: { ...DEFAULT_BOUNDS, limit: 100 },
  });
  const actors: Actor[] = data?.actorsInBounds ?? [];

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-semibold">Actors</h1>

      {loading ? (
        <p className="text-muted-foreground">Loading actors...</p>
      ) : actors.length === 0 ? (
        <p className="text-muted-foreground">No actors found.</p>
      ) : (
        <div className="rounded-lg border border-border overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border bg-muted/50">
                <th className="text-left px-4 py-2 font-medium">Name</th>
                <th className="text-left px-4 py-2 font-medium">Type</th>
                <th className="text-left px-4 py-2 font-medium">Location</th>
                <th className="text-right px-4 py-2 font-medium">Signals</th>
              </tr>
            </thead>
            <tbody>
              {actors.map((a) => (
                <tr key={a.id} className="border-b border-border last:border-0 hover:bg-muted/30">
                  <td className="px-4 py-2">{a.name}</td>
                  <td className="px-4 py-2 text-muted-foreground">{a.actorType}</td>
                  <td className="px-4 py-2 text-muted-foreground">{a.locationName ?? "—"}</td>
                  <td className="px-4 py-2 text-right tabular-nums">{a.signalCount}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
