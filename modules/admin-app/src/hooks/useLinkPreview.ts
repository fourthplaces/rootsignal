import { useState, useEffect } from "react";

export interface LinkPreviewData {
  url: string;
  title?: string;
  description?: string;
  image?: string;
  site_name?: string;
}

const cache = new Map<string, LinkPreviewData>();

export function useLinkPreview(url: string | undefined) {
  const [data, setData] = useState<LinkPreviewData | null>(
    url ? cache.get(url) ?? null : null,
  );
  const [loading, setLoading] = useState(!data && !!url);
  const [error, setError] = useState(false);

  useEffect(() => {
    if (!url) return;

    const cached = cache.get(url);
    if (cached) {
      setData(cached);
      setLoading(false);
      return;
    }

    setLoading(true);
    setError(false);

    const controller = new AbortController();

    fetch(`/api/link-preview?url=${encodeURIComponent(url)}`, {
      signal: controller.signal,
    })
      .then((res) => {
        if (!res.ok) throw new Error("fetch failed");
        return res.json();
      })
      .then((json: LinkPreviewData) => {
        cache.set(url, json);
        setData(json);
        setLoading(false);
      })
      .catch((err) => {
        if (err.name !== "AbortError") {
          setError(true);
          setLoading(false);
        }
      });

    return () => controller.abort();
  }, [url]);

  return { data, loading, error };
}
