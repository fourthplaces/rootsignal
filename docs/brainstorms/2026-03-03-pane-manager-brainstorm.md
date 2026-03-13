---
date: 2026-03-03
topic: pane-manager
---

# Rearrangeable Pane Layout (iTerm2-style)

## What We're Building

A reusable tiling pane manager for the admin app, starting on the `/events` page. Users can drag panes to rearrange them, split horizontally or vertically, and add/remove pane types from a picker. The UX mirrors iTerm2's pane management: grab a pane tab, drag it to a new position (left, right, top, bottom of any existing pane), and the layout reflows.

## Why flexlayout-react

Three approaches were considered:

1. **flexlayout-react** (chosen) — mature tiling WM with drag, split, add/remove built-in via a JSON layout model. ~30KB gzipped. Battle-tested in VS Code-style UIs.
2. **DIY on react-resizable-panels** — would require building a custom layout tree, split/merge logic, and drag target zones from scratch. Too much custom code for the payoff.
3. **react-mosaic** — simpler but less actively maintained and fewer features (no tab groups).

flexlayout-react gives us the full feature set with minimal custom code and a JSON model that makes future persistence (localStorage/URL) trivial.

## Key Decisions

- **Reusable component**: Build a generic `PaneManager` that accepts a registry of pane types. Any admin page can use it, not just `/events`.
- **Three pane types for v1**: Timeline, Causal Tree, Investigate. New types (map, graph, stats) added later via the registry.
- **No persistence yet**: Layout resets to default on page load. JSON model makes adding localStorage persistence a single `onModelChange` handler later.
- **Replaces react-resizable-panels on /events**: The flexlayout-react model subsumes what react-resizable-panels does. Other pages can still use react-resizable-panels if they don't need the full tiling UX.
- **Pane picker (+)**: A button in the layout toolbar to add new pane instances from the registry.
- **Shared state via context**: Selected event, investigate event, filters etc. shared via React context so panes communicate without prop drilling.

## Open Questions

- Should there be preset layouts (e.g. "Timeline + Tree", "Investigation Focus") accessible from a dropdown? (deferred)
- Should panes of the same type be duplicable (e.g. two timeline panes with different filters)? (deferred, but the architecture should not prevent it)

## Next Steps

→ `/workflows:plan` for implementation details
