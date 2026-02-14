# Plan: Align Codebase to Full Signal Taxonomy

## Context

The docs define 10 audience roles, 35+ signal types across 4 domains, 50+ categories, and a rich data model. The code implements only 5 of 10 roles, 11 of 35+ signal types, 20 of 50+ categories. The extraction prompt doesn't know about ecological, civic, consumer, or educational signal.

**Design principle: Taproot is infrastructure, not business logic.** All taxonomy/classification lives in the **tag system** (tag_kinds + tags + taggables). The listings table stays lean — only computed/temporal fields that need indexing. This means adding a new classification dimension (like "population" or "radius_relevant") requires zero schema migrations — just seed a tag_kind and its tag values.

Patterns ported from mntogether:
- **Dynamic tag instructions** — build AI prompt taxonomy from the database
- **tag_kinds table** — configurable taxonomy dimensions
- **Restate services** — `#[restate_sdk::service]` for all listing/tag query operations

---

## Phase 1: Database Migrations

### Migration 016: Minimal listings extensions
**New file:** `migrations/016_extend_listings.sql`

Only add **computed/temporal** columns that need indexing:
- `expires_at` TIMESTAMPTZ — for expiration queries
- `freshness_score` REAL DEFAULT 1.0 — for freshness decay
- `relevance_score` INTEGER — composite 1-10
- `relevance_breakdown` TEXT — human-readable reasoning

Indexes on expires_at, relevance_score.

Everything else (urgency, confidence, capacity_status, signal_domain, radius_relevant, population) lives in tags.

### Migration 017: Add tag_kinds table + seed full taxonomy
**New file:** `migrations/017_tag_kinds_and_full_taxonomy.sql`

