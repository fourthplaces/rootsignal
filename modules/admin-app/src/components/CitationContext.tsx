import { createContext, useContext, useMemo, type ReactNode } from "react";

type Signal = {
  __typename: string;
  id: string;
  title: string;
  summary: string;
  url?: string | null;
  actionUrl?: string | null;
};

type CitationEntry = {
  number: number;
  signal: Signal;
};

type CitationContextValue = {
  getEntry: (signalId: string) => CitationEntry | null;
  totalCitations: number;
  totalSources: number;
};

const CitationCtx = createContext<CitationContextValue>({
  getEntry: () => null,
  totalCitations: 0,
  totalSources: 0,
});

const ANNOTATION_RE = /\[signal:([^\]\[]+)\]/g;

export function CitationProvider({
  signals,
  briefingBody,
  children,
}: {
  signals: Signal[];
  briefingBody: string | null | undefined;
  children: ReactNode;
}) {
  const value = useMemo(() => {
    const signalMap = new Map<string, Signal>();
    for (const s of signals) {
      signalMap.set(s.id, s);
    }

    const numberMap = new Map<string, number>();
    const sourceUrls = new Set<string>();
    let nextNumber = 1;

    if (briefingBody) {
      ANNOTATION_RE.lastIndex = 0;
      let match;
      while ((match = ANNOTATION_RE.exec(briefingBody)) !== null) {
        const id = match[1]!;
        if (!numberMap.has(id)) {
          numberMap.set(id, nextNumber++);
          const sig = signalMap.get(id);
          const sigUrl = sig?.actionUrl || sig?.url;
          if (sigUrl) sourceUrls.add(sigUrl);
        }
      }
    }

    return {
      getEntry(signalId: string): CitationEntry | null {
        const number = numberMap.get(signalId);
        if (number === undefined) return null;
        const signal = signalMap.get(signalId);
        if (!signal) return null;
        return { number, signal };
      },
      totalCitations: numberMap.size,
      totalSources: sourceUrls.size,
    };
  }, [signals, briefingBody]);

  return <CitationCtx.Provider value={value}>{children}</CitationCtx.Provider>;
}

export function useCitations() {
  return useContext(CitationCtx);
}
