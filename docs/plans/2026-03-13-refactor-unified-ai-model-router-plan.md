---
title: "Unified AI Model Router"
type: refactor
date: 2026-03-13
---

# Unified AI Model Router

## Overview

Replace the single shared `FallbackAgent` on `deps.ai` with a `ModelRouter` that returns model-specific `Arc<dyn Agent>` instances. Callers select a model explicitly — `deps.ai.model(SONNET_4_5).extract_json(...)` — and the router auto-detects the provider from the model name prefix (`claude-*` → Anthropic, `gpt-*` → OpenAI, `gemini-*` → Google).

Brainstorm: `docs/brainstorms/2026-03-13-unified-ai-model-router-brainstorm.md`

## Problem Statement

Today `build_base_deps()` creates a single `FallbackAgent(GPT-5-mini, Sonnet-4.6)` that every handler shares. This means:
- Briefing generation uses the same cheap model as classification
- No per-task model selection without constructing clients ad-hoc
- Gemini is wired in config but never used despite having an API key
- `investigate.rs` constructs `Claude` directly because the shared agent is wrong for its needs

## Proposed Solution

A `ModelRouter` struct on `deps.ai` that holds API keys for all providers and lazily creates/caches `Agent` instances per model string. The `Agent` trait stays unchanged — the router is a factory, not an agent.

```
deps.ai.model("claude-sonnet-4-5-20250929").extract_json(system, user, schema)
         │                                    │
         │  returns Arc<dyn Agent>            │ standard Agent trait call
         └────────────────────────────────────┘
```

## Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Router type | Concrete struct, not trait | Follows brainstorm. `ModelRouter::single(mock)` constructor for tests. |
| `model()` return type | `Arc<dyn Agent>` (panics on unknown prefix) | Unknown prefix = programming error. Missing API key = runtime error on the Agent itself. |
| Caching | `DashMap<String, Arc<dyn Agent>>` | Clients are stateless. Concurrent handlers need lock-free reads. |
| FallbackAgent | Removed from `build_base_deps()` | Each call site picks its model. Callers can compose `FallbackAgent` on top if needed. |
| `investigate.rs` | Out of scope | Uses concrete `Claude` with `.tool()` builder. Migrating requires `with_tools()` refactor — separate PR. |
| `archive` | Out of scope | Uses concrete `Claude`/`OpenAi` for `describe_image`/`transcribe` (not on `Agent` trait). |
| `deps.anthropic_api_key` | Kept | Supervisor and archive still need raw keys. Remove later when those are migrated. |
| Migration strategy | Single PR | `deps.ai` type change is breaking — all call sites must update atomically. |

## Model Assignment Table

Every call site must specify a model. This table assigns models by task characteristics.

| Domain | Handler/Activity | Current | New Model | Rationale |
|--------|------------------|---------|-----------|-----------|
| extraction | `Extractor` (signal extraction) | FallbackAgent(GPT-5-mini → Sonnet-4.6) | `GPT_5_MINI` | High volume, cost-sensitive. Good enough for structured extraction. |
| enrichment | `enrich_signals` (batch review) | FallbackAgent | `GPT_4_1_MINI` | Lightweight review/classification. Cheapest viable. |
| expansion | `expand_signals` | FallbackAgent | `GPT_5_MINI` | Similar to extraction — structured output. |
| discovery/bootstrap | `bootstrap_region` | FallbackAgent | `GPT_5_MINI` | Generates search queries from region description. |
| discovery/filter | `filter_signals` | FallbackAgent | `GPT_4_1_MINI` | Binary relevance filtering. Cheap model sufficient. |
| discovery/promotion | `promote_signals` | FallbackAgent | `GPT_5_MINI` | Signal quality assessment. |
| discovery/expansion | `expand_region` | FallbackAgent | `GPT_5_MINI` | Structured output for region expansion. |
| synthesis | `synthesize_briefing` | FallbackAgent | `SONNET_4_5` | Narrative quality matters. Strong model. |
| situation_weaving | `weave_situation` | FallbackAgent | `SONNET_4_5` | Complex narrative synthesis. Strong model. |
| cluster_weaving | `weave_cluster` | FallbackAgent | `GPT_5_MINI` | Cluster labeling. Moderate complexity. |
| coalescing | `coalesce_signal` (3 call sites) | FallbackAgent | `GPT_5_MINI` | Grouping/matching. Structured output. |
| curiosity/investigate | `investigate_situation` | FallbackAgent | `SONNET_4_6` | Multi-turn reasoning. Strongest model. |
| curiosity/concern_link | `link_concerns` | FallbackAgent | `GPT_5_MINI` | Relationship classification. |
| curiosity/response_find | `find_responses` | FallbackAgent | `GPT_5_MINI` | Structured search. |
| curiosity/gathering_find | `GatheringFinderDeps.ai` | FallbackAgent | `SONNET_4_5` | Complex multi-step reasoning with tools. |
| news_scanning | `scan_news` | FallbackAgent | `GPT_4_1_MINI` | Lightweight headline classification. |

