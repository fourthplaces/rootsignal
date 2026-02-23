---
date: 2026-02-23
topic: scout-pipeline-testing
---

# Scout Pipeline Testing

## What We're Building

A testing architecture where every organ of Scout functions in isolation with clear input/output contracts, and the boundaries between organs are verified to ensure correct communication. Given fixed inputs, each organ produces predictable outputs. LLM calls live at the edges behind mockable traits; everything between those edges is deterministic.

## Why

Scout's trust model depends on correctness. Dedup-as-corroboration means a missed duplicate inflates trust metrics; a false duplicate silences real signal. A wrong canonical_value creates phantom source duplicates. A blind region-center stamp on promoted sources contaminates cross-region discovery. Right now, testing most of Scout requires the full stack. We need fast, precise, no-infrastructure tests for each organ — and boundary tests that verify the organs communicate correctly.

---

## The Organs

Six organs with clear input/output contracts:

```
Fetcher ──markdown──→ Extractor ──signals──→ Signal Processor ──writes──→ Graph
   │                                              │
   ├──page links──→ Link Discoverer ──sources──→ Graph (next run)
   │                                              │
   │                                    Actor Resolver ──actors──→ Graph
   │
Embedder ──vectors──→ Signal Processor (dedup decisions)
```

### 1. Fetcher

**Input:** URL (web page, social account, search query, RSS feed)
**Output:** Content — `ArchivedPage` (markdown + links), `Vec<Post>`, `ArchivedFeed`, `ArchivedSearchResults`
**Currently:** `Archive` (concrete struct). ScrapePhase holds `Arc<Archive>`.

The single I/O boundary between Scout and the outside world. Every web page, social media post, search result, and feed comes through it.

**Trait abstraction:**

```rust
#[async_trait]
pub trait ContentFetcher: Send + Sync {
    async fn page(&self, url: &str) -> Result<ArchivedPage>;
    async fn feed(&self, url: &str) -> Result<ArchivedFeed>;
    async fn posts(&self, url: &str, limit: u32) -> Result<Vec<Post>>;
    async fn search(&self, query: &str) -> Result<ArchivedSearchResults>;
    async fn search_topics(&self, platform_url: &str, topics: &[&str], limit: u32) -> Result<Vec<Post>>;
    async fn site_search(&self, query: &str, max_results: usize) -> Result<ArchivedSearchResults>;
}
```

6 methods. All return plain owned structs. Collapses the `source() → SourceHandle` chain into direct methods (the two `source()` call sites in ScrapePhase each use SourceHandle for exactly one follow-up call).

**Mock:**

```rust
let fetcher = MockFetcher::new()
    .on_search("site:linktr.ee mutual aid Minneapolis", vec![
        SearchResult { url: "https://linktr.ee/mplsmutualaid".into(), .. },
    ])
    .on_page("https://linktr.ee/mplsmutualaid", ArchivedPage {
        markdown: "Minneapolis Mutual Aid - Links".into(),
        links: vec![
            "https://instagram.com/mplsmutualaid".into(),
            "https://gofundme.com/f/help-families?utm_source=linktree".into(),
            "https://docs.google.com/document/d/ABC123/edit?usp=sharing".into(),
            "https://fonts.googleapis.com/css2?family=Inter".into(),
        ],
    });
```

HashMap lookup. Same URL always returns same content. Deterministic.

---

### 2. Extractor

**Input:** Markdown content + source URL
**Output:** Structured signals — `Vec<Node>` (with metadata: type, location, actors, links, schedule, etc.) + `Vec<(Uuid, Vec<ResourceTag>)>` + `Vec<(Uuid, Vec<String>)>` signal tags
**Currently:** `SignalExtractor` trait (already exists). Concrete `Extractor` wraps Claude Haiku.

This is where raw web content becomes structured signal. If extraction is wrong — wrong type, wrong location, wrong actors — everything downstream is garbage.

**Already mockable** via the existing trait. For organ-level testing, use the snapshot/replay infrastructure that already exists (fixture markdown → real LLM → save JSON snapshot → replay in CI without LLM calls).

**Deterministic vs. fuzzy assertion split:**

