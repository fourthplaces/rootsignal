import type { IJsonModel } from "flexlayout-react";

export const DEFAULT_EVENTS_LAYOUT: IJsonModel = {
  global: {
    tabEnableClose: true,
    tabSetEnableMaximize: true,
    tabSetEnableTabStrip: true,
    splitterSize: 6,
    splitterExtra: 4,
    enableEdgeDock: false,
  },
  layout: {
    type: "row",
    children: [
      {
        type: "tabset",
        weight: 60,
        children: [
          { type: "tab", name: "Timeline", component: "timeline" },
        ],
      },
      {
        type: "tabset",
        weight: 40,
        children: [
          { type: "tab", name: "Causal Tree", component: "causal-tree" },
        ],
      },
    ],
  },
};
