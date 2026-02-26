import type { ReactNode } from "react";
import { Link } from "react-router";

export const EVENT_COLORS: Record<string, string> = {
  reap_expired: "bg-gray-500/10 text-gray-400 border-gray-500/20",
  bootstrap: "bg-purple-500/10 text-purple-400 border-purple-500/20",
  search_query: "bg-blue-500/10 text-blue-400 border-blue-500/20",
  scrape_url: "bg-cyan-500/10 text-cyan-400 border-cyan-500/20",
  scrape_feed: "bg-cyan-500/10 text-cyan-400 border-cyan-500/20",
  social_scrape: "bg-pink-500/10 text-pink-400 border-pink-500/20",
  social_topic_search: "bg-pink-500/10 text-pink-400 border-pink-500/20",
  llm_extraction: "bg-amber-500/10 text-amber-400 border-amber-500/20",
  signal_created: "bg-green-500/10 text-green-400 border-green-500/20",
  signal_deduplicated: "bg-orange-500/10 text-orange-400 border-orange-500/20",
  signal_corroborated: "bg-emerald-500/10 text-emerald-400 border-emerald-500/20",
  signal_rejected: "bg-red-500/10 text-red-400 border-red-500/20",
  signal_dropped_no_date: "bg-red-500/10 text-red-300 border-red-500/20",
  expansion_query_collected: "bg-violet-500/10 text-violet-400 border-violet-500/20",
  expansion_source_created: "bg-violet-500/10 text-violet-400 border-violet-500/20",
  budget_checkpoint: "bg-gray-500/10 text-gray-400 border-gray-500/20",
  lint_batch: "bg-yellow-500/10 text-yellow-400 border-yellow-500/20",
  lint_correction: "bg-yellow-500/10 text-yellow-400 border-yellow-500/20",
  lint_rejection: "bg-red-500/10 text-red-400 border-red-500/20",
};

export type ScoutRunEvent = {
  id: string;
  parentId: string | null;
  seq: number;
  ts: string;
  type: string;
  sourceUrl?: string;
  query?: string;
  url?: string;
  provider?: string;
  platform?: string;
  identifier?: string;
  signalType?: string;
  title?: string;
  resultCount?: number;
  postCount?: number;
  items?: number;
  contentBytes?: number;
  contentChars?: number;
  signalsExtracted?: number;
  impliedQueries?: number;
  similarity?: number;
  confidence?: number;
  success?: boolean;
  action?: string;
  nodeId?: string;
  matchedId?: string;
  existingId?: string;
  newSourceUrl?: string;
  canonicalKey?: string;
  gatherings?: number;
  needs?: number;
  stale?: number;
  sourcesCreated?: number;
  spentCents?: number;
  remainingCents?: number;
  topics?: string[];
  postsFound?: number;
  reason?: string;
  strategy?: string;
  field?: string;
  oldValue?: string;
  newValue?: string;
  signalCount?: number;
  summary?: string;
};

function signalLink(id: string | undefined, children: ReactNode): ReactNode {
  if (!id) return children;
  return <Link to={`/signals/${id}`} className="text-blue-400 hover:underline">{children}</Link>;
}

export function eventDetail(e: ScoutRunEvent): ReactNode {
  switch (e.type) {
    case "reap_expired":
      return `gatherings=${e.gatherings} needs=${e.needs} stale=${e.stale}`;
    case "bootstrap":
      return `${e.sourcesCreated} sources created`;
    case "search_query":
      return `"${e.query}" → ${e.resultCount} results (${e.provider})`;
    case "scrape_url":
      return `${truncate(e.url ?? "", 60)} ${e.success ? `(${formatBytes(e.contentBytes ?? 0)})` : "(failed)"}`;
    case "scrape_feed":
      return `${truncate(e.url ?? "", 50)} → ${e.items} items`;
    case "social_scrape":
      return `${e.platform}: ${truncate(e.identifier ?? "", 40)} → ${e.postCount} posts`;
    case "social_topic_search":
      return `${e.platform}: ${e.topics?.join(", ")} → ${e.postsFound} posts`;
    case "llm_extraction":
      return `${truncate(e.sourceUrl ?? "", 40)} → ${e.signalsExtracted} signals, ${e.impliedQueries ?? 0} queries`;
    case "signal_created":
      return <>{e.signalType}: {signalLink(e.nodeId, `"${truncate(e.title ?? "", 40)}"`)} ({(e.confidence ?? 0).toFixed(2)})</>;
    case "signal_deduplicated":
      return <>{e.signalType}: {signalLink(e.matchedId, `"${truncate(e.title ?? "", 30)}"`)} → {e.action} (sim={(e.similarity ?? 0).toFixed(3)})</>;
    case "signal_corroborated":
      return <>{e.signalType}: {signalLink(e.existingId, e.existingId?.slice(0, 8))} ← {truncate(e.newSourceUrl ?? "", 30)} (sim={(e.similarity ?? 0).toFixed(3)})</>;
    case "signal_rejected":
      return `"${truncate(e.title ?? "", 40)}" — ${e.reason}`;
    case "signal_dropped_no_date":
      return `"${truncate(e.title ?? "", 40)}" — no content_date`;
    case "expansion_query_collected":
      return `"${e.query}"`;
    case "expansion_source_created":
      return `"${e.query}" → ${e.canonicalKey}`;
    case "budget_checkpoint":
      return `spent=${e.spentCents}¢ remaining=${e.remainingCents === 18446744073709551615 ? "∞" : `${e.remainingCents}¢`}`;
    case "lint_batch":
      return `${truncate(e.sourceUrl ?? "", 40)} → ${e.signalCount} signals (${e.resultCount} passed, ${e.postCount} corrected, ${e.items} rejected)`;
    case "lint_correction":
      return <>{e.signalType}: {signalLink(e.nodeId, `"${truncate(e.title ?? "", 30)}"`)} — {e.field}: {truncate(e.oldValue ?? "", 20)} → {truncate(e.newValue ?? "", 20)}</>;
    case "lint_rejection":
      return <>{e.signalType}: {signalLink(e.nodeId, `"${truncate(e.title ?? "", 30)}"`)} — {truncate(e.reason ?? "", 60)}</>;
    default:
      return "";
  }
}

export function truncate(s: string, max: number): string {
  return s.length <= max ? s : s.slice(0, max - 1) + "…";
}

export function formatBytes(b: number): string {
  if (b < 1024) return `${b}B`;
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)}KB`;
  return `${(b / (1024 * 1024)).toFixed(1)}MB`;
}