| Field | Assertion | Why |
|-------|-----------|-----|
| `node_type` | **Exact** | Discrete classification |
| `location.lat/lng` | **Exact** (epsilon) | Geocoding is stable |
| `location_name` | **Exact** | Place names are in the text |
| `mentioned_actors` | **Exact** (set) | Names are in the text |
| `author_actor` | **Exact** | Attribution is in the text |
| `source_links` | **Exact** (set) | URLs are in the text |
| `starts_at/ends_at/schedule` | **Exact** | Dates are in the text |
| `is_firsthand` | **Exact** | Binary classification |
| `implied_queries` | **Loose** (non-empty for Tension/Need) | Specific queries vary |
| `title` | **Loose** (contains key terms) | Phrasing varies |
| `summary/description` | **Loose** (non-empty) | Free text |

**Edge case fixtures to add** (beyond the existing 20+):

- Multi-signal page: community board with events, needs, resources → 3+ signals
- Ambiguous geography: content mentions multiple cities
- Adversarial: satirical content, clickbait, AI-generated spam
- Non-English: Spanish, Somali, Hmong (Twin Cities relevant)
- Stale content: past dates → correct `ends_at`
- Minimal content: barely enough for a signal → low confidence

**Actor geo resolution scenarios** (extraction-level — these test the Extractor organ, not downstream processing):

```
// Actor bio "Portland, OR" but post about restaurant in "Portland, ME"
→ signal location should be Portland ME, not Portland OR

// Actor bio "NYC", generic opinion piece with no location cues
→ signal has no geo (actor fallback is downstream, not extractor's job)

// Actor bio "Springfield" (no state), post about Springfield event
→ disambiguation depends on region context — test documents actual behavior

// Actor bio "Bay Area" → approximate SF coords, not a precise point

// "Based in Denver, traveling in Austin"
→ signal location = Austin (where the content is about)

// Single post mentions events in Dallas AND Houston
→ should produce two signals or pick the primary — test documents behavior

// "Worldwide" / "Everywhere" → no specific geo
// Non-US country → kept (Berlin is an active city)
```

---

### 3. Signal Processor

**Input:** Extracted signals (Vec<Node>) + embeddings (from Embedder) + prior graph state (existing titles, existing embeddings, existing nodes)
**Output:** Graph writes — create new nodes, corroborate existing ones (increase source_diversity), refresh timestamps, wire evidence trails, link actors, tag resources
**Currently:** `store_signals()` in `scrape_phase.rs` (~700 lines). Holds `GraphWriter` (concrete).

This is the core of Scout — the trust mechanism. It has tight read-write interleaving: each dedup read gates which write happens. This is architecturally correct. We don't restructure it into a pipeline. Instead, we:

1. Put GraphWriter behind a trait so the organ can run without Neo4j
2. Extract the decision logic into pure functions so we can test the organ's brain without its hands

**Trait abstraction:**

```rust
#[async_trait]
pub trait SignalStore: Send + Sync {
    // URL/content guards
    async fn blocked_urls(&self, urls: &[String]) -> Result<HashSet<String>>;
    async fn content_already_processed(&self, hash: &str, url: &str) -> Result<bool>;

    // Signal lifecycle
    async fn create_node(&self, node: &Node, embedding: &[f32], created_by: &str, run_id: &str) -> Result<Uuid>;
    async fn create_evidence(&self, evidence: &EvidenceNode, signal_id: Uuid) -> Result<()>;
    async fn refresh_signal(&self, id: Uuid, node_type: NodeType, now: DateTime<Utc>) -> Result<()>;
    async fn refresh_url_signals(&self, url: &str, now: DateTime<Utc>) -> Result<u64>;
    async fn corroborate(&self, id: Uuid, node_type: NodeType, now: DateTime<Utc>, mappings: &[EntityMappingOwned]) -> Result<()>;

    // Dedup queries
    async fn existing_titles_for_url(&self, url: &str) -> Result<Vec<String>>;
    async fn find_by_titles_and_types(&self, pairs: &[(String, NodeType)]) -> Result<HashMap<(String, NodeType), (Uuid, String)>>;
    async fn find_duplicate(&self, embedding: &[f32], primary_type: NodeType, threshold: f64, min_lat: f64, max_lat: f64, min_lng: f64, max_lng: f64) -> Result<Option<DuplicateMatch>>;

    // Actor graph
    async fn find_actor_by_name(&self, name: &str) -> Result<Option<Uuid>>;
    async fn upsert_actor(&self, actor: &ActorNode) -> Result<()>;
    async fn link_actor_to_signal(&self, actor_id: Uuid, signal_id: Uuid, role: &str) -> Result<()>;

    // Resource graph
    async fn find_or_create_resource(&self, name: &str, slug: &str, desc: &str, embedding: &[f32]) -> Result<Uuid>;
    async fn create_requires_edge(&self, signal_id: Uuid, resource_id: Uuid, confidence: f32, quantity: Option<&str>, notes: Option<&str>) -> Result<()>;
    async fn create_prefers_edge(&self, signal_id: Uuid, resource_id: Uuid, confidence: f32) -> Result<()>;
    async fn create_offers_edge(&self, signal_id: Uuid, resource_id: Uuid, confidence: f32, capacity: Option<&str>) -> Result<()>;

    // Source management
    async fn get_active_sources(&self) -> Result<Vec<SourceNode>>;
    async fn upsert_source(&self, source: &SourceNode) -> Result<()>;
    async fn batch_tag_signals(&self, signal_id: Uuid, tag_slugs: &[String]) -> Result<()>;
}
```

