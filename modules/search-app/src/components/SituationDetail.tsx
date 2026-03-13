import { useQuery } from "@apollo/client";
import { SITUATION_DETAIL } from "@/graphql/queries";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { Components } from "react-markdown";

interface SituationDetailProps {
  situationId: string;
  onBack: () => void;
}

const ARC_COLORS: Record<string, string> = {
  EMERGING: "bg-blue-500/10 text-blue-400",
  DEVELOPING: "bg-green-500/10 text-green-400",
  ACTIVE: "bg-orange-500/10 text-orange-400",
  COLD: "bg-gray-500/10 text-gray-500",
};

const SEVERITY_COLORS: Record<string, string> = {
  CRITICAL: "bg-red-500/20 text-red-300",
  HIGH: "bg-orange-500/20 text-orange-300",
  MODERATE: "bg-amber-500/20 text-amber-300",
  LOW: "bg-green-500/20 text-green-300",
};

const URGENCY_COLORS: Record<string, string> = {
  CRITICAL: "bg-red-500/20 text-red-300",
  HIGH: "bg-orange-500/20 text-orange-300",
  MODERATE: "bg-amber-500/20 text-amber-300",
  LOW: "bg-green-500/20 text-green-300",
};

type Signal = {
  __typename: string;
  id: string;
  title: string;
  summary: string;
  locationName?: string | null;
  startsAt?: string | null;
  actionUrl?: string | null;
  organizer?: string | null;
  availability?: string | null;
  urgency?: string | null;
  whatNeeded?: string | null;
  severity?: string | null;
  subject?: string | null;
  opposing?: string | null;
  observedBy?: string | null;
  measurement?: string | null;
  affectedScope?: string | null;
  effectiveDate?: string | null;
  sourceAuthority?: string | null;
};

const briefingComponents: Components = {
  p: ({ children }) => <p className="mb-3 leading-relaxed text-sm text-foreground">{children}</p>,
  h1: ({ children }) => <h2 className="text-base font-semibold text-foreground mt-5 mb-2">{children}</h2>,
  h2: ({ children }) => <h3 className="text-sm font-semibold text-foreground mt-4 mb-1.5">{children}</h3>,
  ul: ({ children }) => <ul className="mb-3 ml-4 list-disc space-y-1 text-sm text-foreground">{children}</ul>,
  ol: ({ children }) => <ol className="mb-3 ml-4 list-decimal space-y-1 text-sm text-foreground">{children}</ol>,
  li: ({ children }) => <li className="leading-relaxed">{children}</li>,
  strong: ({ children }) => <strong className="font-semibold text-foreground">{children}</strong>,
  em: ({ children }) => <em className="italic text-muted-foreground">{children}</em>,
  a: ({ href, children }) => <a href={href} className="text-blue-400 underline" target="_blank" rel="noreferrer">{children}</a>,
  hr: () => <hr className="my-4 border-border" />,
};

function formatShortDate(d: string | null | undefined) {
  if (!d) return null;
  return new Date(d).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}

function actionableUrl(signal: Signal): string | null {
  const url = signal.actionUrl;
  if (!url || url.trim() === "") return null;
  return url;
}

function groupSignals(signals: Signal[]) {
  const gatherings: Signal[] = [];
  const helpRequests: Signal[] = [];
  const resources: Signal[] = [];
  const concerns: Signal[] = [];
  const conditions: Signal[] = [];
  const announcements: Signal[] = [];

  for (const s of signals) {
    switch (s.__typename) {
      case "GqlGatheringSignal": gatherings.push(s); break;
      case "GqlHelpRequestSignal": helpRequests.push(s); break;
      case "GqlResourceSignal": resources.push(s); break;
      case "GqlConcernSignal": concerns.push(s); break;
      case "GqlConditionSignal": conditions.push(s); break;
      case "GqlAnnouncementSignal": announcements.push(s); break;
    }
  }

  return { gatherings, helpRequests, resources, concerns, conditions, announcements };
}

function CTACard({ signal, verb }: { signal: Signal; verb: string }) {
  const url = actionableUrl(signal);
  const inner = (
    <div className="rounded border border-border p-3 hover:border-blue-500/40 transition-colors">
      <div className="flex items-start justify-between gap-2 mb-1">
        <p className="text-sm font-medium text-foreground">
          {verb} {signal.title}
        </p>
        {signal.urgency && (
          <span className={`shrink-0 px-1.5 py-0.5 rounded text-[10px] ${URGENCY_COLORS[signal.urgency] ?? "bg-muted text-muted-foreground"}`}>
            {signal.urgency}
          </span>
        )}
      </div>
      {signal.summary && (
        <p className="text-xs text-muted-foreground line-clamp-2">{signal.summary}</p>
      )}
      <div className="flex items-center gap-2 mt-1 text-[11px] text-muted-foreground">
        {signal.startsAt && <span>{formatShortDate(signal.startsAt)}</span>}
        {signal.organizer && <span>{signal.organizer}</span>}
        {signal.whatNeeded && <span className="line-clamp-1">{signal.whatNeeded}</span>}
        {signal.availability && <span>{signal.availability}</span>}
        {signal.locationName && <span>{signal.locationName}</span>}
      </div>
    </div>
  );

  if (url) {
    return <a href={url} target="_blank" rel="noreferrer" className="block">{inner}</a>;
  }
  return inner;
}