Port `tag_kinds` from mntogether — this powers dynamic tag instructions and makes taxonomy self-describing:
```sql
CREATE TABLE IF NOT EXISTS tag_kinds (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    description TEXT,
    allowed_resource_types TEXT[] NOT NULL DEFAULT '{}',
    required BOOLEAN NOT NULL DEFAULT FALSE,
    is_public BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

**Seed tag_kinds** (all for "listing" resource type):

| slug | display_name | required | description |
|------|-------------|----------|-------------|
| `listing_type` | Signal Type | yes | Classification of the signal |
| `audience_role` | Audience Role | yes | Who can act on this |
| `category` | Category | no | Subject area |
| `signal_domain` | Signal Domain | no | Broad domain grouping |
| `urgency` | Urgency | no | Time sensitivity |
| `confidence` | Confidence | no | Extraction confidence level |
| `capacity_status` | Capacity | no | Current capacity status |
| `radius_relevant` | Geographic Scope | no | How far this signal carries |
| `population` | Population Served | no | Target populations |

**Seed all missing tag values** via `ON CONFLICT DO NOTHING`:

- **Audience roles (+5):** skilled_professional, citizen_scientist, land_steward, conscious_consumer, educator
- **Listing types (+~30):** ecological (12), civic (5), human services (9), knowledge (5)
- **Categories (+~40):** ecological (19), civic (12), crisis (9), human needs gaps (6)
- **Signal domains:** human_services, ecological_stewardship, civic_economic, knowledge_awareness (fix misnamed)
- **Urgency:** immediate, this_week, this_month, ongoing, flexible
- **Confidence:** high, medium, low
- **Capacity status:** accepting, limited, at_capacity, unknown
- **Radius relevant:** neighborhood, city, metro, region, national, global
- **Population:** youth, seniors, families, immigrants, refugees, veterans, unhoused, disabled, lgbtq, indigenous

---

## Phase 2: Dynamic Tag Instructions (port from mntogether)

### Add TagKindConfig model
**New file:** `modules/taproot-domains/src/entities/models/tag_kind.rs`

Port from mntogether `packages/server/src/domains/tag/models/tag_kind_config.rs`:
- `TagKindConfig` struct with `find_all`, `find_for_resource_type`, `tag_count_for_slug`
- `build_tag_instructions(pool) -> String` — queries tag_kinds for "listing" resource type, loads all tag values per kind, formats as prompt instructions
- Register in `entities/models/mod.rs`, re-export from `entities/mod.rs`

Output format:
```
- **listing_type** (required): Signal type — Pick from: "volunteer_opportunity", "habitat_restoration", ...
- **audience_role** (required): Who can act — Pick from: "volunteer", "donor", ...
- **category**: Subject area — Pick from: "food_security", "water_quality", ...
- **urgency**: Time sensitivity — Pick from: "immediate", "this_week", ...
- **confidence**: Your confidence — Pick from: "high", "medium", "low"
- **capacity_status**: Current capacity — Pick from: "accepting", "limited", ...
```

---

## Phase 3: Type System Updates

### Update ExtractedListing struct
**File:** `modules/taproot-core/src/types.rs`

Add fields (all Optional):
- `signal_domain: Option<String>`
- `urgency: Option<String>`
- `capacity_status: Option<String>`
- `confidence_hint: Option<String>`
- `radius_relevant: Option<String>`
- `expires_at: Option<String>` — ISO 8601
- `populations: Option<Vec<String>>`

### Update Listing + ListingDetail structs
**File:** `modules/taproot-domains/src/listings/models/listing.rs`

Add only the new DB columns: `expires_at`, `freshness_score`, `relevance_score`, `relevance_breakdown`. Update `ListingDetail::find_active` SELECT.

---

## Phase 4: Extraction Prompt

### Replace SYSTEM_PROMPT with dynamic taxonomy
**File:** `modules/taproot-domains/src/extraction/activities/extract.rs`

Replace hardcoded `SYSTEM_PROMPT` with:
1. **Static preamble** — extraction role, rules, output format, field definitions
2. **Dynamic taxonomy** — injected via `build_tag_instructions(pool)` at runtime

The static preamble defines the shape of the output (ExtractedListing fields) and rules (only actionable listings, one call-to-action per listing, never fabricate). The dynamic section provides all available tag values by kind — automatically staying in sync with the database.

---

## Phase 5: Normalization Updates

### Persist new fields + all tags
**File:** `modules/taproot-domains/src/extraction/activities/normalize.rs`

1. Update listings INSERT to bind: `expires_at`, `freshness_score` (default 1.0)
2. Compute `expires_at` from explicit value or fall back to timing_end
3. **Tag everything via Taggable::tag()** — uniform approach for ALL taxonomy dimensions:
   - `listing_type` (already done)
   - `category` (already done)
   - `audience_role` (already done)
   - `signal_domain` (NEW)
   - `urgency` (NEW — was stored as a note, now a proper tag)
   - `confidence` (NEW)
   - `capacity_status` (NEW)
   - `radius_relevant` (NEW)
   - `population` (NEW — loop over populations vec)

---

## Phase 6: Restate Services for Listings & Tags

Following mntogether's pattern: all business logic in `#[restate_sdk::service]`.

### ListingsService
**New file:** `modules/taproot-domains/src/listings/restate/mod.rs`

```rust
#[restate_sdk::service]
#[name = "Listings"]
pub trait ListingsService {
    async fn list(req: ListListingsRequest) -> Result<ListingListResult, HandlerError>;
    async fn filters(req: EmptyRequest) -> Result<ListingFiltersResult, HandlerError>;
    async fn stats(req: EmptyRequest) -> Result<ListingStatsResult, HandlerError>;
    async fn score_batch(req: ScoreBatchRequest) -> Result<ScoreBatchResult, HandlerError>;
    async fn expire_stale(req: EmptyRequest) -> Result<ExpireResult, HandlerError>;
}
```

**`ListListingsRequest`** — all filters as Option:
- Tag-based: `signal_domain`, `audience_role`, `category`, `listing_type`, `urgency`, `confidence`, `capacity_status`, `radius_relevant`, `population`
- Geo: `lat`, `lng`, `radius_km`, `hotspot_id`
- Temporal: `since`
- Pagination: `limit`, `offset`

**All tag-based filters use the same query pattern:**
```sql
EXISTS (
  SELECT 1 FROM taggables tg
  JOIN tags t ON t.id = tg.tag_id
  WHERE tg.taggable_type = 'listing'
    AND tg.taggable_id = l.id
    AND t.kind = $kind
    AND t.value = $value
)
```

This means adding a new filter dimension requires zero code changes to the query — just add the parameter.

### TagsService
**New file:** `modules/taproot-domains/src/entities/restate/tags.rs`