20 methods. All return scalars/simple types.

**Mock:** Stateful in-memory graph — `MockSignalStore` with HashMaps for nodes, actors, resources. Must correctly simulate the dedup state machine (find_by_titles_and_types returns pre-populated matches that drive the create/corroborate/refresh decision).

**Internal extractions** (pure decision functions pulled out of store_signals):

**`dedup_verdict`** — The most valuable extraction. Separates the dedup **decision** from its **execution**:

```rust
enum DedupVerdict {
    Create,
    Corroborate { existing_id: Uuid, existing_url: String, similarity: f64 },
    Refresh { existing_id: Uuid, similarity: f64 },
}

fn dedup_verdict(
    node: &Node,
    source_url: &str,
    embedding: &[f32],
    global_matches: &HashMap<(String, NodeType), (Uuid, String)>,
    embed_cache: &EmbeddingCache,
    graph_duplicate: Option<DuplicateMatch>,
) -> DedupVerdict
```

**`score_and_filter`** — Quality scoring + geo filtering + actor location fallback. Already nearly pure:

```rust
fn score_and_filter(
    nodes: Vec<Node>,
    url: &str,
    geo_config: &GeoFilterConfig,
    actor_ctx: Option<&ActorContext>,
) -> (Vec<Node>, GeoFilterStats)
```

**`batch_title_dedup`** — Within-batch dedup by (normalized_title, node_type):

```rust
fn batch_title_dedup(nodes: Vec<Node>) -> Vec<Node>
```

These are testable with zero infrastructure — pure inputs, pure outputs, no async.

---

### 4. Link Discoverer

**Input:** Page links (Vec<String>) from Fetcher output
**Output:** Promoted SourceNodes for next run's schedule
**Currently:** `extract_links()` → `promote_links()` in `link_promoter.rs`

Discovers new sources from outbound links on scraped pages. The chain: raw HTML links → filter junk (schemes, extensions) → strip tracking params → deduplicate by canonical_value → create SourceNodes.

**Already partially testable** — `extract_links()` and the helper functions are pure. `promote_links()` is the I/O boundary (calls `writer.upsert_source()`). With the `SignalStore` trait from Organ 3, the whole chain is testable.

**Known bug:** `promote_links` stamps every promoted source with the discovering region's center coords, regardless of where the linked content actually is.

---

### 5. Actor Resolver

**Input:** Signal metadata (`mentioned_actors`, `author_actor`) or signal summaries (for batch LLM extraction)
**Output:** Actor nodes + ACTED_IN edges in graph
**Currently:** Two disconnected paths:
- **Path A:** Inline in `store_signals` — processes actor names from extraction metadata during signal storage
- **Path B:** `actor_extractor.rs` — batch LLM extraction over signals with no ACTED_IN edges (runs during synthesis)

Both use `find_actor_by_name` (exact string match). Neither has tests. Both stamp new actors with region-center coords.

The two paths doing the same job without knowing about each other is a design smell — but testing them individually first will reveal whether they should be unified.

---

### 6. Embedder

**Input:** Text
**Output:** Vector (Vec<f32>, 1024-dim)
**Currently:** `TextEmbedder` trait (already exists). Concrete `Embedder` wraps Voyage AI.

Already mockable. For tests: `FixedEmbedder` that returns deterministic vectors from a lookup table, so dedup threshold tests are reproducible.

---

## Boundary Tests

These verify organs communicate correctly — that the output of one organ is correctly consumed by the next.

### Fetcher → Extractor

