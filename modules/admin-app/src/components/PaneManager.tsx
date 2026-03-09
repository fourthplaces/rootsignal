import { useRef, useState, useCallback, useImperativeHandle, forwardRef } from "react";
import { Layout, Model, Actions, DockLocation } from "flexlayout-react";
import type { TabNode, TabSetNode, ITabSetRenderValues, Action } from "flexlayout-react";
import { Plus } from "lucide-react";
import { VirtualPopover } from "./Popover";

export type PaneType = {
  name: string;
  component: string;
  render: () => React.ReactNode;
};

export type PaneManagerProps = {
  defaultLayout: Record<string, unknown>;
  paneRegistry: PaneType[];
  storageKey?: string;
  onModelChange?: (model: Model, action: Action) => void;
};

export type PaneManagerHandle = {
  getModel: () => Model;
  addTab: (component: string, name: string) => void;
  hasTab: (component: string) => boolean;
  selectTab: (component: string) => void;
};

export const PaneManager = forwardRef<PaneManagerHandle, PaneManagerProps>(
  function PaneManager({ defaultLayout, paneRegistry, storageKey, onModelChange }, ref) {
    const layoutRef = useRef<Layout>(null);
    const [model] = useState(() => {
      if (storageKey) {
        try {
          const saved = localStorage.getItem(storageKey);
          if (saved) return Model.fromJson(JSON.parse(saved));
        } catch { /* fall through to default */ }
      }
      return Model.fromJson(defaultLayout as any);
    });
    const [pickerTabsetId, setPickerTabsetId] = useState<string | null>(null);
    const [pickerAnchor, setPickerAnchor] = useState<DOMRect | null>(null);

    // Find a tab by component name
    const findTab = useCallback(
      (component: string): TabNode | null => {
        let found: TabNode | null = null;
        model.visitNodes((node) => {
          if (!found && "getComponent" in node && (node as any).getComponent() === component) {
            found = node as TabNode;
          }
        });
        return found;
      },
      [model],
    );

    // Find the active tabset (or first tabset)
    const findActiveTabset = useCallback((): string | null => {
      const active = model.getActiveTabset();
      if (active) return active.getId();
      let firstTabset: string | null = null;
      model.visitNodes((node) => {
        if (!firstTabset && node.getType() === "tabset") {
          firstTabset = node.getId();
        }
      });
      return firstTabset;
    }, [model]);

    useImperativeHandle(ref, () => ({
      getModel: () => model,
      addTab: (component: string, name: string) => {
        const tabsetId = findActiveTabset();
        if (!tabsetId) return;
        model.doAction(
          Actions.addNode(
            { type: "tab", name, component },
            tabsetId,
            DockLocation.CENTER,
            -1,
            true,
          ),
        );
      },
      hasTab: (component: string) => findTab(component) !== null,
      selectTab: (component: string) => {
        const tab = findTab(component);
        if (tab) {
          model.doAction(Actions.selectTab(tab.getId()));
        }
      },
    }), [model, findTab, findActiveTabset]);

    const factory = useCallback(
      (node: TabNode) => {
        const componentName = node.getComponent();
        const pane = paneRegistry.find((p) => p.component === componentName);
        if (pane) return pane.render();
        return <div className="p-4 text-sm text-muted-foreground">Unknown pane: {componentName}</div>;
      },
      [paneRegistry],
    );

    const handleAction = useCallback(
      (action: Action) => {
        return action;
      },
      [],
    );

    const handleModelChange = useCallback(
      (m: Model, action: Action) => {
        if (storageKey) {
          try { localStorage.setItem(storageKey, JSON.stringify(m.toJson())); } catch { /* quota exceeded */ }
        }
        onModelChange?.(m, action);
      },
      [storageKey, onModelChange],
    );

    const handleRenderTabSet = useCallback(
      (node: TabSetNode, renderValues: ITabSetRenderValues) => {
        renderValues.stickyButtons.push(
          <button
            key="add-tab"
            className="flexlayout__tab_toolbar_button"
            title="Add pane"
            onClick={(e) => {
              const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
              setPickerTabsetId((prev) => {
                if (prev === node.getId()) {
                  return null;
                }
                setPickerAnchor(rect);
                return node.getId();
              });
            }}
          >
            <Plus size={12} />
          </button>,
        );
      },
      [],
    );

    const addPane = useCallback(
      (component: string, name: string) => {
        if (!pickerTabsetId) return;
        model.doAction(
          Actions.addNode(
            { type: "tab", name, component },
            pickerTabsetId,
            DockLocation.CENTER,
            -1,
            true,
          ),
        );
        setPickerTabsetId(null);
        setPickerAnchor(null);
      },
      [model, pickerTabsetId],
    );

    return (
      <div className="h-full relative">
          <Layout
            ref={layoutRef}
            model={model}
            factory={factory}
            onAction={handleAction}
            onModelChange={handleModelChange}
            onRenderTabSet={handleRenderTabSet as any}
          />

          <VirtualPopover
            anchor={pickerAnchor}
            open={!!pickerTabsetId}
            onClose={() => { setPickerTabsetId(null); setPickerAnchor(null); }}
            placement="bottom-start"
          >
            {paneRegistry.map((pane) => (
              <button
                key={pane.component}
                onClick={() => addPane(pane.component, pane.name)}
                className="w-full text-left px-3 py-1.5 text-xs text-foreground hover:bg-accent transition-colors"
              >
                {pane.name}
              </button>
            ))}
          </VirtualPopover>
      </div>
    );
  },
);
