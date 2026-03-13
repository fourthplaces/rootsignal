import { Link, useParams } from "react-router";
import { useQuery } from "@apollo/client";
import { SITUATION_DETAIL } from "@/graphql/queries";
import { DataTable, type Column } from "@/components/DataTable";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { Components } from "react-markdown";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const ARC_COLORS: Record<string, string> = {
  EMERGING: "bg-blue-500/20 text-blue-300",
  DEVELOPING: "bg-green-500/20 text-green-300",
  ACTIVE: "bg-orange-500/20 text-orange-300",
  COOLING: "bg-gray-500/20 text-gray-300",
  COLD: "bg-gray-500/20 text-gray-500",
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

const DISPATCH_TYPE_COLORS: Record<string, string> = {
  EMERGENCE: "bg-blue-500/10 text-blue-400 border-blue-500/20",
  UPDATE: "bg-green-500/10 text-green-400 border-green-500/20",
  CORRECTION: "bg-amber-500/10 text-amber-400 border-amber-500/20",
  SPLIT: "bg-purple-500/10 text-purple-400 border-purple-500/20",
  MERGE: "bg-indigo-500/10 text-indigo-400 border-indigo-500/20",
  REACTIVATION: "bg-orange-500/10 text-orange-400 border-orange-500/20",
};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

type Signal = {
  __typename: string;
  id: string;
  title: string;
  summary: string;
  locationName?: string | null;
  url?: string | null;
  causeHeat?: number | null;
  // Gathering
  startsAt?: string | null;
  endsAt?: string | null;
  actionUrl?: string | null;
  organizer?: string | null;
  isRecurring?: boolean | null;
  // Resource
  availability?: string | null;
  isOngoing?: boolean | null;
  // HelpRequest
  urgency?: string | null;
  whatNeeded?: string | null;
  statedGoal?: string | null;
  // Concern / Condition / Announcement
  severity?: string | null;
  subject?: string | null;
  opposing?: string | null;
  observedBy?: string | null;
  measurement?: string | null;
  affectedScope?: string | null;
  effectiveDate?: string | null;
  sourceAuthority?: string | null;
};

type Dispatch = {
  id: string;
  body: string;
  signalIds: string[];
  createdAt: string;
  dispatchType: string;
  supersedes: string | null;
  flaggedForReview: boolean;
  flagReason: string | null;
  fidelityScore: number | null;
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const formatDate = (d: string | null | undefined) => {
  if (!d) return "—";
  return new Date(d).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
};

const formatShortDate = (d: string | null | undefined) => {
  if (!d) return null;
  return new Date(d).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
};

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

// ---------------------------------------------------------------------------
// Markdown overrides for the briefing narrative
// ---------------------------------------------------------------------------

const briefingComponents: Components = {
  p: ({ children }) => <p className="mb-4 leading-relaxed text-sm text-foreground">{children}</p>,
  h1: ({ children }) => <h2 className="text-base font-semibold text-foreground mt-6 mb-2">{children}</h2>,
  h2: ({ children }) => <h3 className="text-sm font-semibold text-foreground mt-5 mb-2">{children}</h3>,
  h3: ({ children }) => <h4 className="text-sm font-medium text-foreground mt-4 mb-1.5">{children}</h4>,
  ul: ({ children }) => <ul className="mb-4 ml-4 list-disc space-y-1 text-sm text-foreground">{children}</ul>,
  ol: ({ children }) => <ol className="mb-4 ml-4 list-decimal space-y-1 text-sm text-foreground">{children}</ol>,
  li: ({ children }) => <li className="leading-relaxed">{children}</li>,
  strong: ({ children }) => <strong className="font-semibold text-foreground">{children}</strong>,
  em: ({ children }) => <em className="italic text-muted-foreground">{children}</em>,
  a: ({ href, children }) => <a href={href} className="text-blue-400 underline" target="_blank" rel="noreferrer">{children}</a>,
  blockquote: ({ children }) => <blockquote className="border-l-2 border-border pl-3 my-3 text-muted-foreground italic text-sm">{children}</blockquote>,
  hr: () => <hr className="my-5 border-border" />,
};

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

function TempBar({ label, value, max = 1 }: { label: string; value: number; max?: number }) {
  const pct = Math.min((value / max) * 100, 100);
  return (
    <div className="space-y-1">
      <div className="flex justify-between text-xs">
        <span className="text-muted-foreground">{label}</span>
        <span className="tabular-nums font-mono">{value.toFixed(2)}</span>
      </div>
      <div className="h-1.5 rounded-full bg-secondary overflow-hidden">
        <div
          className="h-full rounded-full bg-blue-500/60"
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  );
}

function CTACard({ signal, verb }: { signal: Signal; verb: string }) {
  const url = actionableUrl(signal);
  const inner = (
    <div className="rounded-lg border border-border p-4 hover:border-blue-500/40 transition-colors">
      <div className="flex items-start justify-between gap-2 mb-1">
        <p className="text-sm font-medium text-foreground">
          {verb} {signal.title}
        </p>
        {signal.urgency && (
          <span className={`shrink-0 px-2 py-0.5 rounded-full text-[10px] ${URGENCY_COLORS[signal.urgency] ?? "bg-secondary text-muted-foreground"}`}>
            {signal.urgency}
          </span>
        )}
      </div>
      {signal.summary && (
        <p className="text-xs text-muted-foreground line-clamp-2 mb-2">{signal.summary}</p>
      )}
      <div className="flex items-center gap-3 text-[11px] text-muted-foreground">
        {signal.startsAt && <span>{formatShortDate(signal.startsAt)}</span>}
        {signal.organizer && <span>{signal.organizer}</span>}
        {signal.whatNeeded && <span className="line-clamp-1">{signal.whatNeeded}</span>}
        {signal.availability && <span>{signal.availability}</span>}
        {signal.locationName && <span>{signal.locationName}</span>}
      </div>
    </div>
  );

  if (url) {
    return (
      <a href={url} target="_blank" rel="noreferrer" className="block">
        {inner}
      </a>
    );
  }

  return inner;
}

function CTASection({ title, signals, verb }: { title: string; signals: Signal[]; verb: string }) {
  if (signals.length === 0) return null;
  return (
    <div>
      <h3 className="text-sm font-medium text-muted-foreground mb-2">{title}</h3>
      <div className="grid gap-2">
        {signals.map((s) => (
          <CTACard key={s.id} signal={s} verb={verb} />
        ))}
      </div>
    </div>
  );
}

function ContextCard({ signal }: { signal: Signal }) {
  return (
    <div className="rounded-lg border border-border p-3">
      <div className="flex items-start justify-between gap-2 mb-1">
        <p className="text-sm font-medium text-foreground">{signal.title}</p>
        {signal.severity && (
          <span className={`shrink-0 px-2 py-0.5 rounded-full text-[10px] ${SEVERITY_COLORS[signal.severity] ?? "bg-secondary text-muted-foreground"}`}>
            {signal.severity}
          </span>
        )}
      </div>
      {signal.summary && (
        <p className="text-xs text-muted-foreground line-clamp-3">{signal.summary}</p>
      )}
      <div className="flex items-center gap-3 mt-1.5 text-[11px] text-muted-foreground">
        {signal.subject && <span className="text-foreground/70">{signal.subject}</span>}
        {signal.observedBy && <span>Observed by {signal.observedBy}</span>}
        {signal.sourceAuthority && <span>{signal.sourceAuthority}</span>}
        {signal.effectiveDate && <span>{formatShortDate(signal.effectiveDate)}</span>}
        {signal.measurement && <span>{signal.measurement}</span>}
      </div>
    </div>
  );
}

function ContextSection({ title, signals }: { title: string; signals: Signal[] }) {
  if (signals.length === 0) return null;
  return (
    <div>
      <h3 className="text-sm font-medium text-muted-foreground mb-2">{title}</h3>
      <div className="grid gap-2">
        {signals.map((s) => (
          <ContextCard key={s.id} signal={s} />
        ))}
      </div>
    </div>
  );
}

const dispatchColumns: Column<Dispatch>[] = [
  {
    key: "dispatchType",
    label: "Type",
    render: (d) => (
      <span
        className={`px-2 py-0.5 rounded-full text-xs border ${DISPATCH_TYPE_COLORS[d.dispatchType] ?? "bg-muted text-muted-foreground border-border"}`}
      >
        {d.dispatchType}
      </span>
    ),
  },
  {
    key: "body",
    label: "Body",
    render: (d) => (
      <span className="line-clamp-2 text-sm">{d.body}</span>
    ),
  },
  {
    key: "signalIds",
    label: "Signals",
    align: "right",
    render: (d) => <span className="tabular-nums">{d.signalIds.length}</span>,
  },
  {
    key: "fidelityScore",
    label: "Fidelity",
    align: "right",
    render: (d) =>
      d.fidelityScore != null ? (
        <span className="tabular-nums">{(d.fidelityScore * 100).toFixed(0)}%</span>
      ) : (
        <span className="text-muted-foreground">—</span>
      ),
  },
  {
    key: "flaggedForReview",
    label: "Flag",
    render: (d) =>
      d.flaggedForReview ? (
        <span className="text-amber-400 text-xs" title={d.flagReason ?? undefined}>
          Flagged
        </span>
      ) : null,
  },
  {
    key: "createdAt",
    label: "Created",
    render: (d) => (
      <span className="text-muted-foreground whitespace-nowrap text-xs">
        {formatDate(d.createdAt)}
      </span>
    ),
  },
];

// ---------------------------------------------------------------------------
// Main page
// ---------------------------------------------------------------------------

export function SituationDetailPage() {
  const { id } = useParams<{ id: string }>();
  const { data, loading } = useQuery(SITUATION_DETAIL, {
    variables: { id },
  });

  if (loading) return <p className="text-muted-foreground">Loading...</p>;

  const s = data?.situation;
  if (!s) return <p className="text-muted-foreground">Situation not found</p>;

  const signals: Signal[] = s.signals ?? [];
  const { gatherings, helpRequests, resources, concerns, conditions, announcements } =
    groupSignals(signals);

  const hasActionables = gatherings.length > 0 || helpRequests.length > 0 || resources.length > 0;
  const hasContext = concerns.length > 0 || conditions.length > 0 || announcements.length > 0;

  return (
    <div className="space-y-8 max-w-3xl">
      {/* Breadcrumb */}
      <nav className="text-sm text-muted-foreground">
        <Link to="/data?tab=situations" className="hover:text-foreground">
          Situations
        </Link>
        <span className="mx-2">/</span>
        <span className="line-clamp-1">{s.headline}</span>
      </nav>

      {/* Hero */}
      <div>
        <div className="flex items-center gap-3 mb-2">
          <h1 className="text-xl font-semibold">{s.headline}</h1>
          <span
            className={`px-2 py-0.5 rounded-full text-xs ${ARC_COLORS[s.arc] ?? "bg-secondary"}`}
          >
            {s.arc}
          </span>
        </div>
        <p className="text-sm text-muted-foreground">{s.lede}</p>
        <div className="flex items-center gap-4 mt-2 text-xs text-muted-foreground">
          {s.locationName && <span>{s.locationName}</span>}
          {s.category && (
            <span className="px-2 py-0.5 rounded-full bg-secondary">{s.category}</span>
          )}
        </div>
      </div>

      {/* Briefing narrative */}
      {s.briefingBody ? (
        <div className="rounded-lg border border-border p-5">
          <ReactMarkdown remarkPlugins={[remarkGfm]} components={briefingComponents}>
            {s.briefingBody}
          </ReactMarkdown>
        </div>
      ) : (
        <div className="rounded-lg border border-dashed border-border p-5 text-sm text-muted-foreground">
          <p>{s.lede}</p>
          <p className="mt-3 text-xs italic">Full briefing not yet generated.</p>
        </div>
      )}

      {/* CTAs — actionable signals */}
      {hasActionables && (
        <div className="space-y-4">
          <h2 className="text-base font-semibold">What You Can Do</h2>
          <CTASection title="Help Needed" signals={helpRequests} verb="Help with" />
          <CTASection title="Join a Gathering" signals={gatherings} verb="Join" />
          <CTASection title="Resources Available" signals={resources} verb="" />
        </div>
      )}

      {/* Context — concerns, conditions, announcements */}
      {hasContext && (
        <div className="space-y-4">
          <h2 className="text-base font-semibold">Context</h2>
          <ContextSection title="Concerns" signals={concerns} />
          <ContextSection title="Conditions" signals={conditions} />
          <ContextSection title="Announcements" signals={announcements} />
        </div>
      )}

      {/* Dispatches */}
      <div>
        <h2 className="text-base font-semibold mb-3">
          Dispatches ({s.dispatches?.length ?? 0})
        </h2>
        <DataTable<Dispatch>
          columns={dispatchColumns}
          data={s.dispatches ?? []}
          getRowKey={(d) => d.id}
          emptyMessage="No dispatches yet."
        />
      </div>

      {/* Metadata footer */}
      <div className="rounded-lg border border-border p-4 space-y-4">
        <h2 className="text-sm font-medium">Situation Metadata</h2>
        <dl className="grid grid-cols-2 sm:grid-cols-4 gap-4 text-sm">
          <div>
            <dt className="text-xs text-muted-foreground">Temperature</dt>
            <dd className="font-mono tabular-nums">{s.temperature.toFixed(2)}</dd>
          </div>
          <div>
            <dt className="text-xs text-muted-foreground">Signals</dt>
            <dd className="font-mono tabular-nums">{s.signalCount}</dd>
          </div>
          <div>
            <dt className="text-xs text-muted-foreground">Dispatches</dt>
            <dd className="font-mono tabular-nums">{s.dispatchCount}</dd>
          </div>
          <div>
            <dt className="text-xs text-muted-foreground">Sensitivity</dt>
            <dd>{s.sensitivity}</dd>
          </div>
          <div>
            <dt className="text-xs text-muted-foreground">Clarity</dt>
            <dd>{s.clarity ?? "—"}</dd>
          </div>
          <div>
            <dt className="text-xs text-muted-foreground">First Seen</dt>
            <dd className="text-xs">{formatDate(s.firstSeen)}</dd>
          </div>
          <div>
            <dt className="text-xs text-muted-foreground">Last Updated</dt>
            <dd className="text-xs">{formatDate(s.lastUpdated)}</dd>
          </div>
        </dl>

        {/* Temperature components */}
        <div className="space-y-2 pt-2 border-t border-border">
          <h3 className="text-xs font-medium text-muted-foreground">Temperature Components</h3>
          <TempBar label="Tension Heat" value={s.tensionHeat} />
          <TempBar label="Entity Velocity" value={s.entityVelocity} />
          <TempBar label="Amplification" value={s.amplification} />
          <TempBar label="Response Coverage" value={s.responseCoverage} />
          <TempBar label="Clarity Need" value={s.clarityNeed} />
        </div>
      </div>
    </div>
  );
}
