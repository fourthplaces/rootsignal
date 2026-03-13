import { useState, useRef, useEffect, type JSX } from "react";
import { useCitations } from "./CitationContext";

const TYPE_LABELS: Record<string, string> = {
  GqlGatheringSignal: "Gathering",
  GqlResourceSignal: "Resource",
  GqlHelpRequestSignal: "Help Request",
  GqlAnnouncementSignal: "Announcement",
  GqlConcernSignal: "Concern",
  GqlConditionSignal: "Condition",
};

export function CitationRef(props: JSX.IntrinsicElements["span"] & { kind?: string; identifier?: string }) {
  const { kind, identifier, ...rest } = props;
  const { getEntry } = useCitations();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLSpanElement>(null);

  // Close on click outside
  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [open]);

  if (kind !== "signal" || !identifier) {
    return null;
  }

  const entry = getEntry(identifier);
  if (!entry) {
    return (
      <sup className="text-[10px] text-muted-foreground cursor-default" title="Source no longer available">
        [?]
      </sup>
    );
  }

  const { number, signal } = entry;
  const typeLabel = TYPE_LABELS[signal.__typename] ?? "Signal";

  return (
    <span ref={ref} className="relative inline" {...rest}>
      <sup
        className="text-[10px] text-blue-400 cursor-pointer hover:text-blue-300 transition-colors"
        onClick={() => setOpen((o) => !o)}
      >
        [{number}]
      </sup>
      {open && (
        <span className="absolute z-50 bottom-full left-0 mb-1 w-72 rounded border border-border bg-background shadow-lg p-3 text-xs">
          <span className="flex items-center gap-1.5 mb-1">
            <span className="rounded px-1 py-0.5 text-[10px] bg-muted text-muted-foreground">
              {typeLabel}
            </span>
          </span>
          <span className="block font-medium text-foreground mb-1 leading-snug">
            {signal.title}
          </span>
          {signal.summary && (
            <span className="block text-muted-foreground line-clamp-3 mb-2 leading-relaxed">
              {signal.summary}
            </span>
          )}
          {signal.actionUrl && (
            <a
              href={signal.actionUrl}
              target="_blank"
              rel="noreferrer"
              className="text-blue-400 hover:underline text-[11px]"
              onClick={(e) => e.stopPropagation()}
            >
              View source &rarr;
            </a>
          )}
        </span>
      )}
    </span>
  );
}