## Technical Approach

### Phase 1: ModelRouter in `ai-client`

- [ ] Add `ModelRouter` struct to `ai-client/src/router.rs`
- [ ] Add `DashMap` dependency to `ai-client/Cargo.toml`
- [ ] Export `ModelRouter` from `ai-client/src/lib.rs`
- [ ] Add `ModelRouter::single()` test constructor

```rust
// ai-client/src/router.rs

use dashmap::DashMap;
use std::sync::Arc;
use crate::{Agent, Claude, OpenAi, Gemini};

pub struct ModelRouter {
    anthropic_key: String,
    openai_key: String,
    gemini_key: String,
    cache: DashMap<String, Arc<dyn Agent>>,
}

enum Provider { Anthropic, OpenAi, Gemini }

fn detect_provider(model: &str) -> Provider {
    if model.starts_with("claude-") { Provider::Anthropic }
    else if model.starts_with("gpt-") { Provider::OpenAi }
    else if model.starts_with("gemini-") { Provider::Gemini }
    else { panic!("Unknown model prefix: {model}. Use claude-*, gpt-*, or gemini-*.") }
}

impl ModelRouter {
    pub fn new(anthropic_key: String, openai_key: String, gemini_key: String) -> Self {
        Self { anthropic_key, openai_key, gemini_key, cache: DashMap::new() }
    }

    /// Test constructor: returns the same agent for any model name.
    pub fn single(agent: Arc<dyn Agent>) -> Self {
        let router = Self::new(String::new(), String::new(), String::new());
        // Store a sentinel so model() finds it
        // Overridden in the model() method for single-mode
        // (Implementation detail: store the agent and a flag)
        router
    }

    pub fn model(&self, model: &str) -> Arc<dyn Agent> {
        if let Some(agent) = self.cache.get(model) {
            return Arc::clone(agent.value());
        }
        let agent: Arc<dyn Agent> = match detect_provider(model) {
            Provider::Anthropic => Arc::new(Claude::new(&self.anthropic_key, model)),
            Provider::OpenAi => Arc::new(OpenAi::new(&self.openai_key, model)),
            Provider::Gemini => Arc::new(Gemini::new(&self.gemini_key, model)),
        };
        self.cache.insert(model.to_string(), Arc::clone(&agent));
        agent
    }
}
```

**Tests (TDD):**
- [ ] `model_with_claude_prefix_returns_claude_agent`
- [ ] `model_with_gpt_prefix_returns_openai_agent`
- [ ] `model_with_gemini_prefix_returns_gemini_agent`
- [ ] `unknown_prefix_panics`
- [ ] `same_model_returns_cached_instance`
- [ ] `single_constructor_returns_same_agent_for_any_model`

### Phase 2: Wire `ModelRouter` into `ScoutEngineDeps`

- [ ] Change `ScoutEngineDeps.ai` from `Option<Arc<dyn Agent>>` to `Option<Arc<ModelRouter>>`
- [ ] Update `build_base_deps()` to construct `ModelRouter::new(anthropic, openai, gemini)`
- [ ] Update `make_extractor()` to call `router.model(GPT_5_MINI)` for extraction
- [ ] Remove `FallbackAgent` construction from `build_base_deps()`

**Files:**
| File | Change |
|------|--------|
| `modules/rootsignal-scout/src/core/engine.rs` | `ai: Option<Arc<dyn Agent>>` → `ai: Option<Arc<ModelRouter>>` |
| `modules/rootsignal-scout/src/workflows/mod.rs` | `build_base_deps()` — construct `ModelRouter`, pass to `make_extractor` |

### Phase 3: Migrate all handler call sites

Each call site changes from:
```rust
// Before
let ai = deps.ai.as_ref()?;           // &Arc<dyn Agent>
let result = ai_extract(ai.as_ref(), ...).await?;

// After
let router = deps.ai.as_ref()?;       // &Arc<ModelRouter>
let ai = router.model(ai_client::models::GPT_5_MINI);
let result = ai_extract(ai.as_ref(), ...).await?;
```

For `as_deref()` patterns:
```rust
// Before
let ai = deps.ai.as_deref()?;         // &dyn Agent

// After
let ai = deps.ai.as_ref()?.model(ai_client::models::GPT_5_MINI);
let result = ai_extract(ai.as_ref(), ...).await?;
```

