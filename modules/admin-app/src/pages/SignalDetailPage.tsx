import { useMemo } from "react";
import { Link, useParams } from "react-router";
import { useQuery } from "@apollo/client";
import { SIGNAL_DETAIL } from "@/graphql/queries";
import { LinkPreview } from "@/components/LinkPreview";
import { ReviewStatusBadge } from "@/components/ReviewStatusBadge";


export function SignalDetailPage() {
  const { id } = useParams<{ id: string }>();
  const scheduleVars = useMemo(() => ({
    scheduleFrom: new Date().toISOString(),
    scheduleTo: new Date(Date.now() + 90 * 86400000).toISOString(),
  }), []);
  const { data, loading } = useQuery(SIGNAL_DETAIL, {
    variables: { id, ...scheduleVars },
  });


  if (loading) return <p className="text-muted-foreground">Loading...</p>;

  const signal = data?.adminSignal;
  if (!signal) return <p className="text-muted-foreground">Signal not found</p>;

  const typeName = (signal.__typename as string).replace("Gql", "").replace("Signal", "");


  return (
    <div className="space-y-6 max-w-3xl">
      <div>
        {(signal.locationName || signal.location) && (
          <Link
            to={signal.location ? `/graph?lat=${signal.location.lat}&lng=${signal.location.lng}` : "/graph"}
            className="mb-3 flex items-center gap-2 text-sm text-muted-foreground hover:text-foreground transition-colors"
          >
            <svg className="w-4 h-4 shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M17.657 16.657L13.414 20.9a1.998 1.998 0 01-2.827 0l-4.244-4.243a8 8 0 1111.314 0z" />
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 11a3 3 0 11-6 0 3 3 0 016 0z" />
            </svg>
            <span>
              {signal.locationName ?? `${signal.location.lat.toFixed(4)}, ${signal.location.lng.toFixed(4)}`}
              {signal.locationName && signal.location && (
                <span className="ml-2 text-xs opacity-60">
                  {signal.location.lat.toFixed(4)}, {signal.location.lng.toFixed(4)}
                </span>
              )}
            </span>
          </Link>
        )}
        <div className="flex items-center justify-between mb-1">
          <p className="text-sm text-muted-foreground flex items-center gap-2 flex-wrap">
            <span className="px-2 py-0.5 rounded-full bg-secondary">{typeName}</span>
            <ReviewStatusBadge status={signal.reviewStatus ?? "accepted"} wasCorrected={signal.wasCorrected} />
            <span>&middot; {(signal.confidence * 100).toFixed(0)}% confidence</span>
            <span>&middot; {new Date(signal.contentDate ?? signal.extractedAt).toLocaleDateString()}</span>
          </p>
        </div>
        <h1 className="text-xl font-semibold">{signal.title}</h1>
        <p className="mt-2 text-muted-foreground">{signal.summary}</p>
        {signal.url && (
          <div className="mt-3">
            <LinkPreview url={signal.url} fallbackLabel="Source" />
          </div>
        )}
      </div>

      {signal.rejectionReason && (
        <div className="rounded-lg border border-red-500/30 bg-red-500/10 p-4 text-sm">
          <h2 className="font-medium text-red-400 mb-1">Rejection Reason</h2>
          <p className="text-red-300">{signal.rejectionReason}</p>
        </div>
      )}

      {signal.corrections && (() => {
        try {
          const corrections = JSON.parse(signal.corrections) as Array<{
            field: string;
            old_value: string;
            new_value: string;
            reason: string;
          }>;
          return (
            <div className="rounded-lg border border-blue-500/30 bg-blue-500/10 p-4 text-sm">
              <h2 className="font-medium text-blue-400 mb-2">Corrections Applied</h2>
              <div className="space-y-2">
                {corrections.map((c, i) => (
                  <div key={i} className="text-xs">
                    <span className="text-blue-300 font-medium">{c.field}</span>
                    <span className="text-muted-foreground mx-1">&mdash;</span>
                    <span className="text-red-400 line-through">{c.old_value}</span>
                    <span className="text-muted-foreground mx-1">&rarr;</span>
                    <span className="text-green-400">{c.new_value}</span>
                    {c.reason && (
                      <span className="text-muted-foreground ml-2">({c.reason})</span>
                    )}
                  </div>
                ))}
              </div>
            </div>
          );
        } catch {
          return null;
        }
      })()}

      {signal.schedule && (
        <div className="rounded-lg border border-border p-4">
          <h2 className="text-sm font-medium mb-3">Schedule</h2>
          {signal.schedule.scheduleText && (
            <p className="text-sm text-muted-foreground">{signal.schedule.scheduleText}</p>
          )}
          {signal.schedule.rrule && (
            <p className="mt-1 text-xs text-muted-foreground font-mono">{signal.schedule.rrule}</p>
          )}
          {signal.schedule.timezone && (
            <p className="mt-1 text-xs text-muted-foreground">Timezone: {signal.schedule.timezone}</p>
          )}
          {signal.schedule.occurrences?.length > 0 && (
            <div className="mt-3">
              <h3 className="text-xs font-medium text-muted-foreground mb-2">
                Upcoming ({signal.schedule.occurrences.length})
              </h3>
              <div className="flex flex-wrap gap-1.5">
                {signal.schedule.occurrences.slice(0, 12).map((date: string) => (
                  <span key={date} className="px-2 py-0.5 rounded bg-secondary text-xs">
                    {new Date(date).toLocaleDateString(undefined, {
                      weekday: "short",
                      month: "short",
                      day: "numeric",
                      hour: "numeric",
                      minute: "2-digit",
                    })}
                  </span>
                ))}
                {signal.schedule.occurrences.length > 12 && (
                  <span className="px-2 py-0.5 text-xs text-muted-foreground">
                    +{signal.schedule.occurrences.length - 12} more
                  </span>
                )}
              </div>
            </div>
          )}
        </div>
      )}

      {signal.citations?.length > 0 && (
        <div className="rounded-lg border border-border p-4">
          <h2 className="text-sm font-medium mb-3">Citations ({signal.citations.length})</h2>
          <div className="space-y-3">
            {signal.citations.map(
              (ev: { id: string; sourceUrl: string; snippet: string | null }) => (
                <div key={ev.id} className="space-y-1">
                  <LinkPreview url={ev.sourceUrl} />
                  {ev.snippet && (
                    <p className="text-sm text-muted-foreground">{ev.snippet}</p>
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
            {signal.actors.map((a: { id: string; name: string; actorType: string }) => (
              <Link key={a.id} to={`/actors/${a.id}`} className="px-2 py-1 rounded-md bg-secondary text-sm text-blue-400 hover:underline">
                {a.name}
                <span className="ml-1 text-muted-foreground text-xs">{a.actorType}</span>
              </Link>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