function ContextCard({ signal }: { signal: Signal }) {
  return (
    <div className="rounded border border-border p-3">
      <div className="flex items-start justify-between gap-2 mb-1">
        <p className="text-sm font-medium text-foreground">{signal.title}</p>
        {signal.severity && (
          <span className={`shrink-0 px-1.5 py-0.5 rounded text-[10px] ${SEVERITY_COLORS[signal.severity] ?? "bg-muted text-muted-foreground"}`}>
            {signal.severity}
          </span>
        )}
      </div>
      {signal.summary && (
        <p className="text-xs text-muted-foreground line-clamp-3">{signal.summary}</p>
      )}
      <div className="flex items-center gap-2 mt-1 text-[11px] text-muted-foreground">
        {signal.subject && <span className="text-foreground/70">{signal.subject}</span>}
        {signal.observedBy && <span>Observed by {signal.observedBy}</span>}
        {signal.sourceAuthority && <span>{signal.sourceAuthority}</span>}
        {signal.effectiveDate && <span>{formatShortDate(signal.effectiveDate)}</span>}
        {signal.measurement && <span>{signal.measurement}</span>}
      </div>
    </div>
  );
}

export function SituationDetail({ situationId, onBack }: SituationDetailProps) {
  const { data, loading } = useQuery(SITUATION_DETAIL, {
    variables: { id: situationId },
  });

  const situation = data?.situation;

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

      {situation && (() => {
        const signals: Signal[] = situation.signals ?? [];
        const { gatherings, helpRequests, resources, concerns, conditions, announcements } =
          groupSignals(signals);
        const hasActionables = gatherings.length > 0 || helpRequests.length > 0 || resources.length > 0;
        const hasContext = concerns.length > 0 || conditions.length > 0 || announcements.length > 0;

        return (
          <div className="flex-1 overflow-y-auto p-4 space-y-5">
            {/* Header */}
            <div>
              <div className="flex items-center gap-2 mb-2">
                {situation.arc && situation.arc !== "COOLING" && (
                  <span className={`rounded px-2 py-0.5 text-xs font-medium ${ARC_COLORS[situation.arc] ?? "bg-muted text-muted-foreground"}`}>
                    {situation.arc}
                  </span>
                )}
                {situation.category && (
                  <span className="rounded px-2 py-0.5 text-xs bg-muted text-muted-foreground">
                    {situation.category}
                  </span>
                )}
              </div>
              <h2 className="text-lg font-semibold text-foreground">
                {situation.headline}
              </h2>
              {situation.locationName && (
                <p className="text-sm text-muted-foreground mt-1">{situation.locationName}</p>
              )}
            </div>

            {/* Briefing narrative */}
            {situation.briefingBody ? (
              <div className="rounded border border-border p-4">
                <ReactMarkdown remarkPlugins={[remarkGfm]} components={briefingComponents}>
                  {situation.briefingBody}
                </ReactMarkdown>
              </div>
            ) : situation.lede ? (
              <p className="text-sm text-foreground/90 italic">{situation.lede}</p>
            ) : null}

            {/* CTAs */}
            {hasActionables && (
              <div className="space-y-3">
                <h3 className="text-sm font-semibold text-foreground">What You Can Do</h3>
                {helpRequests.length > 0 && (
                  <div className="space-y-2">
                    <p className="text-xs text-muted-foreground">Help Needed</p>
                    {helpRequests.map((s) => <CTACard key={s.id} signal={s} verb="Help with" />)}
                  </div>
                )}
                {gatherings.length > 0 && (
                  <div className="space-y-2">
                    <p className="text-xs text-muted-foreground">Join a Gathering</p>
                    {gatherings.map((s) => <CTACard key={s.id} signal={s} verb="Join" />)}
                  </div>
                )}
                {resources.length > 0 && (
                  <div className="space-y-2">
                    <p className="text-xs text-muted-foreground">Resources Available</p>
                    {resources.map((s) => <CTACard key={s.id} signal={s} verb="" />)}
                  </div>
                )}
              </div>
            )}

            {/* Context */}
            {hasContext && (
              <div className="space-y-3">
                <h3 className="text-sm font-semibold text-foreground">Context</h3>
                {concerns.map((s) => <ContextCard key={s.id} signal={s} />)}
                {conditions.map((s) => <ContextCard key={s.id} signal={s} />)}
                {announcements.map((s) => <ContextCard key={s.id} signal={s} />)}
              </div>
            )}

            {/* Stats */}
            <div className="grid grid-cols-2 gap-3 text-xs">
              <div className="rounded border border-border p-2">
                <p className="text-muted-foreground">Temperature</p>
                <p className="text-lg font-semibold">{situation.temperature?.toFixed(2)}</p>
              </div>
              <div className="rounded border border-border p-2">
                <p className="text-muted-foreground">Signals</p>
                <p className="text-lg font-semibold">{situation.signalCount}</p>
              </div>
            </div>

            {/* Dispatches */}
            {situation.dispatches?.length > 0 && (
              <div>
                <h3 className="text-sm font-medium text-foreground mb-2">
                  Dispatches ({situation.dispatches.length})
                </h3>
                <div className="space-y-3">
                  {situation.dispatches.map((dispatch: Record<string, unknown>) => (
                    <div key={dispatch.id as string} className="rounded border border-border p-3 text-sm">
                      <div className="flex items-center gap-2 mb-1">
                        <span className="text-xs font-medium text-primary">
                          {dispatch.dispatchType as string}
                        </span>
                        <span className="text-xs text-muted-foreground">
                          {new Date(dispatch.createdAt as string).toLocaleDateString()}
                        </span>
                        {!!dispatch.flaggedForReview && (
                          <span className="text-xs text-red-400">Flagged</span>
                        )}
                      </div>
                      <p className="text-muted-foreground whitespace-pre-wrap">
                        {dispatch.body as string}
                      </p>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>
        );
      })()}
    </div>
  );
}
