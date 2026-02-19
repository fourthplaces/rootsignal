import { useState } from "react";
import { useQuery } from "@apollo/client";
import { EDITIONS, ADMIN_CITIES } from "@/graphql/queries";

export function EditionsPage() {
  const { data: citiesData } = useQuery(ADMIN_CITIES);
  const cities = citiesData?.adminCities ?? [];
  const [city, setCity] = useState("twincities");

  const { data, loading } = useQuery(EDITIONS, {
    variables: { city, limit: 20 },
  });

  const editions = data?.editions ?? [];

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Editions</h1>
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
        <p className="text-muted-foreground">Loading editions...</p>
      ) : (
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
          {editions.map(
            (ed: {
              id: string;
              title: string;
              publishedAt: string;
              signalCount: number;
            }) => (
              <div key={ed.id} className="rounded-lg border border-border p-4">
                <h2 className="font-medium">{ed.title}</h2>
                <p className="text-sm text-muted-foreground mt-1">
                  {new Date(ed.publishedAt).toLocaleDateString()} &middot; {ed.signalCount} signals
                </p>
              </div>
            ),
          )}
        </div>
      )}
    </div>
  );
}
