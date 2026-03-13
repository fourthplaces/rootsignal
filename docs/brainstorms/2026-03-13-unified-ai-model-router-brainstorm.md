---
date: 2026-03-13
topic: unified-ai-model-router
---

# Unified AI Model Router

## What We're Building

A model router that wraps all three AI providers (Claude, OpenAI, Gemini) behind a single entry point. Callers select a model explicitly — `deps.ai.model("claude-sonnet-4-5-20250929").extract_json(...)` — and the router auto-detects the provider from the model name prefix and delegates to the correct client.

Every call site must specify a model. There is no default. This forces intentional model selection per task — briefing generation uses a strong model, classification uses a cheap one.

## Why This Approach

The `Agent` trait and all three provider implementations already exist. The gap is that model selection happens once at engine init time (`build_base_deps()`), locking every handler to the same model. A router on `deps.ai` adds per-call model selection without changing the `Agent` trait or any provider implementation.

Prefix-based routing (`claude-*` → Anthropic, `gpt-*` → OpenAI, `gemini-*` → Google) requires zero configuration and matches existing model constant naming.

## Key Decisions

- **Router, not per-method param**: `deps.ai.model(M)` returns an `Arc<dyn Agent>`, keeping the `Agent` trait unchanged. Providers don't need to know about routing.
- **Always explicit**: No default model on `deps.ai` itself. Every call site passes the model it wants. Existing call sites must be updated.
- **Prefix convention for routing**: `claude-*` / `gpt-*` / `gemini-*` parsed from model string. No registry needed.
- **`deps.ai` type changes**: From `Option<Arc<dyn Agent>>` to `Option<Arc<ModelRouter>>` (or a new trait). The `ModelRouter` holds API keys for all providers and creates/caches `Agent` instances per model.
- **Model constants remain in `ai-client/src/lib.rs`**: Call sites reference `ai_client::models::SONNET_4_5` etc.

## Open Questions

- Should `ModelRouter` cache `Agent` instances per model string, or create fresh clients each call? (Caching is simpler — clients are stateless.)
- Should `ModelRouter` implement `Agent` itself (with a stored default) as a migration path, or be a separate type? (Decision: separate type, no default.)
- How to handle missing API keys gracefully? (e.g., Gemini key not set but caller requests `gemini-*`.) Error at call time vs startup?

## Next Steps

→ `/workflows:plan` for implementation details