"Given this page content, does the extractor produce the right signals?"

```
fetcher.page("https://example.com/food-shelf") → ArchivedPage { markdown: fixture_content }
extractor.extract(fixture_content, "https://example.com/food-shelf") → [Aid node with correct fields]
```

Tested via extraction snapshots (Organ 2 testing). The boundary data is `ArchivedPage.markdown` — a plain string.

### Fetcher → Link Discoverer

"Given this page's links, does the discoverer promote the right sources?"

```
fetcher.page("https://linktr.ee/mplsmutualaid") → ArchivedPage {
    links: [
        "https://instagram.com/mplsmutualaid",
        "https://gofundme.com/f/help-families?utm_source=linktree",
        "https://docs.google.com/document/d/ABC123/edit?usp=sharing",
        "https://eventbrite.com/e/food-distro-12345",
        "https://amazon.com/hz/wishlist/ls/XYZ?ref_=cm_wl_huc_do",
        "https://discord.gg/mutualaid",
        "https://fonts.googleapis.com/css2?family=Inter",
        "https://cdn.jsdelivr.net/npm/bootstrap",
        "https://googletagmanager.com/gtag.js",
    ]
}

// After extract_links:
→ instagram link kept, tracking stripped from GoFundMe/Docs/Amazon
→ fonts, cdn, gtag filtered
→ discord kept (communication channel)

// After promote_links:
→ instagram.com/mplsmutualaid → SourceNode with canonical_value "instagram.com/mplsmutualaid"
→ GoFundMe → SourceNode, url tracking params stripped
→ fonts.googleapis.com → NOT promoted

// Dedup at boundary:
// Two Linktree pages both link to same Instagram
→ canonical_value deduplicates → one source, not two

// Volume control: page with 50+ links → max_per_source cap (20)
```

### Extractor → Signal Processor

"Given these extracted signals, what gets stored/corroborated/refreshed?"

```
extractor produces: [Aid("Free Legal Clinic", location=(44.97, -93.27))]
signal processor receives these nodes + embeddings from Embedder

// New signal — no prior state
→ create_node called, create_evidence called

// Same title exists from different source
→ corroborate called (source_diversity increases), create_evidence called

// Same title exists from same source
→ refresh_signal called (timestamp updated, no corroboration inflation)

// Corroboration decay: recurring event with same title
// "Community Garden Cleanup" scraped in March (starts_at: March 15)
// "Community Garden Cleanup" scraped in June (starts_at: June 20)
// Title+type dedup matches → currently Corroborates (time-blind)
// Should be: Create (different event instance)
// Test documents this gap — dedup_verdict needs temporal awareness for Gatherings
```

### Extractor → Actor Resolver (via Signal Processor)

"When signals have actor metadata, are the right actors created/linked?"

```
extractor produces node with:
  mentioned_actors: ["Open Arms MN", "Second Harvest Heartland"]
  author_actor: Some("Northside Mutual Aid")
  location: GeoPoint { lat: 45.01, lng: -93.28 }

// Signal Processor stores the signal, then Actor Resolver runs:
→ 3 actors created (or found if existing)
→ "Northside Mutual Aid" linked with role "authored"
→ "Open Arms MN" + "Second Harvest Heartland" linked with role "mentioned"
→ all actors get signal's location (45.01, -93.28), not region center

// Actor reuse across boundary:
// Second signal from different source also mentions "Open Arms MN"
→ find_actor_by_name returns existing → reuse, new ACTED_IN edge only

// Known gap at this boundary:
// "Simpson Housing" and "Simpson Housing Services" → two actors (exact match only)
// Test documents this for future fuzzy matching
```

### Embedder → Signal Processor (dedup decisions)

"Given these vectors, does the processor make the right dedup verdict?"

```
// Two signals with different titles but similar content
embedder returns vectors with cosine similarity 0.93
signals from different sources
→ DedupVerdict::Corroborate (above 0.92 cross-source threshold)

// Same content from same source
embedder returns vectors with cosine similarity 0.87
signals from same URL
→ DedupVerdict::Refresh (above 0.85, same source)

// Genuinely different content
embedder returns vectors with cosine similarity 0.60
→ DedupVerdict::Create

// Corroboration decay: same title, same type, but different time window
// "Community Garden Cleanup" in March vs "Community Garden Cleanup" in June
// Title+type match says Corroborate, but starts_at is 3 months apart → different events
// Currently: title dedup is time-blind — this would incorrectly Corroborate
// Test documents the gap; fix requires dedup_verdict to consider starts_at/ends_at
// when present on both the new node and the existing match
```

