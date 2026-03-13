import { useQuery } from "@apollo/client";
import { SIGNAL_DETAIL } from "@/graphql/queries";
import { LinkPreview } from "@/components/LinkPreview";

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
            <LinkPreview url={signal.sourceUrl} fallbackLabel="Source" />
          )}

          {signal.citations?.length > 0 && (
            <details className="group">
              <summary className="flex cursor-pointer items-center gap-1.5 text-sm font-medium text-foreground select-none list-none [&::-webkit-details-marker]:hidden">
                <span className="transition-transform group-open:rotate-90">&#9656;</span>
                Citations
                <span className="text-xs font-normal text-muted-foreground">
                  ({signal.citations.length})
                </span>
              </summary>
              <div className="mt-2 space-y-2">
                {signal.citations.map((ev: Record<string, string>, i: number) => (
                  <div key={i} className="rounded border border-border p-2 text-xs">
                    {ev.snippet && (
                      <p className="text-muted-foreground mb-1">{ev.snippet}</p>
                    )}
                    {ev.sourceUrl && <LinkPreview url={ev.sourceUrl} />}
                  </div>
                ))}
              </div>
            </details>
          )}
        </div>
      )}
    </div>
  );
}
