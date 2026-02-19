import { useParams, Link } from "react-router";
import { useQuery } from "@apollo/client";
import { SIGNAL_DETAIL } from "@/graphql/queries";

export function SignalDetailPage() {
  const { id } = useParams<{ id: string }>();
  const { data, loading } = useQuery(SIGNAL_DETAIL, { variables: { id } });

  if (loading) return <p className="text-muted-foreground">Loading...</p>;

  const signal = data?.signal;
  if (!signal) return <p className="text-muted-foreground">Signal not found</p>;

  return (
    <div className="space-y-6 max-w-3xl">
      <div>
        <p className="text-sm text-muted-foreground mb-1">
          <span className="px-2 py-0.5 rounded-full bg-secondary">{signal.signalType}</span>
          {" "}&middot; {(signal.confidence * 100).toFixed(0)}% confidence
          {" "}&middot; {new Date(signal.createdAt).toLocaleDateString()}
        </p>
        <h1 className="text-xl font-semibold">{signal.title}</h1>
        <p className="mt-2 text-muted-foreground">{signal.summary}</p>
      </div>

      {signal.story && (
        <div className="rounded-lg border border-border p-4">
          <h2 className="text-sm font-medium mb-2">Story</h2>
          <Link to={`/stories/${signal.story.id}`} className="hover:underline">
            {signal.story.title}
          </Link>
          <span className="ml-2 text-xs text-muted-foreground">{signal.story.arc}</span>
        </div>
      )}

      {signal.evidence?.length > 0 && (
        <div className="rounded-lg border border-border p-4">
          <h2 className="text-sm font-medium mb-3">Evidence ({signal.evidence.length})</h2>
          <div className="space-y-3">
            {signal.evidence.map(
              (ev: { id: string; url: string; snippet: string; sourceType: string }) => (
                <div key={ev.id} className="text-sm">
                  <a
                    href={ev.url}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-blue-400 hover:underline break-all"
                  >
                    {ev.url}
                  </a>
                  <span className="ml-2 text-xs text-muted-foreground">{ev.sourceType}</span>
                  {ev.snippet && (
                    <p className="mt-1 text-muted-foreground">{ev.snippet}</p>
                  )}
                </div>
              ),
            )}
          </div>
        </div>
      )}

      {signal.actors?.length > 0 && (
        <div className="rounded-lg border border-border p-4">
          <h2 className="text-sm font-medium mb-3">Actors ({signal.actors.length})</h2>
          <div className="flex flex-wrap gap-2">
            {signal.actors.map((a: { id: string; name: string; role: string }) => (
              <span key={a.id} className="px-2 py-1 rounded-md bg-secondary text-sm">
                {a.name}
                <span className="ml-1 text-muted-foreground text-xs">{a.role}</span>
              </span>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