### Signal Processor + Actor Resolver: Location Handoff

"How does actor location interact with signal location across the boundary?"

```
// Actor in MN, signal explicitly about Texas
actor_ctx = { location_lat: 44.97, location_lng: -93.27 }  // MN
signal has explicit location (32.78, -96.80)  // Dallas
geo_config centered on Minneapolis
→ signal is FILTERED (outside radius) — actor fallback does NOT override explicit coords

// Actor in MN, signal has no location
actor_ctx = { location_lat: 44.97, location_lng: -93.27 }
signal has no location
→ actor coords applied as fallback → signal gets (44.97, -93.27), survives geo filter

// No actor context, signal has no location
→ signal has no coords, survives only if geo_terms match

// Actor location from signal: new actor gets signal's coords
signal.location = Some(GeoPoint { lat: 45.01, lng: -93.28 })
→ new actor.location_lat = 45.01 (not region center)

// Actor location without signal coords: new actor gets region center
signal.location = None
→ new actor.location_lat = region.center_lat (known imprecision, test documents it)
```

### Link Discoverer: Source Location Bug (TDD)

"Does promote_links correctly handle cross-region content?"

```
// TDD: tests that SHOULD FAIL today
promote_links([("https://gofundme.com/f/texas-relief", "https://linktr.ee/mpls-org")])
  with region center (44.97, -93.27)
  → source.center_lat should NOT be 44.97 (it's a Texas campaign)
  → today it IS 44.97 — that's the bug

// Cross-region org discovered from local Linktree
// Minneapolis scout finds Atlanta org's website
→ source should NOT get Minneapolis coords

// National org (mutualaid.org) from local page
→ source should have no specific geo, or be deferred until scraped

// Cascading contamination
// Source A (tagged Minneapolis) links to Source B (actually Atlanta)
→ Source B should not inherit Minneapolis coords
```

---

## Chain Tests

Boundary tests verify one handoff at a time. Chain tests verify **sub-chains of the pipeline end-to-end** — multiple organs wired together with mocked I/O, asserting the complete output. This is where we test how organs compose: discovery loops, multi-hop fetching, corroboration across sources.

All chain tests use:
- `MockFetcher` — HashMap of URL → canned `ArchivedPage` / `ArchivedSearchResults` / `Vec<Post>`
- `MockExtractor` — HashMap of URL → canned `Vec<Node>` (or use the real extractor with snapshot replay)
- `MockSignalStore` — stateful in-memory graph that tracks all creates, corroborates, promotes, actor links
- `FixedEmbedder` — deterministic vectors from a lookup table

Deterministic. No network, no LLM, no Docker. Runs in `cargo test`.

### Chain 1: Discovery Loop

"Search → fetch results → extract links → promote sources"

The core link discovery chain. Tests that `ScrapePipeline` correctly fans out from search results to page fetches to link promotion.

```rust
let fetcher = MockFetcher::new()
    // Search returns Linktree URLs
    .on_search("site:linktr.ee mutual aid Minneapolis", vec![
        SearchResult { url: "https://linktr.ee/mplsmutualaid", .. },
        SearchResult { url: "https://linktr.ee/northsideaid", .. },
    ])
    // Each Linktree page has its own links
    .on_page("https://linktr.ee/mplsmutualaid", ArchivedPage {
        markdown: "Minneapolis Mutual Aid - Links",
        links: vec![
            "https://instagram.com/mplsmutualaid",
            "https://gofundme.com/f/help-families?utm_source=linktree",
            "https://localorg.org/resources",
            "https://fonts.googleapis.com/css2?family=Inter",  // junk
        ],
    })
    .on_page("https://linktr.ee/northsideaid", ArchivedPage {
        markdown: "Northside Aid - Links",
        links: vec![
            "https://instagram.com/mplsmutualaid",  // same IG — dedup
            "https://northsideaid.org/volunteer",
        ],
    });

let store = MockSignalStore::new();

run_discovery_chain(&fetcher, &store, &region).await;

// Sources promoted (deduplicated across both Linktrees)
assert!(store.has_source("instagram.com/mplsmutualaid"));  // once, not twice
assert!(store.has_source_url("https://gofundme.com/f/help-families")); // tracking stripped
assert!(store.has_source_url("https://localorg.org/resources"));
assert!(store.has_source_url("https://northsideaid.org/volunteer"));
assert!(!store.has_source_url("https://fonts.googleapis.com/css2")); // filtered

// Social handles extracted
assert!(store.has_social_source("instagram", "mplsmutualaid"));
```

