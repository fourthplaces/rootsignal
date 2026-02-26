import { useState } from "react";
import { useQuery } from "@apollo/client";
import { useMutation } from "@apollo/client";
import { SCRAPE_URL } from "@/graphql/mutations";
import {
  ADMIN_ARCHIVE_COUNTS,
  ADMIN_ARCHIVE_VOLUME,
  ADMIN_ARCHIVE_POSTS,
  ADMIN_ARCHIVE_SHORT_VIDEOS,
  ADMIN_ARCHIVE_STORIES,
  ADMIN_ARCHIVE_LONG_VIDEOS,
  ADMIN_ARCHIVE_PAGES,
  ADMIN_ARCHIVE_FEEDS,
  ADMIN_ARCHIVE_SEARCH_RESULTS,
  ADMIN_ARCHIVE_FILES,
} from "@/graphql/queries";
import {
  AreaChart,
  Area,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
} from "recharts";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

type Tab = "posts" | "reels" | "stories" | "videos" | "pages" | "feeds" | "search" | "files";

const TABS: { key: Tab; label: string }[] = [
  { key: "posts", label: "Posts" },
  { key: "reels", label: "Reels" },
  { key: "stories", label: "Stories" },
  { key: "videos", label: "Videos" },
  { key: "pages", label: "Pages" },
  { key: "feeds", label: "Feeds" },
  { key: "search", label: "Search Results" },
  { key: "files", label: "Files" },
];

const CHART_COLORS: Record<string, string> = {
  posts: "#8b5cf6",
  shortVideos: "#ec4899",
  stories: "#f59e0b",
  longVideos: "#06b6d4",
  pages: "#10b981",
  feeds: "#6366f1",
  searchResults: "#f97316",
  files: "#64748b",
};

type ArchiveCounts = {
  posts: number;
  shortVideos: number;
  stories: number;
  longVideos: number;
  pages: number;
  feeds: number;
  searchResults: number;
  files: number;
};

type ArchivePost = {
  id: string;
  sourceUrl: string;
  permalink: string | null;
  author: string | null;
  textPreview: string | null;
  platform: string;
  hashtags: string[];
  engagementSummary: string;
  publishedAt: string | null;
  fetchCount: number;
};

type ArchiveShortVideo = {
  id: string;
  sourceUrl: string;
  permalink: string | null;
  textPreview: string | null;
  engagementSummary: string;
  publishedAt: string | null;
  fetchCount: number;
};

type ArchiveStory = {
  id: string;
  sourceUrl: string;
  permalink: string | null;
  textPreview: string | null;
  location: string | null;
  expiresAt: string | null;
  fetchedAt: string;
  fetchCount: number;
};

type ArchiveLongVideo = {
  id: string;
  sourceUrl: string;
  permalink: string | null;
  textPreview: string | null;
  engagementSummary: string;
  publishedAt: string | null;
  fetchCount: number;
};

type ArchivePage = {
  id: string;
  sourceUrl: string;
  title: string | null;
  fetchedAt: string;
  fetchCount: number;
};

type ArchiveFeed = {
  id: string;
  sourceUrl: string;
  title: string | null;
  itemCount: number;
  fetchedAt: string;
  fetchCount: number;
};

type ArchiveSearchResult = {
  id: string;
  query: string;
  resultCount: number;
  fetchedAt: string;
};

