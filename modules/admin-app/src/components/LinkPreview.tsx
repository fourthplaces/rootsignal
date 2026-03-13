import { useLinkPreview } from "@/hooks/useLinkPreview";

interface LinkPreviewProps {
  url: string;
  fallbackLabel?: string;
}

function ShimmerCard() {
  return (
    <div className="flex gap-3 rounded-lg border border-border p-3 animate-pulse">
      <div className="flex-1 space-y-2">
        <div className="h-3 w-24 rounded bg-muted-foreground/20" />
        <div className="h-4 w-3/4 rounded bg-muted-foreground/20" />
        <div className="h-3 w-full rounded bg-muted-foreground/20" />
      </div>
      <div className="h-24 w-24 shrink-0 rounded bg-muted-foreground/20" />
    </div>
  );
}

export function LinkPreview({ url, fallbackLabel }: LinkPreviewProps) {
  const { data, loading, error } = useLinkPreview(url);

  if (loading) return <ShimmerCard />;

  if (error || !data || (!data.title && !data.description)) {
    return (
      <a
        href={url}
        target="_blank"
        rel="noopener noreferrer"
        className="inline-block text-xs text-primary hover:underline"
      >
        {fallbackLabel || url}
      </a>
    );
  }

  const hostname = (() => {
    try {
      return new URL(url).hostname.replace(/^www\./, "");
    } catch {
      return url;
    }
  })();

  const favicon = `https://www.google.com/s2/favicons?domain=${hostname}&sz=32`;

  return (
    <a
      href={url}
      target="_blank"
      rel="noopener noreferrer"
      className="flex gap-3 rounded-lg border border-border p-3 transition-colors hover:bg-muted/50 no-underline"
    >
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5 mb-1">
          <img
            src={favicon}
            alt=""
            className="h-4 w-4 shrink-0"
            loading="lazy"
          />
          <span className="text-xs text-muted-foreground truncate">
            {data.site_name || hostname}
          </span>
        </div>
        {data.title && (
          <p className="text-sm font-medium text-foreground line-clamp-2 mb-0.5">
            {data.title}
          </p>
        )}
        {data.description && (
          <p className="text-xs text-muted-foreground line-clamp-2">
            {data.description}
          </p>
        )}
      </div>
      {data.image && (
        <img
          src={data.image}
          alt=""
          className="h-24 w-24 shrink-0 rounded object-cover"
          loading="lazy"
        />
      )}
    </a>
  );
}
