---
title: "feat: Add Clusters Layer to Map"
type: feat
date: 2026-02-15
---

# Add Clusters Layer to Map

## Overview

Add a new toggleable "Clusters" layer to the existing `/map` page that plots semantic signal clusters geographically. Each cluster marker is color-coded by dominant signal type, with filter controls for signal type, recency, confidence, radius, and topic. Clicking a cluster opens a summary card in the sidebar; from there users can drill into individual signals.

## Problem Statement / Motivation

The `/map` currently shows entities (organizations), density heatmaps, and coverage gaps — but not what those organizations are *doing*. Signals (ask/give/event/informative) represent real-time community activity. Clusters group related signals across sources into meaningful units. Plotting clusters on the map lets users see *where help is needed*, *where resources are offered*, and *where events are happening* — spatially.

## Proposed Solution

### Phase 1: Backend — GraphQL Cluster Query

**New files:**
- `modules/rootsignal-server/src/graphql/clusters/mod.rs` — `ClusterQuery` with `signal_clusters` resolver
- `modules/rootsignal-server/src/graphql/clusters/types.rs` — `GqlMapCluster`, `GqlSignalTypeCounts`

**Modified files:**
- `modules/rootsignal-server/src/graphql/mod.rs` — Register `ClusterQuery` in `QueryRoot`
- `modules/rootsignal-domains/src/clustering/models/cluster.rs` — Add `find_for_map()` query method

**GraphQL query shape:**

```graphql
query signalClusters(
  $signalType: String
  $since: String          # "24h" | "week" | "month"
  $minConfidence: Float
  $zipCode: String
  $radiusMiles: Float
  $about: String
  $limit: Int
) -> [GqlMapCluster!]!
```

**Return type `GqlMapCluster`:**

```graphql
type GqlMapCluster {
  id: UUID!
  latitude: Float!
  longitude: Float!
  memberCount: Int!
  dominantSignalType: String!     # "ask" | "give" | "event" | "informative"
  representativeContent: String!  # representative signal's content (truncated)
  representativeAbout: String     # representative signal's about field (used as "theme")
  signalCounts: GqlSignalTypeCounts!
  entityNames: [String!]!        # names of linked entities (for summary card)
}

type GqlSignalTypeCounts {
  ask: Int!
  give: Int!
  event: Int!
  informative: Int!
}
```

**SQL approach:** Single query joining `clusters` → `signals` (via `representative_id`) → `locationables` → `locations`, with a lateral subquery for signal type counts and entity names. Filter on representative signal's fields. Skip clusters where representative lacks location data.

**Key decisions:**
- **Theme** = representative signal's `about` field (no new column needed)
- **Dominant type** = computed server-side via `COUNT(*) GROUP BY signal_type` across cluster members
- **Confidence filter** = filters on representative signal's confidence
- **Recency filter** = `COALESCE(broadcasted_at, created_at)` on representative signal
- **About/topic filter** = `ILIKE '%term%'` on representative's `about` field (v1 simplicity)
- **Exclude singletons** = only return clusters with 2+ members (singletons are just individual signals)
- **Location fallback** = none; skip clusters without geocoded representative (add count indicator)

### Phase 2: Frontend — Map Layer

**Modified files:**
- `modules/admin-app/app/(app)/map/map-view.tsx` — Add clusters layer, filters, click handler

**Changes:**

1. **Extend `LayerMode`** (line 50):
   ```typescript
   type LayerMode = "density" | "gaps" | "entities" | "clusters";
   ```

2. **Add signal type color map:**
   ```typescript
   const SIGNAL_TYPE_COLORS: Record<string, string> = {
     ask: "#ef4444",         // red — urgency/need
     give: "#22c55e",        // green — positive/resource
     event: "#a855f7",       // purple — calendar
     informative: "#3b82f6", // blue — neutral/info
   };
   ```

3. **Add `loadClusters()` function** following `loadDensity`/`loadGaps` pattern:
   - Fetch via `gqlFetch` with filter params
   - Convert to GeoJSON with properties: `clusterId`, `memberCount`, `dominantSignalType`, `representativeContent`, `representativeAbout`
   - Add Mapbox source `"signal-clusters-source"` with `cluster: true` and `clusterMaxZoom: 12`
   - Add layers: `"signal-clusters-circles"` (sized by memberCount, colored by dominantSignalType), `"signal-clusters-labels"` (member count text)

4. **Add `"clusters"` case to `loadLayer`** (line 392)

5. **Add `"clusters"` to layer toggle buttons** (line 658)

6. **Add cluster legend** (following entities legend pattern at line 673) showing signal type colors

7. **Update `isFilterDisabled`** for clusters layer — enable: signalType (new), recency (new), confidence (new), zipCode, radiusMiles, about (new). Disable: entityType, signalDomain, category.

8. **Add new filter inputs** for clusters layer: signal type dropdown, recency dropdown (24h/week/month), confidence slider, about/topic text input. These are conditionally rendered when `layer === "clusters"`.

9. **Layer ID collision:** Use `"signal-clusters-source"`, `"signal-clusters-circles"`, `"signal-clusters-labels"` — add these to the cleanup list in `addHeatSources` (line 134).

