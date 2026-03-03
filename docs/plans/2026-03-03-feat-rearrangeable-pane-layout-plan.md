---
title: "feat: Rearrangeable Pane Layout (iTerm2-style)"
type: feat
date: 2026-03-03
---

# Rearrangeable Pane Layout (iTerm2-style)

## Overview

Replace the fixed `react-resizable-panels` layout on `/events` with a `flexlayout-react` tiling window manager. Users can drag panes to rearrange, split horizontally/vertically, and add/remove pane types from a picker. The implementation is a reusable `PaneManager` component that any admin page can adopt later.

## Problem Statement / Motivation

The current `/events` page has a fixed three-column layout (Timeline → Causal Tree → Investigate). Panels can be resized but not rearranged. As the admin app grows (map view, graph view, stats), users need the ability to compose their own workspace — similar to iTerm2, VS Code, or Chrome DevTools.

## Proposed Solution

Use `flexlayout-react` (a docking layout manager for React) to provide:
- **Drag-to-rearrange**: Grab a pane tab, drop it on any edge of another pane
- **Split any direction**: Right-click tab → split horizontal/vertical
- **Add pane types**: "+" button on each tabset's tab bar opens a picker
- **Close panes**: X button on each tab, provided by flexlayout-react
- **JSON layout model**: Trivially serializable for future persistence

### Architecture

```
EventsPage
  └─ EventsPaneContext.Provider        ← shared state (selectedSeq, investigateEvent, filters)
       └─ PaneManager                  ← reusable wrapper around flexlayout-react <Layout>
            ├─ factory(node) →         ← maps node.getComponent() to React components
            │   ├─ "timeline"  → <TimelinePane />
            │   ├─ "causal-tree" → <CausalTreePane />
            │   └─ "investigate" → <InvestigatePane />
            └─ default layout JSON     ← initial arrangement
```

### Default Layout

Mirrors the current arrangement — Timeline (60%) left, Causal Tree (40%) right:

```json
{
  "global": { "tabEnableClose": true, "tabSetEnableMaximize": true },
  "layout": {
    "type": "row",
    "children": [
      {
        "type": "tabset",
        "weight": 60,
        "children": [{ "type": "tab", "name": "Timeline", "component": "timeline" }]
      },
      {
        "type": "tabset",
        "weight": 40,
        "children": [{ "type": "tab", "name": "Causal Tree", "component": "causal-tree" }]
      }
    ]
  }
}
```

The Investigate pane is **not** in the default layout — it appears on demand (see below).

## Design Decisions

### Investigate Pane Trigger Model: Auto-open

When the user clicks the investigate icon on any event:
1. If an Investigate tab already exists → update `investigateEvent` in context, which triggers a remount via `key={investigateEvent.seq}` (same as today)
2. If no Investigate tab exists → programmatically add one via `model.doAction(Actions.addNode(...))` to the right of the current tabset, then set context
3. The internal close button in `InvestigateDrawer` is **removed** — tab close (X) is the sole close mechanism
4. Closing the Investigate tab sets `investigateEvent = null` in context via `onModelChange`

### Duplicate Pane Policy: Allowed but shared state

Users can open multiple instances of any pane type. All instances of a type share the same context (same filters, same selected event). This is simple and correct — scroll positions and expanded payloads remain local to each instance. The "+" picker does NOT disable already-open types.

### "+" Button Placement: Per-tabset

flexlayout-react supports `onRenderTabSet` which lets us inject a "+" button into each tabset's tab bar. Clicking it shows a small dropdown with the three pane types (Timeline, Causal Tree, Investigate). The new tab is added to that tabset.

### Pane Drag: No remount

flexlayout-react preserves component instances across drag operations by default (the `enableRenderOnDemand` model attribute controls this). This means dragging a pane does not remount it — investigation conversations, scroll positions, and expanded payloads survive drag operations.

### Last Pane Closed

If the user closes every tab, flexlayout-react renders an empty layout. We handle this by showing a centered "Add a pane" prompt in the empty state, using flexlayout-react's `onRenderTabSet` or a custom empty-state overlay.

### CSS Theme Integration

Override flexlayout-react's CSS variables to match the zinc neutral palette:

