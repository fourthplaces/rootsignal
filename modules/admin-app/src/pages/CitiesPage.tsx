import { useState } from "react";
import { Link } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import { ADMIN_CITIES } from "@/graphql/queries";
import { CREATE_CITY } from "@/graphql/mutations";

export function CitiesPage() {
  const { data, loading, refetch } = useQuery(ADMIN_CITIES);
  const [createCity] = useMutation(CREATE_CITY);
  const [showCreate, setShowCreate] = useState(false);
  const [location, setLocation] = useState("");
  const [creating, setCreating] = useState(false);

  const handleCreate = async (e: React.FormEvent) => {
    e.preventDefault();
    setCreating(true);
    try {
      await createCity({ variables: { location } });
      setLocation("");
      setShowCreate(false);
      refetch();
    } finally {
      setCreating(false);
    }
  };

  if (loading) return <p className="text-muted-foreground">Loading cities...</p>;

  const cities = data?.adminCities ?? [];

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Cities</h1>
        <button
          onClick={() => setShowCreate(!showCreate)}
          className="px-4 py-2 rounded-md bg-primary text-primary-foreground text-sm font-medium hover:bg-primary/90"
        >
          Add City
        </button>
      </div>

      {showCreate && (
        <form onSubmit={handleCreate} className="flex gap-2">
          <input
            type="text"
            value={location}
            onChange={(e) => setLocation(e.target.value)}
            placeholder="City, State (e.g. Austin, TX)"
            className="flex-1 px-3 py-2 rounded-md border border-input bg-background text-sm"
            required
          />
          <button
            type="submit"
            disabled={creating}
            className="px-4 py-2 rounded-md bg-primary text-primary-foreground text-sm font-medium hover:bg-primary/90 disabled:opacity-50"
          >
            {creating ? "Creating..." : "Create"}
          </button>
        </form>
      )}

      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
        {cities.map(
          (city: {
            slug: string;
            name: string;
            active: boolean;
            scoutRunning: boolean;
            sourcesDue: number;
            lastScoutCompletedAt: string | null;
          }) => (
            <Link
              key={city.slug}
              to={`/cities/${city.slug}`}
              className="block rounded-lg border border-border p-4 hover:border-foreground/20 transition-colors"
            >
              <div className="flex items-center justify-between mb-2">
                <h2 className="font-medium">{city.name}</h2>
                <span
                  className={`text-xs px-2 py-0.5 rounded-full ${
                    city.scoutRunning
                      ? "bg-green-900 text-green-300"
                      : "bg-secondary text-muted-foreground"
                  }`}
                >
                  {city.scoutRunning ? "Scout Running" : "Idle"}
                </span>
              </div>
              <p className="text-sm text-muted-foreground">
                {city.sourcesDue} sources due
                {city.lastScoutCompletedAt && (
                  <>
                    {" "}
                    &middot; Last scouted{" "}
                    {new Date(city.lastScoutCompletedAt).toLocaleDateString()}
                  </>
                )}
              </p>
            </Link>
          ),
        )}
      </div>
    </div>
  );
}