10. **Click handler** (line 436): Add `"signal-clusters-circles"` to the listened layers. Disambiguate: if feature has `cluster_id` (Mapbox visual cluster) → zoom in. If feature has `clusterId` (semantic cluster) → open sidebar with `{ entityType: "cluster", entityId: clusterId }`.

11. **URL sync:** Add `signalType`, `since`, `confidence`, `about` to `syncUrl()` and initial state parsing.

### Phase 3: Frontend — Sidebar Cluster Mode

**Modified files:**
- `modules/admin-app/app/(app)/map/sidebar.tsx` — Add cluster detail mode

**Changes:**

1. **Add cluster branch** to sidebar rendering when `selectedPin.entityType === "cluster"`:
   - Fetch cluster detail via new `signalCluster(id:)` GraphQL query (returns full member signals)
   - Display: representative content as title/theme, signal count breakdown by type (colored badges), linked entity names as links
   - List first 10 member signals with type badge, truncated content, and click-to-navigate to `/signals/[id]`
   - "Show all" link if > 10 members

2. **New GraphQL query for sidebar:**

```graphql
query signalCluster($id: UUID!) -> GqlClusterDetail {
  id
  representativeSignal { id content about signalType confidence broadcastedAt }
  signals { id content signalType confidence broadcastedAt }
  entities { id name entityType }
}
```

This is a separate query from the map query — the map query returns lightweight data for markers, this returns full detail for the sidebar.

**New file:**
- `modules/rootsignal-server/src/graphql/clusters/mod.rs` — Add `signal_cluster(id:)` resolver alongside `signal_clusters` list resolver

## Acceptance Criteria

- [x] New "clusters" toggle appears in the map layer bar
- [x] Toggling to clusters shows color-coded markers on the map
- [x] Markers are colored by dominant signal type (ask=red, give=green, event=purple, informative=blue)
- [x] Marker size scales with member count
- [x] Filter controls appear for: signal type, recency, confidence, zip/radius, about/topic
- [x] Applying filters updates the map markers
- [x] Clicking a cluster marker opens a summary card in the sidebar
- [x] Summary card shows: theme (about), signal count by type, linked entities
- [x] Drilling into a signal from the summary card navigates to the signal detail
- [x] Filter state persists in URL for shareability
- [x] Singleton clusters (1 member) are excluded
- [x] Clusters without location data are excluded (with count indicator)
- [x] Legend shows signal type color mapping
- [x] Empty state message when no clusters match filters

## Technical Considerations

- **Mapbox layer ID collision**: Existing code uses `"clusters"` for Mapbox GL built-in clustering. New layer uses `"signal-clusters-*"` prefix.
- **Two-level clustering**: Mapbox visual clustering groups nearby semantic clusters at low zoom. Click on visual cluster → zoom in. Click on semantic cluster → sidebar. Disambiguate via `cluster_id` (Mapbox) vs `clusterId` (semantic) properties.
- **Performance**: Use `cluster: true` with `clusterMaxZoom: 12` on the GeoJSON source. Server-side `limit` parameter (default 500) prevents loading too many clusters.
- **Location coverage**: Not all cluster representatives have location data. The query filters these out. Consider adding a status indicator: "Showing 142 of 200 clusters (58 lack location data)".
- **HNSW index**: Geo-based filtering uses Haversine distance on `locations` table, not pgvector. No HNSW concerns for this query.

## Dependencies & Risks

- **Depends on clustering pipeline**: Clusters must exist in the DB. The clustering Restate job (`ClusteringJob`) must have run.
- **Location data quality**: If many representatives lack geocoding, the map will appear sparse. Mitigation: count indicator + future fallback to entity location.
- **Filter complexity**: Five filter dimensions on one query could produce slow SQL if not indexed. Mitigation: filter on representative signal fields only (single row), not across all members.

## References & Research

### Internal References
- Map view component: `modules/admin-app/app/(app)/map/map-view.tsx`
- Map sidebar: `modules/admin-app/app/(app)/map/sidebar.tsx`
- Cluster model: `modules/rootsignal-domains/src/clustering/models/cluster.rs`
- ClusterItem model: `modules/rootsignal-domains/src/clustering/models/cluster_item.rs`
- Clustering algorithm: `modules/rootsignal-domains/src/clustering/activities/cluster_listings.rs`
- GraphQL schema root: `modules/rootsignal-server/src/graphql/mod.rs`
- HeatMap query (pattern to follow): `modules/rootsignal-server/src/graphql/heat_map/mod.rs`
- Signal types: `modules/rootsignal-server/src/graphql/signals/types.rs`
- Cluster migrations: `migrations/020_clusters.sql`, `migrations/059_signal_clustering.sql`

### Institutional Learnings
- Use `next/dynamic` + `ssr: false` for Mapbox components (not react-map-gl)
- Layer toggles should be mutually exclusive radio-style, not checkboxes
- Use `NOT EXISTS` instead of `NOT IN` for unclustered queries (scalability)
- Representative selection should include `broadcasted_at DESC NULLS LAST` to avoid stale event representatives
- False positives (wrong groupings) are worse than false negatives (missed clusters) — keep thresholds conservative
- Clean up `cluster_items` rows on signal deletion (no FK cascade on polymorphic UUID)
