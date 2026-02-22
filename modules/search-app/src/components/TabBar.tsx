import { cn } from "@/lib/utils";
import type { Tab } from "@/hooks/useUrlState";

interface TabBarProps {
  activeTab: Tab;
  onTabChange: (tab: Tab) => void;
  signalCount?: number;
  situationCount?: number;
}

export function TabBar({ activeTab, onTabChange, signalCount, situationCount }: TabBarProps) {
  return (
    <div className="flex border-b border-border">
      <button
        onClick={() => onTabChange("situations")}
        className={cn(
          "flex-1 px-4 py-2 text-sm font-medium transition-colors",
          activeTab === "situations"
            ? "border-b-2 border-primary text-foreground"
            : "text-muted-foreground hover:text-foreground",
        )}
      >
        Situations{situationCount != null ? ` (${situationCount})` : ""}
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