- [ ] `domains/enrichment/mod.rs` — `enrich_signals` → `GPT_4_1_MINI`
- [ ] `domains/expansion/mod.rs` — `expand_signals` → `GPT_5_MINI`
- [ ] `domains/discovery/activities/bootstrap.rs` — `bootstrap_region` → `GPT_5_MINI`
- [ ] `domains/discovery/mod.rs` — `filter_signals` → `GPT_4_1_MINI`
- [ ] `domains/discovery/mod.rs` — `promote_signals` → `GPT_5_MINI`
- [ ] `domains/discovery/mod.rs` — `expand_region` → `GPT_5_MINI`
- [ ] `domains/synthesis/mod.rs` — `synthesize_briefing` → `SONNET_4_5`
- [ ] `domains/situation_weaving/activities/mod.rs` — `weave_situation` → `SONNET_4_5`
- [ ] `domains/cluster_weaving/activities.rs` — `weave_cluster` → `GPT_5_MINI`
- [ ] `domains/coalescing/mod.rs` — 3 call sites → `GPT_5_MINI`
- [ ] `domains/curiosity/mod.rs` — `investigate_situation` → `SONNET_4_6`
- [ ] `domains/curiosity/mod.rs` — `link_concerns` → `GPT_5_MINI`
- [ ] `domains/curiosity/mod.rs` — `find_responses` → `GPT_5_MINI`
- [ ] `domains/curiosity/mod.rs` — `gathering_find` → `SONNET_4_5`
- [ ] `domains/curiosity/activities/gathering_finder.rs` — `GatheringFinderDeps.ai` type change
- [ ] `domains/news_scanning/activities.rs` — `scan_news` → `GPT_4_1_MINI`

### Phase 4: Migrate tests

- [ ] Update `MockAgent` injection sites to use `ModelRouter::single(mock)`
- [ ] Update `ScoutRunTest` builder: `.ai(Arc<dyn Agent>)` → `.ai(Arc<dyn Agent>)` internally wraps with `ModelRouter::single()`
- [ ] Update `test_engine_with_ai_capture()` in `testing.rs`
- [ ] Update `test_engine_for_source_run()` in `testing.rs`
- [ ] Update coalescing test helpers
- [ ] Verify all existing tests pass with no behavior change

### Phase 5: Cleanup

- [ ] Remove `FallbackAgent` import from `workflows/mod.rs`
- [ ] Update `ai-client` re-exports if needed (keep `FallbackAgent` public — callers may compose it)
- [ ] `cargo check --workspace`
- [ ] `cargo test --workspace`

## Acceptance Criteria

- [ ] `deps.ai.model("claude-sonnet-4-5-20250929")` returns a Claude-backed Agent
- [ ] `deps.ai.model("gpt-5-mini")` returns an OpenAI-backed Agent
- [ ] `deps.ai.model("gemini-2.5-flash")` returns a Gemini-backed Agent
- [ ] Unknown prefix panics with clear message
- [ ] Same model string returns cached instance (pointer equality)
- [ ] Every handler call site specifies an explicit model constant
- [ ] All existing tests pass (behavior unchanged, just model selection explicit)
- [ ] `investigate.rs` and `archive` unchanged (out of scope)
- [ ] `cargo check --workspace` passes
- [ ] `cargo test --workspace` passes

## Out of Scope

- **`investigate.rs`**: Uses concrete `Claude` with `.tool()` builder. Migrating requires refactoring to `with_tools()`. Separate PR.
- **`rootsignal-archive`**: Uses `describe_image` and `transcribe` — provider-specific methods not on `Agent` trait.
- **Supervisor**: Uses raw `anthropic_api_key`. Stays as-is.
- **OpenRouter support**: No prefix convention for vendor-namespaced models (`deepseek/deepseek-v3.2`). Add later if needed.
- **Per-call fallback**: `model_with_fallback(primary, secondary)` — callers can compose `FallbackAgent` on top. Add to router later if pattern emerges.
- **Circuit breaker integration**: Not integrated at router level. Call sites add if needed.
- **Model name validation**: Router does not validate against known constants. API rejects unknown models.

## Existing Code to Reuse

- `Agent` trait — `ai-client/src/traits.rs` (unchanged)
- `Claude::new()`, `OpenAi::new()`, `Gemini::new()` — provider constructors (unchanged)
- `FallbackAgent` — available for callers who want to compose fallback behavior on top
- `MockAgent` — `testing.rs` (wrapped in `ModelRouter::single()`)
- `ScoutRunTest` builder — `testing.rs` (`.ai()` wraps internally)
- Model constants — `ai-client/src/lib.rs`

## Verification

```bash
cargo check --workspace
cargo test -p ai-client
cargo test -p rootsignal-scout
cargo test --workspace
```
