import { Link } from "react-router";
import { useQuery } from "@apollo/client";
import { STORIES } from "@/graphql/queries";

export function StoriesPage() {
  const { data, loading } = useQuery(STORIES, {
    variables: { limit: 50 },
  });

  if (loading) return <p className="text-muted-foreground">Loading stories...</p>;

  const stories = data?.stories ?? [];

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-semibold">Stories</h1>
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-border text-left text-muted-foreground">
              <th className="pb-2 font-medium">Title</th>
              <th className="pb-2 font-medium">Arc</th>
              <th className="pb-2 font-medium">Category</th>
              <th className="pb-2 font-medium">Energy</th>
              <th className="pb-2 font-medium">Signals</th>
            </tr>
          </thead>
          <tbody>
            {stories.map(
              (s: {
                id: string;
                title: string;
                arc: string;
                category: string;
                energy: number;
                signalCount: number;
              }) => (
                <tr key={s.id} className="border-b border-border/50 hover:bg-accent/30">
                  <td className="py-2">
                    <Link to={`/stories/${s.id}`} className="hover:underline">
                      {s.title}
                    </Link>
                  </td>
                  <td className="py-2">
                    <span className="px-2 py-0.5 rounded-full text-xs bg-secondary">{s.arc}</span>
                  </td>
                  <td className="py-2 text-muted-foreground">{s.category}</td>
                  <td className="py-2">{s.energy.toFixed(1)}</td>
                  <td className="py-2">{s.signalCount}</td>
                </tr>
              ),
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
