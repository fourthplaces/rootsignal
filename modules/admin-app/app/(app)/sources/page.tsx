import { headers } from "next/headers";
import { authedClient } from "@/lib/client";
import Link from "next/link";

import { SourcesTable } from "./sources-table";

interface Source {
  id: string;
  name: string;
  sourceType: string;
  url: string | null;
  handle: string | null;
  cadenceHours: number;
  lastScrapedAt: string | null;
  isActive: boolean;
  entityId: string | null;
  qualificationStatus: string;
}

const SOURCE_TYPES = [
  { value: "website", label: "Website" },
  { value: "web_search", label: "Web Search" },
  { value: "instagram", label: "Instagram" },
  { value: "facebook", label: "Facebook" },
  { value: "x", label: "X" },
  { value: "tiktok", label: "TikTok" },
  { value: "gofundme", label: "GoFundMe" },
];

export default async function SourcesPage({
  searchParams,
}: {
  searchParams: Promise<{ type?: string }>;
}) {
  const params = await searchParams;
  const headerStore = await headers();
  const api = authedClient(headerStore.get("cookie") ?? undefined);

  const { sources: allSources } = await api.query<{ sources: Source[] }>(
    `query Sources {
      sources {
        id name sourceType url handle cadenceHours lastScrapedAt isActive entityId qualificationStatus
      }
    }`,
  );

  const activeType = params.type || null;
  const sources = activeType
    ? allSources.filter((s) => s.sourceType === activeType)
    : allSources;

  return (
    <div>
      <div className="mb-6 flex items-center justify-between">
        <h1 className="text-2xl font-bold">Sources</h1>
        <Link
          href="/sources/new"
          className="rounded bg-green-700 px-4 py-2 text-sm text-white hover:bg-green-800"
        >
          New Source
        </Link>
      </div>

      <div className="mb-4 flex flex-wrap gap-2">
        <Link
          href="/sources"
          className={`rounded-full px-3 py-1 text-sm font-medium ${
            !activeType
              ? "bg-green-700 text-white"
              : "bg-gray-100 text-gray-600 hover:bg-gray-200"
          }`}
        >
          All ({allSources.length})
        </Link>
        {SOURCE_TYPES.map((t) => {
          const count = allSources.filter((s) => s.sourceType === t.value).length;
          if (count === 0) return null;
          return (
            <Link
              key={t.value}
              href={`/sources?type=${t.value}`}
              className={`rounded-full px-3 py-1 text-sm font-medium ${
                activeType === t.value
                  ? "bg-green-700 text-white"
                  : "bg-gray-100 text-gray-600 hover:bg-gray-200"
              }`}
            >
              {t.label} ({count})
            </Link>
          );
        })}
      </div>

      <SourcesTable sources={sources} />
    </div>
  );
}