### Chain 2: Signal Processing Path

"Fetch page → extract signals → dedup against prior state → store + wire actors"

Tests the full signal lifecycle from content to graph writes.

```rust
let fetcher = MockFetcher::new()
    .on_page("https://localorg.org/resources", ArchivedPage {
        markdown: "Free legal clinic every Tuesday at Sabathani Center...",
        links: vec!["https://facebook.com/localorg"],
    });

let extractor = MockExtractor::new()
    .on_url("https://localorg.org/resources", vec![
        Aid {
            title: "Free Legal Clinic at Sabathani",
            location: (44.93, -93.27),
            mentioned_actors: vec!["Volunteer Lawyers Network"],
            author_actor: Some("Sabathani Community Center"),
            ..
        },
    ]);

let store = MockSignalStore::new();

run_signal_chain(&fetcher, &extractor, &store, &embedder, &region).await;

// Signal created
assert_eq!(store.signals_created(), 1);
assert!(store.has_signal_titled("Free Legal Clinic at Sabathani"));

// Actors wired
assert!(store.has_actor("Volunteer Lawyers Network"));
assert!(store.has_actor("Sabathani Community Center"));
assert!(store.actor_linked_to_signal("Sabathani Community Center", "Free Legal Clinic at Sabathani", "authored"));
assert!(store.actor_linked_to_signal("Volunteer Lawyers Network", "Free Legal Clinic at Sabathani", "mentioned"));

// Evidence trail
assert!(store.has_evidence_from("https://localorg.org/resources"));
```

### Chain 3: Multi-Hop Recursive Discovery

"Linktree → org site → org's own links → promoted sources"

Tests that discovery follows links across multiple hops.

```rust
let fetcher = MockFetcher::new()
    // Hop 1: Linktree
    .on_page("https://linktr.ee/mplsmutualaid", ArchivedPage {
        links: vec!["https://localorg.org"],
    })
    // Hop 2: Org site (discovered from Linktree, fetched in next phase)
    .on_page("https://localorg.org", ArchivedPage {
        markdown: "Local Org - Serving South Minneapolis...",
        links: vec![
            "https://localorg.org/calendar",
            "https://partnerorg.org/joint-event",
            "https://facebook.com/localorg",
        ],
    });

let store = MockSignalStore::new();

// Phase A: scrape Linktree → promote localorg.org as source
// Phase B: scrape localorg.org → extract signals + promote its links
run_two_phase_pipeline(&fetcher, &extractor, &store, &region).await;

// Hop 1 discovery
assert!(store.has_source_url("https://localorg.org"));

// Hop 2 discovery (from localorg's own links)
assert!(store.has_source_url("https://partnerorg.org/joint-event"));
assert!(store.has_social_source("facebook", "localorg"));
```

### Chain 4: Multi-Source Corroboration

"Same event described on 3 independent sources → source_diversity = 3"

Tests the trust mechanism across the full pipeline.

```rust
let fetcher = MockFetcher::new()
    .on_page("https://source-a.org/events", ArchivedPage { .. })
    .on_page("https://source-b.org/calendar", ArchivedPage { .. })
    .on_page("https://instagram.com/localaccount", ..);

let extractor = MockExtractor::new()
    // All three sources describe the same event
    .on_url("https://source-a.org/events", vec![
        Gathering { title: "Community Garden Cleanup", location: (44.95, -93.26), .. }
    ])
    .on_url("https://source-b.org/calendar", vec![
        Gathering { title: "Community Garden Clean-Up", location: (44.95, -93.26), .. }
    ])
    .on_url("https://instagram.com/localaccount", vec![
        Gathering { title: "Garden cleanup this Saturday!", location: (44.95, -93.26), .. }
    ]);

// Embedder returns near-identical vectors for all three
let embedder = FixedEmbedder::new()
    .on_text("Community Garden Cleanup", VECTOR_A)
    .on_text("Community Garden Clean-Up", VECTOR_A_PRIME)    // cosine 0.97
    .on_text("Garden cleanup this Saturday!", VECTOR_A_CLOSE); // cosine 0.94

let store = MockSignalStore::new();

run_full_pipeline(&fetcher, &extractor, &store, &embedder, &region).await;

// ONE signal, corroborated by 3 sources
assert_eq!(store.signals_created(), 1);  // not 3
assert_eq!(store.corroborations("Community Garden Cleanup"), 2);  // 2 corroborations after initial create
assert_eq!(store.evidence_count("Community Garden Cleanup"), 3);  // 3 evidence trails
```

