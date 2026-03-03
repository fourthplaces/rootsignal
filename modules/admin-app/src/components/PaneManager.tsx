import { useRef, useState, useCallback, useEffect, useImperativeHandle, forwardRef } from "react";
import { Layout, Model, Actions, DockLocation } from "flexlayout-react";
import type { TabNode, TabSetNode, ITabSetRenderValues, Action } from "flexlayout-react";
import { Plus, RotateCcw } from "lucide-react";

export type PaneType = {
  name: string;
  component: string;
  render: () => React.ReactNode;
};

export type PaneManagerProps = {
  defaultLayout: Record<string, unknown>;
  paneRegistry: PaneType[];
  onModelChange?: (model: Model, action: Action) => void;
  onResetLayout?: () => void;
};

export type PaneManagerHandle = {
  getModel: () => Model;
  addTab: (component: string, name: string) => void;
  hasTab: (component: string) => boolean;
  selectTab: (component: string) => void;
};

export const PaneManager = forwardRef<PaneManagerHandle, PaneManagerProps>(
  function PaneManager({ defaultLayout, paneRegistry, onModelChange, onResetLayout }, ref) {
    const layoutRef = useRef<Layout>(null);
    const [model, setModel] = useState(() => Model.fromJson(defaultLayout as any));
    const [pickerTabsetId, setPickerTabsetId] = useState<string | null>(null);

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
      // Fallback: find first tabset
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
        onModelChange?.(m, action);
      },
      [onModelChange],
    );

    const handleRenderTabSet = useCallback(
      (node: TabSetNode, renderValues: ITabSetRenderValues) => {
        renderValues.stickyButtons.push(
          <button
            key="add-tab"
            className="flexlayout__tab_toolbar_button"
            title="Add pane"
            onClick={() => setPickerTabsetId((prev) => (prev === node.getId() ? null : node.getId()))}
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
      },
      [model, pickerTabsetId],
    );

    const handleResetLayout = useCallback(() => {
      setModel(Model.fromJson(defaultLayout as any));
      setPickerTabsetId(null);
      onResetLayout?.();
    }, [defaultLayout, onResetLayout]);

    // Close picker on click outside
    useEffect(() => {
      if (!pickerTabsetId) return;
      const handler = (e: MouseEvent) => {
        const target = e.target as HTMLElement;
        if (target.closest("[data-pane-picker]")) return;
        setPickerTabsetId(null);
      };
      document.addEventListener("mousedown", handler);
      return () => document.removeEventListener("mousedown", handler);
    }, [pickerTabsetId]);

    return (
      <div className="flex flex-col h-full">
        {/* Toolbar */}
        <div className="flex items-center gap-2 px-3 py-1.5 border-b border-border bg-card/50 shrink-0">
          <span className="text-[10px] font-medium text-muted-foreground uppercase tracking-wider">Layout</span>
          <button
            onClick={handleResetLayout}
            className="flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
            title="Reset layout to default"
          >
            <RotateCcw size={10} />
            Reset
          </button>
        </div>

        {/* Layout container — flexlayout-react needs position:relative parent */}
        <div className="flex-1 relative">
          <Layout
            ref={layoutRef}
            model={model}
            factory={factory}
            onAction={handleAction}
            onModelChange={handleModelChange}
            onRenderTabSet={handleRenderTabSet as any}
          />

          {/* Pane picker dropdown */}
          {pickerTabsetId && (
            <div
              data-pane-picker
              className="fixed z-[1100] bg-popover border border-border rounded-md shadow-lg py-1 min-w-[140px]"
              style={{ top: 60, right: 16 }}
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
            </div>
          )}
        </div>
      </div>
    );
  },
);