type ArchiveFile = {
  id: string;
  url: string;
  title: string | null;
  mimeType: string;
  duration: number | null;
  pageCount: number | null;
  fetchedAt: string;
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function fmtDate(d: string | null) {
  if (!d) return "-";
  return new Date(d).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function isExpired(expiresAt: string | null): boolean {
  if (!expiresAt) return false;
  return new Date(expiresAt) < new Date();
}

function linkUrl(permalink: string | null, sourceUrl: string): string {
  return permalink || sourceUrl;
}

function formatDuration(seconds: number | null): string {
  if (seconds == null) return "-";
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export function ArchivePage() {
  const [tab, setTab] = useState<Tab>("posts");

  const { data: countsData, loading: countsLoading } = useQuery(ADMIN_ARCHIVE_COUNTS);
  const { data: volumeData } = useQuery(ADMIN_ARCHIVE_VOLUME, { variables: { days: 7 } });

  const { data: postsData, loading: postsLoading } = useQuery(ADMIN_ARCHIVE_POSTS, {
    variables: { limit: 50 },
    skip: tab !== "posts",
  });
  const { data: reelsData, loading: reelsLoading } = useQuery(ADMIN_ARCHIVE_SHORT_VIDEOS, {
    variables: { limit: 50 },
    skip: tab !== "reels",
  });
  const { data: storiesData, loading: storiesLoading } = useQuery(ADMIN_ARCHIVE_STORIES, {
    variables: { limit: 50 },
    skip: tab !== "stories",
  });
  const { data: videosData, loading: videosLoading } = useQuery(ADMIN_ARCHIVE_LONG_VIDEOS, {
    variables: { limit: 50 },
    skip: tab !== "videos",
  });
  const { data: pagesData, loading: pagesLoading } = useQuery(ADMIN_ARCHIVE_PAGES, {
    variables: { limit: 50 },
    skip: tab !== "pages",
  });
  const { data: feedsData, loading: feedsLoading } = useQuery(ADMIN_ARCHIVE_FEEDS, {
    variables: { limit: 50 },
    skip: tab !== "feeds",
  });
  const { data: searchData, loading: searchLoading } = useQuery(ADMIN_ARCHIVE_SEARCH_RESULTS, {
    variables: { limit: 50 },
    skip: tab !== "search",
  });
  const { data: filesData, loading: filesLoading } = useQuery(ADMIN_ARCHIVE_FILES, {
    variables: { limit: 50 },
    skip: tab !== "files",
  });

  if (countsLoading) return <p className="text-muted-foreground">Loading archive...</p>;

  const counts: ArchiveCounts = countsData?.adminArchiveCounts ?? {
    posts: 0, shortVideos: 0, stories: 0, longVideos: 0,
    pages: 0, feeds: 0, searchResults: 0, files: 0,
  };

  const volume = volumeData?.adminArchiveVolume ?? [];

  const statCards = [
    { label: "Posts", value: counts.posts },
    { label: "Reels", value: counts.shortVideos },
    { label: "Stories", value: counts.stories },
    { label: "Videos", value: counts.longVideos },
    { label: "Pages", value: counts.pages },
    { label: "Feeds", value: counts.feeds },
    { label: "Searches", value: counts.searchResults },
    { label: "Files", value: counts.files },
  ];

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-semibold">Archive</h1>

      {/* Stat cards */}
      <div className="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-8 gap-4">
        {statCards.map((stat) => (
          <div key={stat.label} className="rounded-lg border border-border p-4">
            <p className="text-xs text-muted-foreground">{stat.label}</p>
            <p className="text-2xl font-semibold mt-1">{stat.value.toLocaleString()}</p>
          </div>
        ))}
      </div>

      {/* Ingestion volume chart */}
      <div className="rounded-lg border border-border p-4">
        <h2 className="text-sm font-medium mb-4">Ingestion Volume (7 day)</h2>
        <ResponsiveContainer width="100%" height={200}>
          <AreaChart data={volume}>
            <XAxis dataKey="day" tick={{ fontSize: 11 }} />
            <YAxis tick={{ fontSize: 11 }} />
            <Tooltip />
            <Area type="monotone" dataKey="posts" stackId="1" fill={CHART_COLORS.posts} stroke={CHART_COLORS.posts} />
            <Area type="monotone" dataKey="shortVideos" stackId="1" fill={CHART_COLORS.shortVideos} stroke={CHART_COLORS.shortVideos} />
            <Area type="monotone" dataKey="stories" stackId="1" fill={CHART_COLORS.stories} stroke={CHART_COLORS.stories} />
            <Area type="monotone" dataKey="longVideos" stackId="1" fill={CHART_COLORS.longVideos} stroke={CHART_COLORS.longVideos} />
            <Area type="monotone" dataKey="pages" stackId="1" fill={CHART_COLORS.pages} stroke={CHART_COLORS.pages} />
            <Area type="monotone" dataKey="feeds" stackId="1" fill={CHART_COLORS.feeds} stroke={CHART_COLORS.feeds} />
            <Area type="monotone" dataKey="searchResults" stackId="1" fill={CHART_COLORS.searchResults} stroke={CHART_COLORS.searchResults} />
            <Area type="monotone" dataKey="files" stackId="1" fill={CHART_COLORS.files} stroke={CHART_COLORS.files} />
          </AreaChart>
        </ResponsiveContainer>
      </div>

      {/* Tabs */}
      <div className="flex gap-1 border-b border-border overflow-x-auto">
        {TABS.map((t) => (
          <button
            key={t.key}
            onClick={() => setTab(t.key)}
            className={`px-3 py-2 text-sm whitespace-nowrap -mb-px transition-colors ${
              tab === t.key
                ? "border-b-2 border-foreground text-foreground"
                : "text-muted-foreground hover:text-foreground"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>

      {/* Tab content */}
      {tab === "posts" && (
        <TabTable loading={postsLoading}>
          <thead className="border-b border-border bg-muted/50">
          <tr>
            <Th>Link</Th><Th>Author</Th><Th>Text</Th><Th>Platform</Th><Th>Hashtags</Th><Th>Engagement</Th><Th className="text-right">Published</Th><Th className="text-right">Fetches</Th><Th></Th>
          </tr>
          </thead>
          <tbody>
            {(postsData?.adminArchivePosts ?? []).map((p: ArchivePost) => (
              <Tr key={p.id}>
                <Td><ExtLink href={linkUrl(p.permalink, p.sourceUrl)} /></Td>
                <Td>{p.author || "-"}</Td>
                <Td className="max-w-xs truncate">{p.textPreview || "-"}</Td>
                <Td>{p.platform}</Td>
                <Td className="max-w-[120px] truncate">{p.hashtags.length ? p.hashtags.map(h => `#${h}`).join(" ") : "-"}</Td>
                <Td>{p.engagementSummary || "-"}</Td>
                <Td className="text-right">{fmtDate(p.publishedAt)}</Td>
                <Td className="text-right">{p.fetchCount}</Td>
                <Td><ScrapeBtn url={linkUrl(p.permalink, p.sourceUrl)} /></Td>
              </Tr>
            ))}
          </tbody>
        </TabTable>
      )}

      {tab === "reels" && (
        <TabTable loading={reelsLoading}>
          <thead className="border-b border-border bg-muted/50">
          <tr>
            <Th>Link</Th><Th>Text</Th><Th>Engagement</Th><Th className="text-right">Published</Th><Th className="text-right">Fetches</Th>
          </tr>
          </thead>
          <tbody>
            {(reelsData?.adminArchiveShortVideos ?? []).map((v: ArchiveShortVideo) => (
              <Tr key={v.id}>
                <Td><ExtLink href={linkUrl(v.permalink, v.sourceUrl)} /></Td>
                <Td className="max-w-md truncate">{v.textPreview || "-"}</Td>
                <Td>{v.engagementSummary || "-"}</Td>
                <Td className="text-right">{fmtDate(v.publishedAt)}</Td>
                <Td className="text-right">{v.fetchCount}</Td>
              </Tr>
            ))}
          </tbody>
        </TabTable>
      )}

      {tab === "stories" && (
        <TabTable loading={storiesLoading}>
          <thead className="border-b border-border bg-muted/50">
          <tr>
            <Th>Link</Th><Th>Text</Th><Th>Location</Th><Th>Expires</Th><Th className="text-right">Fetched</Th><Th className="text-right">Fetches</Th>
          </tr>
          </thead>
          <tbody>
            {(storiesData?.adminArchiveStories ?? []).map((s: ArchiveStory) => (
              <Tr key={s.id} className={isExpired(s.expiresAt) ? "opacity-50" : undefined}>
                <Td><ExtLink href={linkUrl(s.permalink, s.sourceUrl)} /></Td>
                <Td className="max-w-md truncate">{s.textPreview || "-"}</Td>
                <Td>{s.location || "-"}</Td>
                <Td>{s.expiresAt ? fmtDate(s.expiresAt) : "-"}</Td>
                <Td className="text-right">{fmtDate(s.fetchedAt)}</Td>
                <Td className="text-right">{s.fetchCount}</Td>
              </Tr>
            ))}
          </tbody>
        </TabTable>
      )}

      {tab === "videos" && (
        <TabTable loading={videosLoading}>
          <thead className="border-b border-border bg-muted/50">
          <tr>
            <Th>Link</Th><Th>Text</Th><Th>Engagement</Th><Th className="text-right">Published</Th><Th className="text-right">Fetches</Th>
          </tr>
          </thead>
          <tbody>
            {(videosData?.adminArchiveLongVideos ?? []).map((v: ArchiveLongVideo) => (
              <Tr key={v.id}>
                <Td><ExtLink href={linkUrl(v.permalink, v.sourceUrl)} /></Td>
                <Td className="max-w-md truncate">{v.textPreview || "-"}</Td>
                <Td>{v.engagementSummary || "-"}</Td>
                <Td className="text-right">{fmtDate(v.publishedAt)}</Td>
                <Td className="text-right">{v.fetchCount}</Td>
              </Tr>
            ))}
          </tbody>
        </TabTable>
      )}

      {tab === "pages" && (
        <TabTable loading={pagesLoading}>
          <thead className="border-b border-border bg-muted/50">
          <tr>
            <Th>URL</Th><Th>Title</Th><Th className="text-right">Fetched</Th><Th className="text-right">Fetches</Th><Th></Th>
          </tr>
          </thead>
          <tbody>
            {(pagesData?.adminArchivePages ?? []).map((p: ArchivePage) => (
              <Tr key={p.id}>
                <Td><ExtLink href={p.sourceUrl} /></Td>
                <Td className="max-w-md truncate">{p.title || "-"}</Td>
                <Td className="text-right">{fmtDate(p.fetchedAt)}</Td>
                <Td className="text-right">{p.fetchCount}</Td>
                <Td><ScrapeBtn url={p.sourceUrl} /></Td>
              </Tr>
            ))}
          </tbody>
        </TabTable>
      )}

      {tab === "feeds" && (
        <TabTable loading={feedsLoading}>
          <thead className="border-b border-border bg-muted/50">
          <tr>
            <Th>URL</Th><Th>Title</Th><Th className="text-right">Items</Th><Th className="text-right">Fetched</Th><Th className="text-right">Fetches</Th>
          </tr>
          </thead>
          <tbody>
            {(feedsData?.adminArchiveFeeds ?? []).map((f: ArchiveFeed) => (
              <Tr key={f.id}>
                <Td><ExtLink href={f.sourceUrl} /></Td>
                <Td className="max-w-md truncate">{f.title || "-"}</Td>
                <Td className="text-right">{f.itemCount}</Td>
                <Td className="text-right">{fmtDate(f.fetchedAt)}</Td>
                <Td className="text-right">{f.fetchCount}</Td>
              </Tr>
            ))}
          </tbody>
        </TabTable>
      )}

      {tab === "search" && (
        <TabTable loading={searchLoading}>
          <thead className="border-b border-border bg-muted/50">
          <tr>
            <Th>Query</Th><Th className="text-right">Results</Th><Th className="text-right">Fetched</Th>
          </tr>
          </thead>
          <tbody>
            {(searchData?.adminArchiveSearchResults ?? []).map((s: ArchiveSearchResult) => (
              <Tr key={s.id}>
                <Td className="max-w-md truncate">{s.query}</Td>
                <Td className="text-right">{s.resultCount}</Td>
                <Td className="text-right">{fmtDate(s.fetchedAt)}</Td>
              </Tr>
            ))}
          </tbody>
        </TabTable>
      )}

      {tab === "files" && (
        <TabTable loading={filesLoading}>
          <thead className="border-b border-border bg-muted/50">
          <tr>
            <Th>URL</Th><Th>Title</Th><Th>Type</Th><Th className="text-right">Duration</Th><Th className="text-right">Pages</Th><Th className="text-right">Fetched</Th>
          </tr>
          </thead>
          <tbody>
            {(filesData?.adminArchiveFiles ?? []).map((f: ArchiveFile) => (
              <Tr key={f.id}>
                <Td><ExtLink href={f.url} /></Td>
                <Td className="max-w-xs truncate">{f.title || "-"}</Td>
                <Td>{f.mimeType}</Td>
                <Td className="text-right">{formatDuration(f.duration)}</Td>
                <Td className="text-right">{f.pageCount ?? "-"}</Td>
                <Td className="text-right">{fmtDate(f.fetchedAt)}</Td>
              </Tr>
            ))}
          </tbody>
        </TabTable>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Shared table primitives
// ---------------------------------------------------------------------------

function TabTable({ loading, children }: { loading: boolean; children: React.ReactNode }) {
  if (loading) return <p className="text-muted-foreground py-4">Loading...</p>;
  return (
    <div className="rounded-lg border border-border overflow-hidden overflow-x-auto">
      <table className="w-full text-sm">{children}</table>
    </div>
  );
}

function Th({ children, className }: { children?: React.ReactNode; className?: string }) {
  return (
    <th className={`px-3 py-2 text-left text-xs font-medium text-muted-foreground ${className ?? ""}`}>
      {children}
    </th>
  );
}

function Tr({ children, className }: { children: React.ReactNode; className?: string }) {
  return <tr className={`border-b border-border last:border-0 hover:bg-muted/30 ${className ?? ""}`}>{children}</tr>;
}

function Td({ children, className }: { children?: React.ReactNode; className?: string }) {
  return <td className={`px-3 py-2 ${className ?? ""}`}>{children}</td>;
}

function ScrapeBtn({ url }: { url: string }) {
  const [scrape, { loading }] = useMutation(SCRAPE_URL);
  return (
    <button
      onClick={() => scrape({ variables: { url } })}
      disabled={loading}
      className="text-xs px-1.5 py-0.5 rounded border border-border text-muted-foreground hover:text-foreground hover:bg-accent/50 disabled:opacity-50"
    >
      {loading ? "..." : "Scrape"}
    </button>
  );
}

function ExtLink({ href }: { href: string }) {
  const display = href
    .replace(/^https?:\/\//, "")
    .replace(/^www\./, "");
  const truncated = display.length > 40 ? display.slice(0, 40) + "..." : display;
  return (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      className="text-blue-400 hover:underline"
      title={href}
    >
      {truncated}
    </a>
  );
}