### Chain 5: Full Realistic Scenario

"Search for mutual aid → find Linktrees + org sites → scrape all → extract signals + actors + links → promote new sources"

End-to-end pipeline test with realistic data volume.

```rust
// 2 search results, 4 pages total (2 Linktrees + 2 org sites discovered from them)
// Produces: 3 signals, 5 actors, 8 promoted sources, 1 corroboration
// Tests volume caps, dedup across sources, actor reuse, link filtering

let fetcher = MockFetcher::new()
    .on_search("mutual aid Minneapolis", vec![..2 results..])
    .on_search("site:linktr.ee mutual aid Minneapolis", vec![..2 results..])
    .on_page("https://linktr.ee/mplsmutualaid", ..)
    .on_page("https://linktr.ee/northsideaid", ..)
    .on_page("https://mplsmutualaid.org", ..)
    .on_page("https://northsideaid.org", ..);

// ... extractor, embedder, store setup ...

run_scrape_pipeline(&fetcher, &extractor, &store, &embedder, &region).await;

// Assert the complete output state
assert_eq!(store.signals_created(), 3);
assert_eq!(store.actors_created(), 5);
assert_eq!(store.sources_promoted(), 8);
assert_eq!(store.total_corroborations(), 1);

// Specific assertions on dedup, actor reuse, link filtering...
```

---

## Shared Utilities

Pure functions that multiple organs depend on. Not organs themselves — they have no I/O contract — but critical to correctness.

### `canonical_value(url)` — SourceNode identity key

**Zero tests today.** This is the identity function for the entire source system.

```
// Social platform normalization
"https://www.instagram.com/mplsmutualaid/"   → "instagram.com/mplsmutualaid"
"https://instagram.com/mplsmutualaid"        → "instagram.com/mplsmutualaid"
"https://twitter.com/handle"                 → "x.com/handle"
"https://x.com/handle"                       → "x.com/handle"
"https://www.tiktok.com/@handle/"            → "tiktok.com/handle"
"https://www.reddit.com/r/Minneapolis/"      → "reddit.com/r/Minneapolis"

// Web URL edge cases (likely expose gaps in current implementation)
"https://www.example.com/page"  vs "https://example.com/page"    → should dedup?
"https://example.com/page#section" vs "https://example.com/page" → should dedup?
"https://example.com/page/" vs "https://example.com/page"        → should dedup?
"https://Example.COM/Page" vs "https://example.com/page"         → should dedup?

// Google Docs variants
"docs.google.com/document/d/ABC/edit"  vs "docs.google.com/document/d/ABC/view" → same doc

// Web queries pass through unchanged
"site:linktr.ee mutual aid Minneapolis" → "site:linktr.ee mutual aid Minneapolis"
```

### `extract_all_links(html, base_url)` — HTML link extraction

**Zero tests today.** Foundation of the entire Link Discoverer organ.

```
"<a href='https://instagram.com/org'>IG</a>"  → ["https://instagram.com/org"]
"<a href='/about'>About</a>" with base "https://example.com" → ["https://example.com/about"]
"background: url(https://cdn.example.com/img.png)" → ["https://cdn.example.com/img.png"]
"See us at https://example.com in the text" → ["https://example.com"]
empty HTML → []
malformed href → gracefully skipped
```

### Other utilities needing tests

| Function | Location | Tests today |
|----------|----------|-------------|
| `normalize_title()` | scrape_phase.rs | None (private) |
| `sanitize_url()` | scout/infra/util.rs | Good |
| `strip_tracking_params()` | link_promoter.rs | Good |
| `content_hash()` | scout/infra/util.rs | Good |
| `quality::score()` | scout/enrichment/quality.rs | Good |
| `geo_filter::filter_nodes()` | scout/pipeline/geo_filter.rs | Good |

Three URL normalization systems exist (`canonical_value`, `sanitize_url`, `strip_tracking_params`) with overlapping but different parameter lists. Tests will document the inconsistencies for future unification.

---

## Work Order

