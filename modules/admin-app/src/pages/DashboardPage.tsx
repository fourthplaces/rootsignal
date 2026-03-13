import { useEffect } from "react";
import { useQuery, NetworkStatus } from "@apollo/client";
import { Link } from "react-router";
import { ADMIN_DASHBOARD } from "@/graphql/queries";
import { PipelineCards } from "@/components/dashboard/PipelineCards";
import { BudgetSummary } from "@/components/dashboard/BudgetSummary";
import { QualityScorecard } from "@/components/dashboard/QualityScorecard";
import { DomainCountCard } from "@/components/dashboard/DomainCountCard";
import { HottestConcernsTable } from "@/components/dashboard/HottestConcernsTable";

interface AdminDashboardData {
  adminDashboard: {
    pipelineStatus: {
      runId: string;
      region: string;
      flowType: string;
      status: string;
      startedAt: string;
      finishedAt: string | null;
      error: string | null;
    }[];
    errorCount: number;
    budgetSpentCents: number;
    budgetLimitCents: number;
    signalsMissingCategory: number;
    signalsWithoutLocation: number;
    orphanedSignals: number;
    validationSummary: {
      totalOpen: number;
      countBySeverity: { label: string; count: number }[];
    };
    countByType: { signalType: string; count: number }[];
    situationCount: number;
    hottestConcerns: {
      id: string;
      title: string;
      category: string | null;
      causeHeat: number;
      corroborationCount: number;
    }[];
  };
}

function SectionHeader({ title }: { title: string }) {
  return (
    <div className="flex items-center gap-2">
      <div className="w-1 h-4 rounded-full bg-zinc-500" />
      <h2 className="text-sm font-medium tracking-wide uppercase text-muted-foreground">{title}</h2>
    </div>
  );
}

const TYPE_LABELS: Record<string, string> = {
  Gathering: "Gatherings",
  Resource: "Resources",
  HelpRequest: "Help Requests",
  Announcement: "Announcements",
  Concern: "Concerns",
  Condition: "Conditions",
};

export function DashboardPage() {
  const { data, loading, error, networkStatus, startPolling, stopPolling, refetch } =
    useQuery<AdminDashboardData>(ADMIN_DASHBOARD, {
      pollInterval: 30_000,
      errorPolicy: "all",
      fetchPolicy: "cache-and-network",
      notifyOnNetworkStatusChange: true,
    });

  useEffect(() => {
    const handler = () => {
      if (document.visibilityState === "hidden") stopPolling();
      else {
        refetch();
        startPolling(30_000);
      }
    };
    document.addEventListener("visibilitychange", handler);
    return () => document.removeEventListener("visibilitychange", handler);
  }, [startPolling, stopPolling, refetch]);

  const initialLoading = loading && !data;
  const refreshing = networkStatus === NetworkStatus.poll;

  if (initialLoading) return <p className="text-muted-foreground">Loading dashboard...</p>;

  const d = data?.adminDashboard;
  if (!d) return <p className="text-muted-foreground">No data</p>;

  return (
    <div className={`space-y-8 ${refreshing ? "opacity-90" : ""}`}>
      <h1 className="text-xl font-semibold">Dashboard</h1>

      {error && data && (
        <div className="rounded-lg border border-amber-500/30 bg-amber-500/5 px-4 py-2 text-sm text-amber-400">
          Data may be stale — {error.message}
        </div>
      )}

      {/* Section 1: System Health */}
      <section className="space-y-3">
        <SectionHeader title="System Health" />
        <PipelineCards statuses={d.pipelineStatus} />
        <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
          <Link
            to="/events"
            className={`rounded-lg border p-4 hover:bg-accent/50 transition-colors focus-visible:ring-2 ring-ring ${
              d.errorCount > 0 ? "border-l-2 border-l-red-500/60 border-border" : "border-border"
            }`}
          >
            <p className="text-sm font-medium">Errors (24h)</p>
            <p className="text-2xl font-semibold tabular-nums mt-1">{d.errorCount}</p>
          </Link>
          <BudgetSummary spentCents={d.budgetSpentCents} limitCents={d.budgetLimitCents} />
        </div>
      </section>

      {/* Section 2: Data Quality */}
      <section className="space-y-3">
        <SectionHeader title="Data Quality" />
        <div className="grid grid-cols-2 gap-3">
          <QualityScorecard label="Missing category" count={d.signalsMissingCategory} />
          <QualityScorecard label="Without location" count={d.signalsWithoutLocation} />
          <QualityScorecard label="Orphaned signals" count={d.orphanedSignals} />
          <Link
            to="/findings"
            className="rounded-lg border border-border p-4 text-left hover:bg-accent/50 transition-colors focus-visible:ring-2 ring-ring group"
          >
            <div className="flex items-center justify-between">
              <p className="text-xs text-muted-foreground">Validation issues</p>
              <span
                className={`text-xs ${d.validationSummary.totalOpen === 0 ? "text-emerald-500" : d.validationSummary.totalOpen <= 10 ? "text-amber-500" : "text-red-500"}`}
                aria-hidden="true"
              >
                {d.validationSummary.totalOpen === 0 ? "●" : d.validationSummary.totalOpen <= 10 ? "▲" : "■"}
              </span>
            </div>
            <p className="text-2xl font-semibold tabular-nums mt-1">
              {d.validationSummary.totalOpen}
            </p>
            <p className="text-xs text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity mt-1">
              View details →
            </p>
          </Link>
        </div>
      </section>

      {/* Section 3: Graph Overview */}
      <section className="space-y-3">
        <SectionHeader title="Graph Overview" />
        <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-3">
          <DomainCountCard label="Situations" count={d.situationCount} to="/situations" />
          {d.countByType.map((t) => (
            <DomainCountCard
              key={t.signalType}
              label={TYPE_LABELS[t.signalType] ?? t.signalType}
              count={t.count}
              to={`/data?tab=signals&type=${t.signalType}`}
            />
          ))}
        </div>
      </section>

      {/* Hottest Concerns */}
      <section className="space-y-3">
        <SectionHeader title="Hottest Concerns" />
        <div className="rounded-lg border border-border p-4">
          <HottestConcernsTable concerns={d.hottestConcerns} />
        </div>
      </section>
    </div>
  );
}
