import { cn } from "@/lib/utils";
import type { Tab } from "@/hooks/useUrlState";

interface TabBarProps {
  activeTab: Tab;
  onTabChange: (tab: Tab) => void;
  signalCount?: number;
  storyCount?: number;
}

export function TabBar({ activeTab, onTabChange, signalCount, storyCount }: TabBarProps) {
  return (
    <div className="flex border-b border-border">
      <button
        onClick={() => onTabChange("stories")}
        className={cn(
          "flex-1 px-4 py-2 text-sm font-medium transition-colors",
          activeTab === "stories"
            ? "border-b-2 border-primary text-foreground"
            : "text-muted-foreground hover:text-foreground",
        )}
      >
        Stories{storyCount != null ? ` (${storyCount})` : ""}
      </button>
      <button
        onClick={() => onTabChange("signals")}
        className={cn(
          "flex-1 px-4 py-2 text-sm font-medium transition-colors",
          activeTab === "signals"
            ? "border-b-2 border-primary text-foreground"
            : "text-muted-foreground hover:text-foreground",
        )}
      >
        Signals{signalCount != null ? ` (${signalCount})` : ""}
      </button>
    </div>
  );
}
