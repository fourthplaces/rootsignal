import { useMemo, useState } from "react";
import { useParams } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import { SIGNAL_DETAIL } from "@/graphql/queries";
import { RE_EXTRACT_SIGNAL } from "@/graphql/mutations";
import { LinkPreview } from "@/components/LinkPreview";
import { ReviewStatusBadge } from "@/components/ReviewStatusBadge";

type ReExtractedSignal = {
  signalType: string;
  title: string;
  summary: string;
  sensitivity: string;
  latitude: number | null;
  longitude: number | null;
  locationName: string | null;
  startsAt: string | null;
  endsAt: string | null;
  organizer: string | null;
  urgency: string | null;
  severity: string | null;
  category: string | null;
  contentDate: string | null;
  tags: string[];
  isFirsthand: boolean | null;
  whatWouldHelp: string | null;
};

type ReExtractedRejection = {
  title: string;
  sourceUrl: string;
  reason: string;
};

type ReExtractResult = {
  sourceUrl: string;
  signals: ReExtractedSignal[];
  rejected: ReExtractedRejection[];
};

export function SignalDetailPage() {
  const { id } = useParams<{ id: string }>();
  const scheduleVars = useMemo(() => ({
    scheduleFrom: new Date().toISOString(),
    scheduleTo: new Date(Date.now() + 90 * 86400000).toISOString(),
  }), []);
  const { data, loading } = useQuery(SIGNAL_DETAIL, {
    variables: { id, ...scheduleVars },
  });

  const [reExtract] = useMutation(RE_EXTRACT_SIGNAL);
  const [extracting, setExtracting] = useState(false);
  const [extractError, setExtractError] = useState<string | null>(null);
  const [extractResult, setExtractResult] = useState<ReExtractResult | null>(null);

  if (loading) return <p className="text-muted-foreground">Loading...</p>;

  const signal = data?.signal;
  if (!signal) return <p className="text-muted-foreground">Signal not found</p>;

  const typeName = (signal.__typename as string).replace("Gql", "").replace("Signal", "");

  const handleReExtract = async () => {
    setExtracting(true);
    setExtractError(null);
    setExtractResult(null);
    try {
      const { data } = await reExtract({ variables: { signalId: id } });
      setExtractResult(data.reExtractSignal);
    } catch (e: unknown) {
      setExtractError(e instanceof Error ? e.message : "Re-extraction failed");
    } finally {
      setExtracting(false);
    }
  };

  return (
    <div className="space-y-6 max-w-3xl">
      <div>
        <div className="flex items-center justify-between mb-1">
          <p className="text-sm text-muted-foreground flex items-center gap-2 flex-wrap">
            <span className="px-2 py-0.5 rounded-full bg-secondary">{typeName}</span>
            <ReviewStatusBadge status={signal.reviewStatus ?? "live"} wasCorrected={signal.wasCorrected} />
            <span>&middot; {(signal.confidence * 100).toFixed(0)}% confidence</span>
            <span>&middot; {new Date(signal.contentDate ?? signal.extractedAt).toLocaleDateString()}</span>
          </p>
          <button
            onClick={handleReExtract}
            disabled={extracting}
            className="px-3 py-1 text-sm rounded-md bg-secondary hover:bg-secondary/80 disabled:opacity-50"
          >
            {extracting ? "Extracting..." : "Re-Extract"}
          </button>
        </div>
        <h1 className="text-xl font-semibold">{signal.title}</h1>
        <p className="mt-2 text-muted-foreground">{signal.summary}</p>
        {signal.sourceUrl && (
          <div className="mt-3">
            <LinkPreview url={signal.sourceUrl} fallbackLabel="Source" />
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

      {extractError && (
        <div className="rounded-lg border border-red-500/30 bg-red-500/10 p-4 text-sm text-red-400">
          {extractError}
        </div>
      )}

      {extractResult && (
        <div className="rounded-lg border border-border p-4 space-y-4">
          <h2 className="text-sm font-medium">
            Re-Extraction Results
            <span className="ml-2 text-muted-foreground font-normal">
              {extractResult.signals.length} signal{extractResult.signals.length !== 1 && "s"}
              {extractResult.rejected.length > 0 && (
                <>, {extractResult.rejected.length} rejected</>
              )}
            </span>
          </h2>
          <p className="text-xs text-muted-foreground break-all">{extractResult.sourceUrl}</p>

          {extractResult.signals.map((s, i) => (
            <div key={i} className="rounded-md border border-border p-3 space-y-2">
              <div className="flex items-center gap-2">
                <span className="px-2 py-0.5 rounded-full bg-secondary text-xs">
                  {s.signalType}
                </span>
                <span className="font-medium text-sm">{s.title}</span>
              </div>
              <p className="text-sm text-muted-foreground">{s.summary}</p>
              <div className="flex flex-wrap gap-1">
                {s.tags.map((tag) => (
                  <span key={tag} className="px-1.5 py-0.5 rounded bg-secondary/60 text-xs">
                    {tag}
                  </span>
                ))}
              </div>
              <div className="text-xs text-muted-foreground space-y-0.5">
                {s.sensitivity !== "general" && <p>Sensitivity: {s.sensitivity}</p>}
                {s.locationName && <p>Location: {s.locationName}</p>}
                {s.startsAt && <p>Starts: {s.startsAt}</p>}
                {s.organizer && <p>Organizer: {s.organizer}</p>}
                {s.urgency && <p>Urgency: {s.urgency}</p>}
                {s.severity && <p>Severity: {s.severity}</p>}
                {s.category && <p>Category: {s.category}</p>}
                {s.whatWouldHelp && <p>What would help: {s.whatWouldHelp}</p>}
                {s.isFirsthand != null && <p>First-hand: {s.isFirsthand ? "yes" : "no"}</p>}
              </div>
            </div>
          ))}

          {extractResult.rejected.map((r, i) => (
            <div key={i} className="rounded-md border border-border p-3 opacity-60">
              <span className="line-through text-sm">{r.title}</span>
              <span className="ml-2 text-xs text-muted-foreground">{r.reason}</span>
            </div>
          ))}
        </div>
      )}

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
              <span key={a.id} className="px-2 py-1 rounded-md bg-secondary text-sm">
                {a.name}
                <span className="ml-1 text-muted-foreground text-xs">{a.actorType}</span>
              </span>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