```css
/* In index.css, scoped to .flexlayout__layout */
.flexlayout__layout {
  --color-1: theme(--color-background);      /* #09090b */
  --color-2: theme(--color-card);             /* #09090b */
  --color-3: theme(--color-border);           /* #27272a */
  --color-4: theme(--color-accent);           /* #27272a */
  --color-5: theme(--color-muted-foreground); /* #a1a1aa */
  --color-6: theme(--color-foreground);       /* #fafafa */
  --color-tabset-background: theme(--color-background);
  --color-drag-rect-border: theme(--color-ring);
  --color-drag-rect-bg: oklch(from theme(--color-accent) l c h / 0.3);
  --font-family: inherit;
  --font-size: 0.75rem; /* text-xs */
}
```

Exact variable names will need verification against flexlayout-react's actual CSS API during implementation.

### AdminLayout Compatibility

The current `-m-6 h-[calc(100vh-3rem)]` escape hatch works with flexlayout-react — it just needs a fixed-height container. However, `AdminLayout`'s `<main className="flex-1 overflow-auto p-6">` must change to `overflow-hidden` when a PaneManager child is rendered (to prevent double scrollbars). This can be done via a CSS class on the page's root div or a layout slot.

### URL State

Existing URL params (`seq`, `layers`, `q`, `from`, `to`) continue to work as-is — they track filter/selection state, not layout. Layout is not persisted (for now).

### Reset Layout

A "Reset layout" button in the page toolbar resets the flexlayout-react model to the default JSON. This is the escape hatch for users who drag things into an unusable state.

## Technical Considerations

