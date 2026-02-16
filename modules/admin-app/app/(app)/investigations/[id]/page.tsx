import { headers } from "next/headers";
import { authedClient } from "@/lib/client";
import Link from "next/link";

interface InvestigationStep {
  id: string;
  stepNumber: number;
  toolName: string;
  input: Record<string, unknown>;
  output: Record<string, unknown>;
  pageSnapshotId: string | null;
  createdAt: string;
}

interface FindingSummary {
  id: string;
  title: string;
  status: string;
  validationStatus: string | null;
  connectionCount: number;
  evidence: { id: string; evidenceType: string; quote: string; attribution: string | null }[];
  connections: ConnectionSummary[];
}

interface ConnectionSummary {
  id: string;
  fromType: string;
  fromId: string;
  toType: string;
  toId: string;
  role: string;
  causalQuote: string | null;
  confidence: number | null;
}

interface Investigation {
  id: string;
  subjectType: string;
  subjectId: string;
  trigger: string;
  status: string;
  summary: string | null;
  summaryConfidence: number | null;
  startedAt: string | null;
  completedAt: string | null;
  createdAt: string;
  steps: InvestigationStep[];
  finding: FindingSummary | null;
  signal: { id: string; signalType: string; content: string } | null;
}

const STATUS_COLORS: Record<string, string> = {
  pending: "bg-yellow-100 text-yellow-800",
  running: "bg-blue-100 text-blue-800",
  completed: "bg-green-100 text-green-800",
  failed: "bg-red-100 text-red-800",
};

const TOOL_LABELS: Record<string, string> = {
  follow_link: "Follow Link",
  web_search: "Web Search",
  query_signals: "Query Signals",
  query_social: "Query Social",
  query_entities: "Query Entities",
  query_findings: "Query Findings",
  recommend_source: "Recommend Source",
};

const TOOL_COLORS: Record<string, string> = {
  follow_link: "bg-cyan-100 text-cyan-800",
  web_search: "bg-orange-100 text-orange-800",
  query_signals: "bg-green-100 text-green-800",
  query_social: "bg-purple-100 text-purple-800",
  query_entities: "bg-blue-100 text-blue-800",
  query_findings: "bg-yellow-100 text-yellow-800",
  recommend_source: "bg-pink-100 text-pink-800",
};

const ROLE_LABELS: Record<string, string> = {
  response_to: "Responds to",
  affected_by: "Affected by",
  evidence_of: "Evidence of",
  driven_by: "Driven by",
};

