import { useQuery } from "@apollo/client";
import { SIGNAL_DETAIL } from "@/graphql/queries";

interface SignalDetailProps {
  signalId: string;
  onBack: () => void;
}

export function SignalDetail({ signalId, onBack }: SignalDetailProps) {
  const { data, loading } = useQuery(SIGNAL_DETAIL, {
    variables: { id: signalId },
  });

  const signal = data?.signal;

  return (
    <div className="flex flex-col h-full">
      <button
        onClick={onBack}
        className="flex items-center gap-1 px-4 py-2 text-sm text-muted-foreground hover:text-foreground border-b border-border"
      >
        &larr; Back to results
      </button>

      {loading && (
        <div className="flex items-center justify-center p-8">
          <div className="h-6 w-6 animate-spin rounded-full border-2 border-muted-foreground border-t-primary" />
        </div>
      )}

      {signal && (
        <div className="flex-1 overflow-y-auto p-4 space-y-4">
          <div>
            <span className="inline-block rounded px-2 py-0.5 text-xs font-medium bg-primary/10 text-primary mb-2">
              {(signal.__typename as string)?.replace("Gql", "").replace("Signal", "")}
            </span>
            <h2 className="text-lg font-semibold text-foreground">
              {signal.title}
            </h2>
          </div>

          <p className="text-sm text-muted-foreground">{signal.summary}</p>

          {signal.locationName && (
            <div className="text-xs text-muted-foreground">
              Location: {signal.locationName}
            </div>
          )}

          {signal.sourceUrl && (
            <a
              href={signal.sourceUrl}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-block text-xs text-primary hover:underline"
            >
              Source
            </a>
          )}

          {signal.story && (
            <div className="rounded-lg border border-border p-3">
              <p className="text-xs text-muted-foreground mb-1">Part of story</p>
              <p className="text-sm font-medium">{signal.story.headline}</p>
            </div>
          )}

          {signal.evidence?.length > 0 && (
            <div>
              <h3 className="text-sm font-medium text-foreground mb-2">Evidence</h3>
              <div className="space-y-2">
                {signal.evidence.map((ev: Record<string, string>, i: number) => (
                  <div key={i} className="rounded border border-border p-2 text-xs">
                    {ev.snippet && (
                      <p className="text-muted-foreground mb-1">{ev.snippet}</p>
                    )}
                    <a
                      href={ev.sourceUrl}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="text-primary hover:underline"
                    >
                      {ev.sourceUrl}
                    </a>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
