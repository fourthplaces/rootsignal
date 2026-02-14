---
date: 2026-02-14
topic: configurable-server
---

# Configurable Server via TOML + Markdown Prompts

## What We're Building

A configuration system that separates deployment identity from infrastructure. The Rust binary becomes a generic engine invoked with `./server --config ./some/config.toml`. The TOML defines the deployment's identity, model preferences, and operational parameters. AI preambles live in standalone Markdown files referenced from the TOML, using `{dotted.path}` template variables that resolve against the full config tree.

## Why This Approach

- **TOML for structured config, Markdown for prose** — Operators edit prompts in Markdown (easy to read, diff, review). TOML stays lean with just scalars and paths.
- **Template variables from config tree** — Prompts reference any TOML value via `{identity.region}`, `{models.extraction}`, etc. No separate variable registry needed.
- **Prompt files relative to config location** — `--config /etc/rootsignal/config.toml` resolves `./prompts/extraction.md` from `/etc/rootsignal/prompts/`. Deployments are self-contained directories.
- **Env vars stay for secrets** — `DATABASE_URL`, API keys, auth tokens remain env vars. Config holds identity and behavior, not credentials.

## Structure

```
config/
├── rootsignal.toml
└── prompts/
    ├── extraction.md
    ├── investigation.md
    └── nlq.md
```

### rootsignal.toml

```toml
[identity]
region = "Twin Cities (Minneapolis-St. Paul, Minnesota)"
description = "community signals in the Twin Cities metro area"
system_name = "community signal database"
locales = ["en", "es", "so", "ht"]

[models]
extraction = "gpt-4o"
nlq = "gpt-4o-mini"
investigation = "gpt-4o"

[prompts]
extraction = "./prompts/extraction.md"
investigation = "./prompts/investigation.md"
nlq = "./prompts/nlq.md"

[clustering]
similarity_threshold = 0.92
match_score_threshold = 0.75
merge_coherence_threshold = 0.85
geo_radius_meters = 500.0
time_window_hours = 24
batch_size = 100
hnsw_ef_search = 100
```

### prompts/extraction.md

```markdown
You are a community signal extractor for the {identity.region}.

Your job is to identify and extract actionable community signals—events,
services, resources, and other listings—from raw source content related to
{identity.description}.

...
```

## Key Decisions

- **Any TOML value is referenceable**: Templates use `{dotted.path}` syntax resolved against the parsed TOML tree. `{identity.region}`, `{clustering.geo_radius_meters}`, `{models.extraction}` all work.
- **Env vars for secrets, TOML for behavior**: `DATABASE_URL`, `OPENAI_API_KEY`, `JWT_SECRET`, etc. stay as env vars. The config file is safe to commit.
- **Paths resolve relative to config file**: Enables self-contained deployment directories that can be copied/shipped as a unit.
- **CLI arg `--config`**: Added to the server binary via clap. Falls back to `./config/rootsignal.toml` if not provided.
- **Existing env var defaults migrate into a default config**: The current Twin Cities defaults become the shipped `config/rootsignal.toml`.

## Open Questions

- Should arrays (e.g., `locales`) be referenceable in templates? If so, what's the format — comma-separated, JSON array?
- Should there be a `--validate-config` flag that checks all prompt files exist and all template vars resolve?
- Do we want config hot-reloading, or is restart-on-change sufficient for now?

## Next Steps

→ `/workflows:plan` for implementation details
