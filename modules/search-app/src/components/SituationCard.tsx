import { cn } from "@/lib/utils";

interface SituationCardProps {
  situation: Record<string, unknown>;
  isSelected: boolean;
  onClick: () => void;
}

const ARC_COLORS: Record<string, string> = {
  EMERGING: "text-blue-400",
  DEVELOPING: "text-green-400",
  ACTIVE: "text-orange-400",
  COLD: "text-gray-500",
};

const ARC_LABELS: Record<string, string> = {
  EMERGING: "Emerging",
  DEVELOPING: "Developing",
  ACTIVE: "Active",
  COLD: "Cold",
};

export function SituationCard({ situation, isSelected, onClick }: SituationCardProps) {
  const arc = (situation.arc as string) ?? "";
  const signalCount = (situation.signalCount as number) ?? 0;
  const locationName = situation.locationName as string | null;

  return (
    <button
      onClick={onClick}
      className={cn(
        "w-full text-left px-4 py-3 border-b border-border transition-colors hover:bg-card",
        isSelected && "bg-card border-l-2 border-l-primary",
      )}
    >
      <div className="flex items-center gap-2 mb-1">
        {arc && arc !== "COOLING" && (
          <span className={cn("text-xs font-medium", ARC_COLORS[arc] ?? "text-muted-foreground")}>
            {ARC_LABELS[arc] ?? arc}
          </span>
        )}
        {locationName && (
          <span className="text-xs text-muted-foreground">{locationName}</span>
        )}
        <span className="ml-auto text-xs text-muted-foreground">
          {signalCount} signal{signalCount !== 1 ? "s" : ""}
        </span>
      </div>
      <h3 className="text-sm font-medium text-foreground line-clamp-2">
        {situation.headline as string}
      </h3>
      {typeof situation.lede === "string" && situation.lede && (
        <p className="mt-1 text-xs text-muted-foreground line-clamp-2">
          {situation.lede}
        </p>
      )}
    </button>
  );
}
