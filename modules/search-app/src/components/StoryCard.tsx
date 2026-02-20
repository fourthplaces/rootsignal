import { cn } from "@/lib/utils";

interface StoryCardProps {
  story: Record<string, unknown>;
  score?: number;
  topMatchingSignalTitle?: string | null;
  isSelected: boolean;
  onClick: () => void;
  onTagClick?: (tagSlug: string) => void;
}

const ARC_COLORS: Record<string, string> = {
  Emerging: "text-blue-400",
  Growing: "text-green-400",
  Stable: "text-yellow-400",
  Fading: "text-gray-400",
  Resurgent: "text-purple-400",
};

export function StoryCard({ story, score, topMatchingSignalTitle, isSelected, onClick, onTagClick }: StoryCardProps) {
  const arc = (story.arc as string) ?? "";
  const category = (story.category as string) ?? "";
  const signalCount = (story.signalCount as number) ?? 0;
  const energy = (story.energy as number) ?? 0;

  return (
    <button
      onClick={onClick}
      className={cn(
        "w-full text-left px-4 py-3 border-b border-border transition-colors hover:bg-card",
        isSelected && "bg-card border-l-2 border-l-primary",
      )}
    >
      <div className="flex items-center gap-2 mb-1">
        {arc && (
          <span className={cn("text-xs font-medium", ARC_COLORS[arc] ?? "text-muted-foreground")}>
            {arc}
          </span>
        )}
        {category && (
          <span className="text-xs text-muted-foreground">{category}</span>
        )}
        <span className="ml-auto text-xs text-muted-foreground">
          {signalCount} signal{signalCount !== 1 ? "s" : ""}
        </span>
      </div>
      <h3 className="text-sm font-medium text-foreground line-clamp-2">
        {story.headline as string}
      </h3>
      {typeof story.lede === "string" && story.lede && (
        <p className="mt-1 text-xs text-muted-foreground line-clamp-2">
          {story.lede}
        </p>
      )}
      <div className="mt-1.5 flex items-center gap-3">
        <span className="text-xs text-muted-foreground/70">
          Energy: {energy.toFixed(2)}
        </span>
        {score != null && (
          <span className="text-xs text-primary">
            {(score * 100).toFixed(0)}% match
          </span>
        )}
      </div>
      {topMatchingSignalTitle && (
        <p className="mt-1 text-xs text-primary/70 line-clamp-1">
          Matched: {topMatchingSignalTitle}
        </p>
      )}
      {Array.isArray(story.tags) && story.tags.length > 0 && (
        <div className="mt-1.5 flex flex-wrap gap-1">
          {(story.tags as Array<{ slug: string; name: string }>).slice(0, 3).map((tag) => (
            <span
              key={tag.slug}
              role={onTagClick ? "button" : undefined}
              onClick={onTagClick ? (e) => { e.stopPropagation(); onTagClick(tag.slug); } : undefined}
              className={cn(
                "text-xs bg-muted px-1.5 py-0.5 rounded",
                onTagClick && "cursor-pointer hover:bg-muted/80 hover:ring-1 hover:ring-muted-foreground",
              )}
            >
              {tag.name}
            </span>
          ))}
        </div>
      )}
    </button>
  );
}
