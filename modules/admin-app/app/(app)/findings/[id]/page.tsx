import { headers } from "next/headers";
import { authedClient } from "@/lib/client";
import Link from "next/link";

interface FindingEvidence {
  id: string;
  evidenceType: string;
  quote: string;
  attribution: string | null;
  url: string | null;
}

interface Connection {
  id: string;
  fromType: string;
  fromId: string;
  toType: string;
  toId: string;
  role: string;
  causalQuote: string | null;
  confidence: number | null;
}

interface Finding {
  id: string;
  title: string;
  summary: string;
  status: string;
  validationStatus: string | null;
  signalVelocity: number | null;
  investigationId: string | null;
  triggerSignalId: string | null;
  createdAt: string;
  updatedAt: string;
  evidence: FindingEvidence[];
  connections: Connection[];
  connectionCount: number;
}

const STATUS_COLORS: Record<string, string> = {
  emerging: "bg-yellow-100 text-yellow-800",
  active: "bg-red-100 text-red-800",
  declining: "bg-gray-100 text-gray-600",
  resolved: "bg-green-100 text-green-800",
};

const EVIDENCE_TYPE_LABELS: Record<string, string> = {
  org_statement: "Organization Statement",
  social_media: "Social Media",
  news_reporting: "News Reporting",
  government_record: "Government Record",
  academic_research: "Academic Research",
  court_filing: "Court Filing",
};

const EVIDENCE_TYPE_COLORS: Record<string, string> = {
  org_statement: "bg-blue-100 text-blue-800",
  social_media: "bg-purple-100 text-purple-800",
  news_reporting: "bg-orange-100 text-orange-800",
  government_record: "bg-green-100 text-green-800",
  academic_research: "bg-teal-100 text-teal-800",
  court_filing: "bg-red-100 text-red-800",
};

const ROLE_LABELS: Record<string, string> = {
  response_to: "Responds to",
  affected_by: "Affected by",
  evidence_of: "Evidence of",
  driven_by: "Driven by",
};

export default async function FindingDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  const headerStore = await headers();
  const api = authedClient(headerStore.get("cookie") ?? undefined);

  const { finding } = await api.query<{ finding: Finding }>(
    `query Finding($id: ID!) {
      finding(id: $id) {
        id title summary status validationStatus signalVelocity
        investigationId triggerSignalId createdAt updatedAt
        connectionCount
        evidence {
          id evidenceType quote attribution url
        }
        connections {
          id fromType fromId toType toId role causalQuote confidence
        }
      }
    }`,
    { id },
  );

  // Group connections by role
  const connectionsByRole = finding.connections.reduce(
    (acc, conn) => {
      const role = conn.role;
      if (!acc[role]) acc[role] = [];
      acc[role].push(conn);
      return acc;
    },
    {} as Record<string, Connection[]>,
  );

  return (
    <div className="mx-auto max-w-4xl">
      <div className="mb-4">
        <Link
          href="/findings"
          className="text-sm text-blue-600 hover:underline"
        >
          &larr; Back to Findings
        </Link>
      </div>

      <div className="rounded-lg border border-gray-200 bg-white p-6">
        <div className="mb-4 flex items-center gap-3">
          <span
            className={`inline-block rounded-full px-3 py-1 text-sm font-medium ${
              STATUS_COLORS[finding.status] || "bg-gray-100"
            }`}
          >
            {finding.status}
          </span>
          {finding.validationStatus && (
            <span className="text-sm text-gray-400">
              {finding.validationStatus}
            </span>
          )}
          {finding.signalVelocity != null && (
            <span className="text-sm text-gray-400">
              {finding.signalVelocity.toFixed(1)} signals/day
            </span>
          )}
          <span className="text-sm text-gray-400">
            {finding.connectionCount} connected signals
          </span>
        </div>

        <h1 className="mb-2 text-xl font-bold">{finding.title}</h1>
        <p className="mb-6 text-gray-700">{finding.summary}</p>

        {/* Evidence */}
        <div className="mb-6">
          <h2 className="mb-3 text-lg font-semibold">Evidence</h2>
          <div className="space-y-3">
            {finding.evidence.map((ev) => (
              <div
                key={ev.id}
                className="rounded border border-gray-100 bg-gray-50 p-3"
              >
                <div className="mb-1 flex items-center gap-2">
                  <span
                    className={`inline-block rounded px-2 py-0.5 text-xs font-medium ${
                      EVIDENCE_TYPE_COLORS[ev.evidenceType] || "bg-gray-100"
                    }`}
                  >
                    {EVIDENCE_TYPE_LABELS[ev.evidenceType] || ev.evidenceType}
                  </span>
                  {ev.attribution && (
                    <span className="text-xs text-gray-500">
                      {ev.attribution}
                    </span>
                  )}
                </div>
                <blockquote className="border-l-2 border-gray-300 pl-3 text-sm italic text-gray-700">
                  {ev.quote}
                </blockquote>
                {ev.url && (
                  <a
                    href={ev.url}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="mt-1 block text-xs text-blue-600 hover:underline"
                  >
                    {ev.url.length > 80 ? ev.url.slice(0, 80) + "..." : ev.url}
                  </a>
                )}
              </div>
            ))}
          </div>
        </div>

        {/* Connected signals grouped by role */}
        <div className="mb-6">
          <h2 className="mb-3 text-lg font-semibold">Connections</h2>
          {Object.entries(connectionsByRole).map(([role, connections]) => (
            <div key={role} className="mb-4">
              <h3 className="mb-2 text-sm font-medium text-gray-500">
                {ROLE_LABELS[role] || role} ({connections.length})
              </h3>
              <div className="space-y-2">
                {connections.map((conn) => (
                  <div
                    key={conn.id}
                    className="flex items-start gap-3 rounded border border-gray-100 bg-white p-2"
                  >
                    <span className="mt-0.5 rounded bg-gray-100 px-1.5 py-0.5 text-xs text-gray-600">
                      {conn.fromType}
                    </span>
                    <div className="flex-1">
                      <Link
                        href={
                          conn.fromType === "signal"
                            ? `/signals/${conn.fromId}`
                            : `/findings/${conn.fromId}`
                        }
                        className="text-sm text-blue-600 hover:underline"
                      >
                        {conn.fromId}
                      </Link>
                      {conn.causalQuote && (
                        <p className="mt-0.5 text-xs italic text-gray-500">
                          &ldquo;{conn.causalQuote}&rdquo;
                        </p>
                      )}
                    </div>
                    {conn.confidence != null && (
                      <span className="text-xs text-gray-400">
                        {Math.round(conn.confidence * 100)}%
                      </span>
                    )}
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>

        {/* Provenance */}
        <div className="border-t border-gray-100 pt-4">
          <h3 className="mb-2 text-sm font-medium text-gray-500">
            Provenance
          </h3>
          <dl className="grid grid-cols-2 gap-2 text-sm">
            {finding.triggerSignalId && (
              <>
                <dt className="text-gray-500">Trigger Signal</dt>
                <dd>
                  <Link
                    href={`/signals/${finding.triggerSignalId}`}
                    className="text-blue-600 hover:underline"
                  >
                    {finding.triggerSignalId}
                  </Link>
                </dd>
              </>
            )}
            <dt className="text-gray-500">Created</dt>
            <dd>{new Date(finding.createdAt).toLocaleString()}</dd>
            <dt className="text-gray-500">Updated</dt>
            <dd>{new Date(finding.updatedAt).toLocaleString()}</dd>
          </dl>
        </div>
      </div>
    </div>
  );
}