Port from mntogether:
```rust
#[restate_sdk::service]
#[name = "Tags"]
pub trait TagsService {
    async fn list_kinds(req: EmptyRequest) -> Result<TagKindListResult, HandlerError>;
    async fn list_tags(req: ListTagsRequest) -> Result<TagListResult, HandlerError>;
    async fn create_tag(req: CreateTagRequest) -> Result<TagResult, HandlerError>;
}
```

### Model: Listing::find_filtered()
**File:** `modules/taproot-domains/src/listings/models/listing.rs`

`sqlx::QueryBuilder` with:
- Tag filters: uniform EXISTS subqueries (see above)
- Geo: Haversine via locations JOIN
- Hotspot: subquery for center/radius
- Temporal: created_at >= since, expires_at filter
- Order: relevance_score DESC NULLS LAST, created_at DESC

### Hotspot::find_by_id
**File:** `modules/taproot-domains/src/entities/models/hotspot.rs`

### Register services
**File:** `modules/taproot-server/src/main.rs`

```rust
.bind(taproot_domains::listings::ListingsServiceImpl::with_deps(server_deps.clone()).serve())
.bind(taproot_domains::entities::TagsServiceImpl::with_deps(server_deps.clone()).serve())
```

---

## Phase 7: Assessment Page Updates

**File:** `modules/taproot-server/src/routes.rs`

Axum assessment page stays as direct DB queries (internal tool). Add breakdowns by:
- Signal domain (tag kind)
- Urgency distribution (tag kind)
- Confidence distribution (tag kind)
- Capacity status (tag kind)
- Relevance score histogram

---

## Files Modified

| File | Change |
|------|--------|
| `migrations/016_extend_listings.sql` | **NEW** — expires_at, freshness_score, relevance_score, relevance_breakdown |
| `migrations/017_tag_kinds_and_full_taxonomy.sql` | **NEW** — tag_kinds table + all tags (9 kinds, ~120 values) |
| `modules/taproot-domains/src/entities/models/tag_kind.rs` | **NEW** — TagKindConfig + build_tag_instructions |
| `modules/taproot-domains/src/entities/models/mod.rs` | Register tag_kind module |
| `modules/taproot-domains/src/entities/mod.rs` | Re-export TagKindConfig |
| `modules/taproot-domains/src/entities/restate/tags.rs` | **NEW** — TagsService (Restate service) |
| `modules/taproot-domains/src/entities/restate/mod.rs` | **NEW** — register tags module |
| `modules/taproot-domains/src/listings/restate/mod.rs` | **NEW** — ListingsService (Restate service) |
| `modules/taproot-domains/src/listings/mod.rs` | Register restate module |
| `modules/taproot-core/src/types.rs` | Add 7 fields to ExtractedListing |
| `modules/taproot-domains/src/extraction/activities/extract.rs` | Dynamic prompt via build_tag_instructions |
| `modules/taproot-domains/src/extraction/activities/normalize.rs` | Tag all dimensions via Taggable::tag() |
| `modules/taproot-domains/src/listings/models/listing.rs` | find_filtered, update structs |
| `modules/taproot-domains/src/entities/models/hotspot.rs` | Add find_by_id |
| `modules/taproot-server/src/main.rs` | Register ListingsService + TagsService |
| `modules/taproot-server/src/routes.rs` | Assessment page with tag-based breakdowns |

---

## Verification

1. `cargo build` — compiles
2. Run migrations, verify: `SELECT kind, COUNT(*) FROM tags GROUP BY kind` shows 9 kinds with ~120 total values
3. Verify `SELECT * FROM tag_kinds` shows 9 configured kinds
4. `build_tag_instructions(pool)` output includes all seeded tags grouped by kind
5. Extract an ecological source — verify signal_domain, urgency, confidence all land as tags (not columns)
6. `curl -X POST http://localhost:8080/Listings/list -d '{"signal_domain":"ecological_stewardship"}'`
7. `curl -X POST http://localhost:8080/Listings/filters -d '{}'` — returns all tag kinds with counts
8. `curl -X POST http://localhost:8080/Tags/list_kinds -d '{}'`
9. Assessment page shows breakdowns by all 9 tag kinds
