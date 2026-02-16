"use client";

import { useState } from "react";
import Link from "next/link";

interface PageSnapshot {
  id: string;
  pageUrl: string;
  url: string;
  contentHash: string;
  fetchedVia: string;
  contentPreview: string | null;
  crawledAt: string;
  scrapeStatus: string;
}

interface Signal {
  id: string;
  signalType: string;
  content: string;
  about: string | null;
  createdAt: string;
}

type Tab = "signals" | "snapshots";

export function SourceTabs({
  snapshots,
  signals,
}: {
  snapshots: PageSnapshot[];
  signals: Signal[];
}) {
  const [tab, setTab] = useState<Tab>("signals");

  return (
    <div className="mt-8">
      <div className="flex border-b border-gray-200">
        <button
          onClick={() => setTab("signals")}
          className={`px-4 py-2 text-sm font-medium ${
            tab === "signals"
              ? "border-b-2 border-green-700 text-green-700"
              : "text-gray-500 hover:text-gray-700"
          }`}
        >
          Signals ({signals.length})
        </button>
        <button
          onClick={() => setTab("snapshots")}
          className={`px-4 py-2 text-sm font-medium ${
            tab === "snapshots"
              ? "border-b-2 border-green-700 text-green-700"
              : "text-gray-500 hover:text-gray-700"
          }`}
        >
          Page Snapshots ({snapshots.length})
        </button>
      </div>

      <div className="mt-4">
        {tab === "signals" && <SignalsPanel signals={signals} />}
        {tab === "snapshots" && <SnapshotsPanel snapshots={snapshots} />}
      </div>
    </div>
  );
}

function SignalsPanel({ signals }: { signals: Signal[] }) {
  if (signals.length === 0) {
    return <p className="text-sm text-gray-500">No signals extracted yet.</p>;
  }

  return (
    <ul className="space-y-1">
      {signals.map((s) => (
        <li key={s.id} className="rounded border border-gray-200 bg-white px-3 py-2 text-sm">
          <div className="flex items-center gap-2">
            <span
              className={`inline-block shrink-0 rounded-full px-2 py-0.5 text-xs font-medium ${
                s.signalType === "ask"
                  ? "bg-orange-100 text-orange-800"
                  : s.signalType === "give"
                    ? "bg-green-100 text-green-800"
                    : s.signalType === "event"
                      ? "bg-blue-100 text-blue-800"
                      : "bg-gray-100 text-gray-800"
              }`}
            >
              {s.signalType}
            </span>
            <Link href={`/signals/${s.id}`} className="text-green-700 hover:underline">
              {s.content.length > 120 ? s.content.slice(0, 120) + "..." : s.content}
            </Link>
            <span className="ml-auto shrink-0 text-xs text-gray-400">
              {new Date(s.createdAt).toLocaleDateString()}
            </span>
          </div>
          {s.about && <p className="ml-14 text-xs text-gray-400">{s.about}</p>}
        </li>
      ))}
    </ul>
  );
}

function SnapshotsPanel({ snapshots }: { snapshots: PageSnapshot[] }) {
  if (snapshots.length === 0) {
    return <p className="text-sm text-gray-500">No snapshots yet. Run a scrape to collect pages.</p>;
  }

  return (
    <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
      <table className="min-w-full divide-y divide-gray-200">
        <thead className="bg-gray-50">
          <tr>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Page URL</th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Via</th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Status</th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Crawled</th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Preview</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-gray-200">
          {snapshots.map((snap) => (
            <tr key={snap.id} className="hover:bg-gray-50">
              <td className="max-w-xs truncate px-4 py-3 text-sm">
                <Link
                  href={`/snapshots/${snap.id}`}
                  className="text-green-700 hover:underline"
                  title={snap.pageUrl}
                >
                  {snap.pageUrl.replace(/^https?:\/\//, "").slice(0, 60)}
                </Link>
              </td>
              <td className="whitespace-nowrap px-4 py-3 text-xs text-gray-500">
                {snap.fetchedVia}
              </td>
              <td className="whitespace-nowrap px-4 py-3 text-xs">
                <span
                  className={`rounded px-2 py-0.5 text-xs font-medium ${
                    snap.scrapeStatus === "completed"
                      ? "bg-green-100 text-green-700"
                      : "bg-yellow-100 text-yellow-700"
                  }`}
                >
                  {snap.scrapeStatus}
                </span>
              </td>
              <td className="whitespace-nowrap px-4 py-3 text-xs text-gray-500">
                {new Date(snap.crawledAt).toLocaleString()}
              </td>
              <td className="max-w-xs truncate px-4 py-3 text-xs text-gray-400">
                {snap.contentPreview || "\u2014"}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
