import { useState } from "react";
import { useQuery } from "@apollo/client";
import { ACTORS, ADMIN_CITIES } from "@/graphql/queries";

export function ActorsPage() {
  const { data: citiesData } = useQuery(ADMIN_CITIES);
  const cities = citiesData?.adminCities ?? [];
  const [city, setCity] = useState("twincities");

  const { data, loading } = useQuery(ACTORS, {
    variables: { city, limit: 100 },
  });

  const actors = data?.actors ?? [];

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Actors</h1>
        <select
          value={city}
          onChange={(e) => setCity(e.target.value)}
          className="px-3 py-1.5 rounded-md border border-input bg-background text-sm"
        >
          {cities.map((c: { slug: string; name: string }) => (
            <option key={c.slug} value={c.slug}>
              {c.name}
            </option>
          ))}
        </select>
      </div>

      {loading ? (
        <p className="text-muted-foreground">Loading actors...</p>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border text-left text-muted-foreground">
                <th className="pb-2 font-medium">Name</th>
                <th className="pb-2 font-medium">Type</th>
                <th className="pb-2 font-medium">Signals</th>
              </tr>
            </thead>
            <tbody>
              {actors.map((a: { id: string; name: string; actorType: string; description: string | null; signalCount: number }) => (
                <tr key={a.id} className="border-b border-border/50">
                  <td className="py-2">{a.name}</td>
                  <td className="py-2 text-muted-foreground">{a.actorType}</td>
                  <td className="py-2">{a.signalCount}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