```
Phase 1: Shared utility tests (zero infrastructure, immediate value)
├── canonical_value() test battery
├── extract_all_links() test battery
├── normalize_title() tests
└── Document URL normalization inconsistencies across the three systems

Phase 2: Trait abstractions (unlock organ-level testing)
├── ContentFetcher trait + impl for Archive
├── SignalStore trait + impl for GraphWriter
├── MockFetcher (HashMap-based)
└── MockSignalStore (stateful in-memory graph)

Phase 3: Internal extractions from Signal Processor
├── dedup_verdict() + DedupVerdict enum
├── score_and_filter()
├── batch_title_dedup()
└── Test batteries for each (pure, sync, zero-infrastructure)

Phase 4: Boundary tests
├── Fetcher → Link Discoverer (Linktree chain, source location TDD)
├── Extractor → Signal Processor (dedup/corroborate/refresh)
├── Extractor → Actor Resolver (actor creation/linking/location)
├── Embedder → Signal Processor (vector dedup thresholds)
└── Location handoff scenarios (actor ↔ signal ↔ geo filter)

Phase 5: LLM edge testing
├── Record extraction snapshots (RECORD=1)
├── Deterministic field assertions on snapshots
├── Edge case fixtures (adversarial, multilingual, ambiguous geo)
└── Actor extraction snapshot tests

Phase 6: Chain tests (deterministic end-to-end with mocks)
├── Discovery loop (search → fetch → extract links → promote)
├── Signal processing path (fetch → extract → dedup → store + actors)
├── Multi-hop recursive discovery (Linktree → org site → their links)
├── Multi-source corroboration (same event from 3 sources)
└── Full realistic scenario (search → discover → scrape → extract → store)

Phase 7: SimWeb integration (LLM-backed scenarios)
├── SimulatedWeb implements ContentFetcher trait
├── Wire existing 8 scenarios through real ScrapePhase
├── Snapshot/replay for CI (record once, replay deterministically)
└── Judge evaluates signal quality against scenario criteria
```

Phase 1 has zero dependencies and catches real bugs. Phase 2 unlocks organ-level testing. Phase 3 makes the Signal Processor's internals testable. Phase 4 tests organ communication. Phase 5 tests the LLM edges. Phase 6 tests the pipeline end-to-end with mocked I/O. Phase 7 wires up SimWeb for LLM-backed scenario testing.

## Key Decisions

- **Four test levels: organs, boundaries, chains, scenarios.** Organ tests verify each organ in isolation. Boundary tests verify one handoff at a time. Chain tests verify sub-chains of the pipeline end-to-end with mocked I/O. SimWeb scenarios test the full pipeline with LLM-generated content. Utility tests cut across all levels.
- **`DedupVerdict` separates decision from execution.** The most valuable internal extraction — makes the trust mechanism testable with zero infrastructure.
- **Traits are honest about their surface.** `ContentFetcher` is 6 methods. `SignalStore` is 20 methods. Every method represents a distinct code path.
- **Hand-written mocks, no frameworks.** `MockSignalStore` with HashMaps is more readable than generated mock code.
- **Snapshot testing for LLM edges.** Record once, replay in CI. Deterministic fields exact, free text loose.
- **TDD for known bugs.** Source location stamp gets failing tests first, then a fix.
- **`SimulatedWeb` implements `ContentFetcher`.** The simweb crate already has `search()`, `scrape()`, `social_posts()` — same shape as the trait. Once `ContentFetcher` exists, `SimulatedWeb` becomes just another fetcher implementation. The 8 existing scenarios plug directly into the real pipeline.
- **`MockFetcher` supports multi-hop.** Chain tests need the fetcher called multiple times as discovery fans out. `MockFetcher` is a HashMap of URL → response; it naturally supports this — each URL in the chain just needs an entry.
- **Document known gaps.** Fuzzy actor matching, cross-source identity, URL normalization inconsistencies — tests document current behavior for future work.

## What We're NOT Doing

- Not rewriting `store_signals` as a pipeline of transforms. The read-write interleaving is correct.
- Not adding test frameworks (`mockall`, `wiremock`, `insta`).
- Not testing Restate workflows. Thin durability wrapper.
- Not solving actor fuzzy matching now. Tests document the gap.
- Not unifying URL normalization systems now. Tests document the inconsistencies.

## Next Steps

→ `/workflows:plan` for implementation — starting with Phase 1 shared utility tests.