- **Bundle size**: flexlayout-react is ~30KB gzipped with React as only peer dep. Acceptable.
- **React 19 compat**: flexlayout-react supports React 18+ and 19+.
- **Performance**: flexlayout-react mounts all tabs (even hidden ones behind other tabs in a tabset). Investigate pane should NOT auto-fire investigation on mount — only when `investigateEvent` is set in context. This prevents phantom API calls from hidden tabs.
- **react-resizable-panels stays** in package.json for GraphExplorerPage (and any other page that doesn't need full tiling).

## Acceptance Criteria

- [x] User can drag pane tabs to rearrange (left/right/top/bottom of any pane)
- [x] User can split any pane horizontally or vertically
- [x] "+" button on each tabset opens a picker with Timeline / Causal Tree / Investigate
- [x] X button closes any pane tab
- [x] Clicking investigate icon auto-opens/focuses the Investigate pane
- [x] Dragging panes does NOT remount components (investigation conversations survive)
- [x] Default layout: Timeline left (60%), Causal Tree right (40%)
- [x] Theme matches existing zinc neutral dark palette
- [x] "Reset layout" button restores default arrangement
- [x] All existing functionality preserved: filters, infinite scroll, causal tree, investigate
- [x] URL params (seq, layers, q, from, to) continue to work
- [x] `PaneManager` is a reusable component in `src/components/` (not EventsPage-specific)

## Implementation Phases

### Phase 1: Foundation — PaneManager + Context

**Files:**

- `src/components/PaneManager.tsx` — reusable wrapper around flexlayout-react `<Layout>`
  - Props: `defaultLayout` (JSON), `paneRegistry` (map of component name → React component), `onModelChange?`
  - Renders `<Layout model={model} factory={factory} />`
  - Factory function maps `node.getComponent()` to registry entries
  - `onRenderTabSet` injects "+" button with pane type picker dropdown
  - Empty state handler when all tabs are closed

- `src/pages/events/EventsPaneContext.tsx` — shared state context
  - `selectedSeq`, `setSelectedSeq`
  - `investigateEvent`, `setInvestigateEvent`
  - `layers`, `toggleLayer`
  - `search`, `setSearch`
  - `timeFrom`, `setTimeFrom`, `timeTo`, `setTimeTo`
  - `fetchTree` (lazy query trigger)
  - `treeData`, `treeLoading`
  - Apollo queries live here (hoisted from EventsPage)

- Install `flexlayout-react` dependency

### Phase 2: Extract Pane Components

Extract the three content areas from `EventsPage.tsx` into standalone pane components that consume context:

- `src/pages/events/panes/TimelinePane.tsx`
  - Renders `<FilterBar>` + `<EventTimeline>` (both already exist as sub-components)
  - Reads filter state + event data from `EventsPaneContext`
  - Owns infinite scroll state (cursor, allEvents) locally — this is pane-instance-specific

- `src/pages/events/panes/CausalTreePane.tsx`
  - Renders `<CausalTreePanel>` (already exists)
  - Reads `treeData`, `treeLoading`, `selectedSeq` from context
  - Shows "Select an event" when `selectedSeq` is null
  - Auto-fetches tree on mount if `selectedSeq` is already set

- `src/pages/events/panes/InvestigatePane.tsx`
  - Wraps `<InvestigateDrawer>` (existing component)
  - Reads `investigateEvent` from context
  - `key={investigateEvent?.seq}` for remount on event change
  - Shows placeholder when `investigateEvent` is null
  - Internal close button **removed** — tab X is the close mechanism
  - Does NOT auto-fire investigation on mount if `investigateEvent` is null

### Phase 3: Wire Up EventsPage

- `src/pages/events/EventsPage.tsx` — rewrite to use PaneManager
  - Wraps everything in `<EventsPaneContext.Provider>`
  - Passes default layout JSON + pane registry to `<PaneManager>`
  - Investigate trigger: sets `investigateEvent` in context + programmatically adds Investigate tab if absent
  - `onModelChange`: detects Investigate tab removal → clears `investigateEvent`
  - "Reset layout" button in a small toolbar above the PaneManager
  - Preserves URL param sync for filters/selection

- `src/pages/events/defaultLayout.ts` — the default JSON model
  - Timeline (60%) + Causal Tree (40%) side by side
  - Exported as a constant

### Phase 4: Theme + Polish

- `src/index.css` — flexlayout-react CSS variable overrides
  - Map flexlayout-react's dark theme variables to zinc neutrals
  - Override tab bar styling (height, font size, hover/active states)
  - Resize handle styling to match existing `w-1.5 bg-border hover:bg-accent` convention
  - Drag indicator styling

- Verify `AdminLayout` compatibility
  - If needed, add `overflow-hidden` to `<main>` when PaneManager pages are active
  - Or handle via the page's own root div styling

- Tab close guard: if Investigate pane is streaming, show a small "investigation in progress" indicator on the tab (flexlayout-react supports custom tab rendering via `onRenderTab`)

## File Summary

| File | Action | Description |
|------|--------|-------------|
| `src/components/PaneManager.tsx` | Create | Reusable flexlayout-react wrapper |
| `src/pages/events/EventsPaneContext.tsx` | Create | Shared state context for events panes |
| `src/pages/events/panes/TimelinePane.tsx` | Create | Timeline pane (extracted from EventsPage) |
| `src/pages/events/panes/CausalTreePane.tsx` | Create | Causal tree pane (extracted from EventsPage) |
| `src/pages/events/panes/InvestigatePane.tsx` | Create | Investigate pane wrapper |
| `src/pages/events/defaultLayout.ts` | Create | Default layout JSON constant |
| `src/pages/EventsPage.tsx` | Rewrite | Wire PaneManager + context |
| `src/components/InvestigateDrawer.tsx` | Modify | Remove internal close button |
| `src/index.css` | Modify | Add flexlayout-react theme overrides |
| `package.json` | Modify | Add flexlayout-react dependency |

## Dependencies & Risks

- **flexlayout-react CSS conflicts**: Their CSS variables may collide with Tailwind v4. Scoping overrides to `.flexlayout__layout` should prevent this, but needs testing.
- **React 19 edge cases**: flexlayout-react claims React 19 support but may have subtle issues with concurrent features. Test drag-and-drop thoroughly.
- **InvestigateDrawer refactor**: Removing the close button and making it context-driven is a behavioral change. Must verify the streaming abort logic still fires correctly on tab close (component unmount).
- **AdminLayout overflow**: Changing `overflow-auto` to `overflow-hidden` on PaneManager pages could affect other content if the page has overflow beyond the PaneManager. Scoping this change carefully is important.

## References & Research

- Brainstorm: `docs/brainstorms/2026-03-03-pane-manager-brainstorm.md`
- [flexlayout-react GitHub](https://github.com/caplin/FlexLayout)
- [flexlayout-react npm](https://www.npmjs.com/package/flexlayout-react)
- Current EventsPage: `modules/admin-app/src/pages/EventsPage.tsx`
- Current InvestigateDrawer: `modules/admin-app/src/components/InvestigateDrawer.tsx`
- GraphExplorerPage (keeps react-resizable-panels): `modules/admin-app/src/pages/GraphExplorerPage.tsx`
