import { useState } from "react";
import { useQuery, useMutation } from "@apollo/client";
import {
  SUPERVISOR_FINDINGS,
  SUPERVISOR_SUMMARY,
} from "@/graphql/queries";
import { DISMISS_FINDING } from "@/graphql/mutations";

const SEVERITY_COLORS: Record<string, string> = {
  error: "bg-red-500/10 text-red-400 border-red-500/20",
  warning: "bg-amber-500/10 text-amber-400 border-amber-500/20",
  info: "bg-blue-500/10 text-blue-400 border-blue-500/20",
};

type Finding = {
  id: string;
  issueType: string;
  severity: string;
  targetId: string;
  targetLabel: string;
  description: string;
  suggestedAction: string;
  status: string;
  createdAt: string;
  resolvedAt: string | null;
};

type Summary = {
  totalOpen: number;
  totalResolved: number;
  totalDismissed: number;
  countByType: { label: string; count: number }[];
  countBySeverity: { label: string; count: number }[];
};

export function FindingsPage() {
  const region = "twincities";
  const [statusFilter, setStatusFilter] = useState<string | undefined>(
    undefined,
  );
  const [severityFilter, setSeverityFilter] = useState<string | undefined>(
    undefined,
  );
  const [typeFilter, setTypeFilter] = useState<string | undefined>(undefined);

  const { data: summaryData, refetch: refetchSummary } = useQuery(
    SUPERVISOR_SUMMARY,
    { variables: { region } },
  );

  const {
    data: findingsData,
    loading,
    refetch: refetchFindings,
  } = useQuery(SUPERVISOR_FINDINGS, {
    variables: { region, status: statusFilter, limit: 200 },
  });

  const [dismissFinding] = useMutation(DISMISS_FINDING);

  const summary: Summary | undefined = summaryData?.supervisorSummary;
  const findings: Finding[] = findingsData?.supervisorFindings ?? [];

  const filtered = findings.filter((f) => {
    if (severityFilter && f.severity !== severityFilter) return false;
    if (typeFilter && f.issueType !== typeFilter) return false;
    return true;
  });

  const handleDismiss = async (id: string) => {
    await dismissFinding({ variables: { id } });
    refetchFindings();
    refetchSummary();
  };

  const formatDate = (d: string | null) => {
    if (!d) return "—";
    return new Date(d).toLocaleDateString("en-US", {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  };

  const issueTypes = [
    ...new Set(findings.map((f) => f.issueType)),
  ].sort();

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Findings</h1>
      </div>

      {/* Summary cards */}
      {summary && (
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
          {[
            { label: "Open", value: summary.totalOpen },
            { label: "Resolved", value: summary.totalResolved },
            { label: "Dismissed", value: summary.totalDismissed },
            {
              label: "Total",
              value:
                summary.totalOpen +
                summary.totalResolved +
                summary.totalDismissed,
            },
          ].map((stat) => (
            <div key={stat.label} className="rounded-lg border border-border p-4">
              <p className="text-xs text-muted-foreground">{stat.label}</p>
              <p className="text-2xl font-semibold mt-1">{stat.value}</p>
            </div>
          ))}
        </div>
      )}

      {/* Filters */}
      <div className="flex gap-3">
        <select
          value={statusFilter ?? ""}
          onChange={(e) =>
            setStatusFilter(e.target.value || undefined)
          }
          className="px-3 py-1.5 rounded-md border border-input bg-background text-sm"
        >
          <option value="">All statuses</option>
          <option value="open">Open</option>
          <option value="resolved">Resolved</option>
          <option value="dismissed">Dismissed</option>
        </select>

        <select
          value={severityFilter ?? ""}
          onChange={(e) =>
            setSeverityFilter(e.target.value || undefined)
          }
          className="px-3 py-1.5 rounded-md border border-input bg-background text-sm"
        >
          <option value="">All severities</option>
          <option value="error">Error</option>
          <option value="warning">Warning</option>
          <option value="info">Info</option>
        </select>

        <select
          value={typeFilter ?? ""}
          onChange={(e) =>
            setTypeFilter(e.target.value || undefined)
          }
          className="px-3 py-1.5 rounded-md border border-input bg-background text-sm"
        >
          <option value="">All types</option>
          {issueTypes.map((t) => (
            <option key={t} value={t}>
              {t}
            </option>
          ))}
        </select>
      </div>

      {/* Findings table */}
      {loading ? (
        <p className="text-muted-foreground">Loading findings...</p>
      ) : filtered.length === 0 ? (
        <p className="text-muted-foreground">No findings match the current filters.</p>
      ) : (
        <div className="rounded-lg border border-border overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border bg-muted/50">
                <th className="text-left px-4 py-2 font-medium">Severity</th>
                <th className="text-left px-4 py-2 font-medium">Type</th>
                <th className="text-left px-4 py-2 font-medium">Target</th>
                <th className="text-left px-4 py-2 font-medium">Description</th>
                <th className="text-left px-4 py-2 font-medium">Created</th>
                <th className="text-left px-4 py-2 font-medium">Status</th>
                <th className="text-right px-4 py-2 font-medium">Action</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((f) => (
                <tr key={f.id} className="border-b border-border last:border-0 hover:bg-muted/30">
                  <td className="px-4 py-2">
                    <span
                      className={`inline-block px-2 py-0.5 rounded text-xs border ${SEVERITY_COLORS[f.severity] ?? "bg-muted text-muted-foreground"}`}
                    >
                      {f.severity}
                    </span>
                  </td>
                  <td className="px-4 py-2 text-muted-foreground">
                    {f.issueType}
                  </td>
                  <td className="px-4 py-2">
                    <span className="font-medium">{f.targetLabel}</span>
                  </td>
                  <td className="px-4 py-2 max-w-md truncate text-muted-foreground">
                    {f.description}
                  </td>
                  <td className="px-4 py-2 text-muted-foreground whitespace-nowrap">
                    {formatDate(f.createdAt)}
                  </td>
                  <td className="px-4 py-2">
                    <span
                      className={`text-xs ${
                        f.status === "open"
                          ? "text-amber-400"
                          : f.status === "resolved"
                            ? "text-green-400"
                            : "text-muted-foreground"
                      }`}
                    >
                      {f.status}
                    </span>
                  </td>
                  <td className="px-4 py-2 text-right">
                    {f.status === "open" && (
                      <button
                        onClick={() => handleDismiss(f.id)}
                        className="text-xs px-2 py-1 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50 transition-colors"
                      >
                        Dismiss
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
