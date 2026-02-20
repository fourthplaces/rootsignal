import { cn } from "@/lib/utils";

const TYPE_STYLES: Record<string, { bg: string; text: string; label: string }> = {
  GqlGatheringSignal: { bg: "bg-gathering/10", text: "text-gathering", label: "Gathering" },
  GqlAidSignal: { bg: "bg-aid/10", text: "text-aid", label: "Aid" },
  GqlNeedSignal: { bg: "bg-need/10", text: "text-need", label: "Need" },
  GqlNoticeSignal: { bg: "bg-notice/10", text: "text-notice", label: "Notice" },
  GqlTensionSignal: { bg: "bg-tension/10", text: "text-tension", label: "Tension" },
};

// eslint-disable-next-line @typescript-eslint/no-explicit-any
interface SignalCardProps {
  signal: Record<string, unknown>;
  score?: number;
  isSelected: boolean;
  onClick: () => void;
}

export function SignalCard({ signal, score, isSelected, onClick }: SignalCardProps) {
  const typename = (signal.__typename as string) ?? "";
  const style = TYPE_STYLES[typename] ?? { bg: "bg-muted", text: "text-muted-foreground", label: "Signal" };

  return (
    <button
      onClick={onClick}
      className={cn(
        "w-full text-left px-4 py-3 border-b border-border transition-colors hover:bg-card",
        isSelected && "bg-card border-l-2 border-l-primary",
      )}
    >
      <div className="flex items-center gap-2 mb-1">
        <span className={cn("rounded px-1.5 py-0.5 text-xs font-medium", style.bg, style.text)}>
          {style.label}
        </span>
        {score != null && (
          <span className="text-xs text-muted-foreground">
            {(score * 100).toFixed(0)}% match
          </span>
        )}
      </div>
      <h3 className="text-sm font-medium text-foreground line-clamp-2">
        {signal.title as string}
      </h3>
      <p className="mt-1 text-xs text-muted-foreground line-clamp-2">
        {signal.summary as string}
      </p>
      {typeof signal.locationName === "string" && signal.locationName && (
        <p className="mt-1 text-xs text-muted-foreground/70">
          {signal.locationName}
        </p>
      )}
    </button>
  );
}
