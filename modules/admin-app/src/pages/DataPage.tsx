import { useSearchParams } from "react-router";
import { SourcesPage } from "./SourcesPage";
import { RegionsPage } from "./RegionsPage";
import { ClustersPage } from "./ClustersPage";
import { SignalsPage } from "./SignalsPage";
import { SituationsPage } from "./SituationsPage";

const TABS = ["sources", "regions", "signals", "clusters", "situations"] as const;
type Tab = (typeof TABS)[number];

const TAB_LABELS: Record<Tab, string> = {
  sources: "Sources",
  regions: "Regions",
  clusters: "Clusters",
  signals: "Signals",
  situations: "Situations",
};

const TAB_COMPONENTS: Record<Tab, React.ComponentType> = {
  sources: SourcesPage,
  regions: RegionsPage,
  clusters: ClustersPage,
  signals: SignalsPage,
  situations: SituationsPage,
};

export function DataPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const rawTab = searchParams.get("tab");
  const tab: Tab = TABS.includes(rawTab as Tab) ? (rawTab as Tab) : "sources";

  const setTab = (t: Tab) =>
    setSearchParams((prev) => {
      prev.set("tab", t);
      return prev;
    }, { replace: true });

  const Content = TAB_COMPONENTS[tab];

  return (
    <div className="space-y-4">
      <div className="flex gap-1 border-b border-border">
        {TABS.map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            className={`px-3 py-2 text-sm -mb-px transition-colors ${
              tab === t
                ? "border-b-2 border-foreground text-foreground"
                : "text-muted-foreground hover:text-foreground"
            }`}
          >
            {TAB_LABELS[t]}
          </button>
        ))}
      </div>

      <Content />
    </div>
  );
}
