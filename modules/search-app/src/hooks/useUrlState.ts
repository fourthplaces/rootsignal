import { useCallback, useEffect, useRef } from "react";
import { useSearchParams } from "react-router";

export type Tab = "signals" | "situations";

interface UrlState {
  lat: number | null;
  lng: number | null;
  z: number | null;
  q: string;
  tab: Tab;
  id: string | null;
}

function parseUrlState(params: URLSearchParams): UrlState {
  return {
    lat: params.has("lat") ? Number(params.get("lat")) : null,
    lng: params.has("lng") ? Number(params.get("lng")) : null,
    z: params.has("z") ? Number(params.get("z")) : null,
    q: params.get("q") ?? "",
    tab: (params.get("tab") as Tab) === "signals" ? "signals" : "situations",
    id: params.get("id"),
  };
}

export function useUrlState() {
  const [searchParams, setSearchParams] = useSearchParams();
  const state = parseUrlState(searchParams);
  const isInitialMount = useRef(true);

  // On mount, mark initial state loaded
  useEffect(() => {
    isInitialMount.current = false;
  }, []);

  const updateUrl = useCallback(
    (
      updates: Partial<UrlState>,
      options?: { replace?: boolean },
    ) => {
      setSearchParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          if (updates.lat != null) next.set("lat", updates.lat.toFixed(4));
          if (updates.lng != null) next.set("lng", updates.lng.toFixed(4));
          if (updates.z != null) next.set("z", updates.z.toFixed(1));
          if (updates.q !== undefined) {
            if (updates.q) next.set("q", updates.q);
            else next.delete("q");
          }
          if (updates.tab !== undefined) next.set("tab", updates.tab);
          if (updates.id !== undefined) {
            if (updates.id) next.set("id", updates.id);
            else next.delete("id");
          }
          return next;
        },
        { replace: options?.replace ?? true },
      );
    },
    [setSearchParams],
  );

  return { ...state, isInitialMount: isInitialMount.current, updateUrl };
}