function formatDuration(startedAt: string | null, completedAt: string | null): string {
  if (!startedAt) return "-";
  const start = new Date(startedAt).getTime();
  const end = completedAt ? new Date(completedAt).getTime() : Date.now();
  const seconds = Math.round((end - start) / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = seconds % 60;
  return `${minutes}m ${remainingSeconds}s`;
}

function JsonPreview({ data }: { data: Record<string, unknown> }) {
  const text = JSON.stringify(data, null, 2);
  const truncated = text.length > 500 ? text.slice(0, 500) + "\n..." : text;
  return (
    <pre className="max-h-40 overflow-auto rounded bg-gray-50 p-2 text-xs text-gray-600">
      {truncated}
    </pre>
  );
}

export default async function InvestigationDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  const headerStore = await headers();
  const api = authedClient(headerStore.get("cookie") ?? undefined);

  const { investigation } = await api.query<{ investigation: Investigation }>(
    `query Investigation($id: ID!) {
      investigation(id: $id) {
        id subjectType subjectId trigger status summary
        summaryConfidence startedAt completedAt createdAt
        steps {
          id stepNumber toolName input output pageSnapshotId createdAt
        }
        finding {
          id title status validationStatus connectionCount
          evidence { id evidenceType quote attribution }
          connections { id fromType fromId toType toId role causalQuote confidence }
        }
        signal {
          id signalType content
        }
      }
    }`,
    { id },
  );

  return (
    <div className="mx-auto max-w-4xl">
      <div className="mb-4">
        <Link
          href="/investigations"
          className="text-sm text-blue-600 hover:underline"
        >
          &larr; Back to Investigations
        </Link>
      </div>

      <div className="rounded-lg border border-gray-200 bg-white p-6">
        {/* Header */}
        <div className="mb-4 flex items-center gap-3">
          <span
            className={`inline-block rounded-full px-3 py-1 text-sm font-medium ${
              STATUS_COLORS[investigation.status] || "bg-gray-100"
            }`}
          >
            {investigation.status}
          </span>
          {investigation.summaryConfidence != null && (
            <span className="text-sm text-gray-400">
              Confidence: {Math.round(investigation.summaryConfidence * 100)}%
            </span>
          )}
          {investigation.status === "running" && (
            <span className="inline-flex items-center gap-1.5 text-sm text-blue-600">
              <span className="inline-block h-2 w-2 animate-pulse rounded-full bg-blue-500" />
              In progress
            </span>
          )}
        </div>

        <h1 className="mb-2 text-xl font-bold">
          Investigation {investigation.id.slice(0, 8)}...
        </h1>

        {/* Metadata */}
        <div className="mb-6 rounded border border-gray-100 bg-gray-50 p-4">
          <dl className="grid grid-cols-2 gap-2 text-sm">
            <dt className="text-gray-500">Trigger</dt>
            <dd className="font-mono text-xs">{investigation.trigger}</dd>
            <dt className="text-gray-500">Signal</dt>
            <dd>
              {investigation.signal ? (
                <Link
                  href={`/signals/${investigation.subjectId}`}
                  className="text-blue-600 hover:underline"
                >
                  [{investigation.signal.signalType}]{" "}
                  {investigation.signal.content.length > 80
                    ? investigation.signal.content.slice(0, 80) + "..."
                    : investigation.signal.content}
                </Link>
              ) : (
                <Link
                  href={`/${investigation.subjectType}s/${investigation.subjectId}`}
                  className="text-blue-600 hover:underline"
                >
                  {investigation.subjectId.slice(0, 8)}...
                </Link>
              )}
            </dd>
            <dt className="text-gray-500">Duration</dt>
            <dd>{formatDuration(investigation.startedAt, investigation.completedAt)}</dd>
            <dt className="text-gray-500">Started</dt>
            <dd>
              {investigation.startedAt
                ? new Date(investigation.startedAt).toLocaleString()
                : "-"}
            </dd>
          </dl>
        </div>

        {/* Summary */}
        {investigation.summary && (
          <div className="mb-6">
            <h2 className="mb-2 text-lg font-semibold">Summary</h2>
            <p className="text-gray-700">{investigation.summary}</p>
          </div>
        )}

        {/* Tool Call Timeline */}
        {investigation.steps.length > 0 && (
          <div className="mb-6">
            <h2 className="mb-3 text-lg font-semibold">
              Tool Call Timeline ({investigation.steps.length} steps)
            </h2>
            <div className="space-y-3">
              {investigation.steps.map((step) => (
                <div
                  key={step.id}
                  className="rounded border border-gray-200 bg-white p-3"
                >
                  <div className="mb-2 flex items-center gap-2">
                    <span className="text-xs font-bold text-gray-400">
                      #{step.stepNumber}
                    </span>
                    <span
                      className={`inline-block rounded px-2 py-0.5 text-xs font-medium ${
                        TOOL_COLORS[step.toolName] || "bg-gray-100"
                      }`}
                    >
                      {TOOL_LABELS[step.toolName] || step.toolName}
                    </span>
                    {step.pageSnapshotId && (
                      <Link
                        href={`/snapshots/${step.pageSnapshotId}`}
                        className="text-xs text-blue-500 hover:underline"
                      >
                        View snapshot
                      </Link>
                    )}
                    <span className="ml-auto text-xs text-gray-400">
                      {new Date(step.createdAt).toLocaleTimeString()}
                    </span>
                  </div>
                  <div className="grid gap-2 md:grid-cols-2">
                    <div>
                      <span className="mb-1 block text-xs font-medium text-gray-400">
                        Input
                      </span>
                      <JsonPreview data={step.input} />
                    </div>
                    <div>
                      <span className="mb-1 block text-xs font-medium text-gray-400">
                        Output
                      </span>
                      <JsonPreview data={step.output} />
                    </div>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Resulting Finding */}
        {investigation.finding && (
          <div className="mb-6">
            <h2 className="mb-3 text-lg font-semibold">Resulting Finding</h2>
            <div className="rounded border border-gray-200 bg-white p-4">
              <div className="mb-2 flex items-center gap-2">
                <span
                  className={`inline-block rounded-full px-2 py-0.5 text-xs font-medium ${
                    investigation.finding.status === "emerging"
                      ? "bg-yellow-100 text-yellow-800"
                      : investigation.finding.status === "active"
                        ? "bg-red-100 text-red-800"
                        : "bg-green-100 text-green-800"
                  }`}
                >
                  {investigation.finding.status}
                </span>
                {investigation.finding.validationStatus && (
                  <span className="text-xs text-gray-400">
                    {investigation.finding.validationStatus}
                  </span>
                )}
                <span className="text-xs text-gray-400">
                  {investigation.finding.connectionCount} connections
                </span>
              </div>
              <Link
                href={`/findings/${investigation.finding.id}`}
                className="text-blue-600 hover:underline"
              >
                {investigation.finding.title}
              </Link>

              {/* Evidence preview */}
              {investigation.finding.evidence.length > 0 && (
                <div className="mt-3">
                  <h3 className="mb-1 text-xs font-medium text-gray-400">
                    Evidence ({investigation.finding.evidence.length})
                  </h3>
                  <div className="space-y-1">
                    {investigation.finding.evidence.slice(0, 3).map((ev) => (
                      <div key={ev.id} className="text-xs text-gray-600">
                        <span className="font-medium">[{ev.evidenceType}]</span>{" "}
                        {ev.quote.length > 120
                          ? ev.quote.slice(0, 120) + "..."
                          : ev.quote}
                      </div>
                    ))}
                    {investigation.finding.evidence.length > 3 && (
                      <p className="text-xs text-gray-400">
                        +{investigation.finding.evidence.length - 3} more
                      </p>
                    )}
                  </div>
                </div>
              )}

              {/* Connections preview */}
              {investigation.finding.connections &&
                investigation.finding.connections.length > 0 && (
                  <div className="mt-3">
                    <h3 className="mb-1 text-xs font-medium text-gray-400">
                      Connections ({investigation.finding.connections.length})
                    </h3>
                    <div className="space-y-1">
                      {investigation.finding.connections.slice(0, 5).map((conn: ConnectionSummary) => (
                        <div
                          key={conn.id}
                          className="flex items-center gap-2 text-xs text-gray-600"
                        >
                          <span className="rounded bg-gray-100 px-1 py-0.5 text-gray-500">
                            {conn.fromType}
                          </span>
                          <span className="text-gray-400">
                            {ROLE_LABELS[conn.role] || conn.role}
                          </span>
                          {conn.causalQuote && (
                            <span className="italic text-gray-400">
                              &ldquo;{conn.causalQuote.slice(0, 60)}...&rdquo;
                            </span>
                          )}
                          {conn.confidence != null && (
                            <span className="text-gray-400">
                              {Math.round(conn.confidence * 100)}%
                            </span>
                          )}
                        </div>
                      ))}
                    </div>
                  </div>
                )}
            </div>
          </div>
        )}

        {/* No finding produced */}
        {!investigation.finding && investigation.status === "completed" && (
          <div className="mb-6 rounded border border-gray-200 bg-gray-50 p-4">
            <p className="text-sm text-gray-500">
              No finding was produced by this investigation.
              {investigation.summary && " See summary above for details."}
            </p>
          </div>
        )}

        {/* Provenance */}
        <div className="border-t border-gray-100 pt-4">
          <h3 className="mb-2 text-sm font-medium text-gray-500">
            Provenance
          </h3>
          <dl className="grid grid-cols-2 gap-2 text-sm">
            <dt className="text-gray-500">Investigation ID</dt>
            <dd className="font-mono text-xs">{investigation.id}</dd>
            <dt className="text-gray-500">Created</dt>
            <dd>{new Date(investigation.createdAt).toLocaleString()}</dd>
          </dl>
        </div>
      </div>
    </div>
  );
}
