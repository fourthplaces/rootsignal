---
title: "feat: Configurable server via TOML config + Markdown prompt templates"
type: feat
date: 2026-02-14
---

# Configurable Server via TOML + Markdown Prompt Templates

## Overview

Transform the Root Signal server from a hardcoded, env-var-configured binary into a generic engine driven by a TOML config file and externalized Markdown prompt templates. The server is invoked with `./server --config ./config/rootsignal.toml`. AI preambles live in `.md` files referenced from the TOML. All template variables use `{{double.brace}}` syntax with three resolution phases: config-time, runtime-dynamic, and runtime-DB.

## Problem Statement / Motivation

The server's identity (region, AI behavior, operational params) is scattered across env vars with hardcoded Twin Cities defaults and inline Rust `format!()` prompt strings. This couples the infrastructure to a single deployment. To run the same binary for a different city or use-case, you'd need to modify Rust source code. Externalizing identity into config + prompt files makes the binary reusable across deployments.

## Proposed Solution

### Config Structure

```
config/
├── rootsignal.toml
└── prompts/
    ├── extraction.md
    ├── investigation.md
    └── nlq.md
```

### TOML Schema

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

[server]
port = 9080
allowed_origins = ["http://localhost:3000"]
```

### Template Syntax

All template variables use `{{double.brace}}` syntax. Three resolution phases:

| Phase | Prefix/Pattern | Resolved When | Example |
|-------|---------------|---------------|---------|
| Config | `{{config.*}}` | Startup (load time) | `{{config.identity.region}}` |
| Runtime input | `{{today}}`, `{{query}}` | Per-request | `{{today}}` in NLQ prompt |
| Runtime DB | `{{taxonomy}}` | Per-request | Tag instructions from DB |

Example `prompts/extraction.md`:
```markdown
You are a community signal extractor for the {{config.identity.region}}.

Your job is to identify and extract actionable community signals—events,
services, resources, and other listings—from raw source content related to
{{config.identity.description}}.

## Available Categories

{{taxonomy}}
```

### Secrets Stay as Env Vars

These remain env-var-only (never in TOML):
- `DATABASE_URL`
- `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`
- `TAVILY_API_KEY`, `FIRECRAWL_API_KEY`, `APIFY_API_KEY`, `EVENTBRITE_API_KEY`
- `GEOCODING_API_KEY`
- `JWT_SECRET`
- `TWILIO_ACCOUNT_SID`, `TWILIO_AUTH_TOKEN`, `TWILIO_VERIFY_SERVICE_SID`
- `RESTATE_AUTH_TOKEN`

## Technical Approach

### Phase 1: Config Loading Infrastructure

Add dependencies and build the config loading pipeline.

**New workspace dependencies** in `Cargo.toml`:
```toml
toml = "0.8"
clap = { version = "4", features = ["derive"] }
```

**New file: `modules/rootsignal-core/src/file_config.rs`**

```rust
/// TOML-backed configuration loaded from disk.
/// Secrets (API keys, DB URL) stay as env vars.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    pub identity: IdentityConfig,
    pub models: ModelsConfig,
    pub prompts: PromptsConfig,
    pub clustering: ClusteringConfig,
    pub server: ServerConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IdentityConfig {
    pub region: String,
    pub description: String,
    pub system_name: String,
    pub locales: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelsConfig {
    pub extraction: String,
    pub nlq: String,
    pub investigation: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PromptsConfig {
    pub extraction: PathBuf,
    pub investigation: PathBuf,
    pub nlq: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClusteringConfig {
    pub similarity_threshold: f64,
    pub match_score_threshold: f64,
    pub merge_coherence_threshold: f64,
    pub geo_radius_meters: f64,
    pub time_window_hours: i64,
    pub batch_size: usize,
    pub hnsw_ef_search: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
    pub allowed_origins: Vec<String>,
}
```

**Config loading function** in `file_config.rs`:

```rust
pub fn load_config(path: &Path) -> Result<FileConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    let config: FileConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
    Ok(config)
}
```

**Files touched:**
- `Cargo.toml` (workspace deps) — add `toml`, `clap`
- `modules/rootsignal-core/Cargo.toml` — add `toml`, `clap` deps
- `modules/rootsignal-core/src/file_config.rs` — new file
- `modules/rootsignal-core/src/lib.rs` — re-export `file_config`

### Phase 2: Template Engine

A minimal template engine that resolves `{{...}}` variables against a context map.

**New file: `modules/rootsignal-core/src/template.rs`**

The engine:
1. At load time: reads each `.md` file, resolves `{{config.*}}` variables by walking the TOML tree, caches the partially-resolved string.
2. At call time: resolves remaining `{{variables}}` (like `{{taxonomy}}`, `{{today}}`) via a passed-in `HashMap<&str, String>`.

```rust
/// Resolve {{config.*}} variables from the TOML tree at load time.
/// Returns the template with config vars resolved, runtime vars left as-is.
pub fn resolve_config_vars(template: &str, toml_value: &toml::Value) -> Result<String>

/// Resolve remaining {{var}} placeholders from a runtime context map.
pub fn resolve_runtime_vars(template: &str, vars: &HashMap<&str, &str>) -> String

/// Validate that all {{...}} in a template are either config.* or in an allowed runtime set.
pub fn validate_template(template: &str, toml_value: &toml::Value, allowed_runtime: &[&str]) -> Result<()>
```

Resolution of `{{config.*}}` walks the TOML `Value` tree by splitting on `.`:
- `{{config.identity.region}}` → look up `identity` → `region` → render as string
- Scalars (string, int, float, bool) render as their Display representation
- Arrays are **not directly referenceable** — the locale display-name mapping stays in Rust code. `{{config.identity.locales}}` would error; use runtime vars for formatted locale lists if needed.

Literal `{{` is escaped as `\{{`.

**Files touched:**
- `modules/rootsignal-core/src/template.rs` — new file
- `modules/rootsignal-core/src/lib.rs` — re-export

### Phase 3: Prompt Registry

A struct that holds loaded + config-resolved prompt templates, ready for runtime variable injection.

**New file: `modules/rootsignal-core/src/prompt_registry.rs`**

```rust
/// Holds pre-resolved prompt templates (config vars resolved, runtime vars intact).
#[derive(Debug, Clone)]
pub struct PromptRegistry {
    pub extraction: String,    // config vars resolved, {{taxonomy}} intact
    pub investigation: String, // config vars resolved
    pub nlq: String,           // config vars resolved, {{taxonomy}} + {{today}} intact
}

impl PromptRegistry {
    /// Load all prompt files, resolve config vars, validate runtime vars.
    pub fn load(config: &FileConfig, config_dir: &Path, toml_value: &toml::Value) -> Result<Self> {
        // For each prompt path:
        // 1. Resolve path relative to config_dir
        // 2. Read .md file
        // 3. resolve_config_vars()
        // 4. validate remaining {{vars}} against allowed set
        // 5. Store resolved string
    }

    /// Get extraction prompt with runtime vars filled in.
    pub fn extraction_prompt(&self, taxonomy: &str) -> String {
        resolve_runtime_vars(&self.extraction, &HashMap::from([("taxonomy", taxonomy)]))
    }

    /// Get NLQ prompt with runtime vars filled in.
    pub fn nlq_prompt(&self, taxonomy: &str, today: &str) -> String {
        resolve_runtime_vars(&self.nlq, &HashMap::from([
            ("taxonomy", taxonomy),
            ("today", today),
        ]))
    }

    /// Get investigation prompt (no runtime vars currently).
    pub fn investigation_prompt(&self) -> &str {
        &self.investigation
    }
}
```

**Files touched:**
- `modules/rootsignal-core/src/prompt_registry.rs` — new file
- `modules/rootsignal-core/src/lib.rs` — re-export

### Phase 4: Wire into ServerDeps + AppConfig

Update `ServerDeps` and `AppConfig` to use the new config system.

**`modules/rootsignal-core/src/deps.rs`** — add `prompts: Arc<PromptRegistry>` and `file_config: Arc<FileConfig>` to `ServerDeps`:

```rust
pub struct ServerDeps {
    pub db_pool: PgPool,
    pub http_client: reqwest::Client,
    pub ai: Arc<OpenAi>,
    pub claude: Option<Arc<Claude>>,
    pub ingestor: Arc<dyn Ingestor>,
    pub web_searcher: Arc<dyn WebSearcher>,
    pub embedding_service: Arc<dyn EmbeddingService>,
    pub config: AppConfig,           // secrets + env-only values
    pub file_config: Arc<FileConfig>, // NEW: TOML-loaded config
    pub prompts: Arc<PromptRegistry>, // NEW: resolved prompt templates
}
```

**`modules/rootsignal-core/src/config.rs`** — slim down `AppConfig` to secrets-only. Remove fields that moved to `FileConfig` (`region_name`, `region_description`, `system_description`, `supported_locales`, clustering params, `port`, `allowed_origins`). Keep all API keys, DB URL, auth tokens, Restate URLs.

**Files touched:**
- `modules/rootsignal-core/src/deps.rs` — add `file_config` + `prompts` fields
- `modules/rootsignal-core/src/config.rs` — remove fields that moved to TOML
- Every site that reads migrated fields from `deps.config` → read from `deps.file_config`

### Phase 5: CLI + Server Startup

Add `--config` flag to the server binary.

**`modules/rootsignal-server/src/main.rs`:**

```rust
#[derive(Parser)]
#[command(name = "rootsignal-server")]
struct Cli {
    /// Path to config TOML file
    #[arg(long, default_value = "./config/rootsignal.toml")]
    config: PathBuf,
}
```

Startup flow changes:
1. Parse CLI args via clap
2. Load `FileConfig` from the TOML path (fatal error if missing)
3. Load `PromptRegistry` from `.md` files (fatal error if missing/invalid)
4. Load `AppConfig` from env vars (secrets only)
5. Read model names from `file_config.models` and pass to AI client constructors
6. Build `ServerDeps` with all three config sources
7. Start servers using `file_config.server.port`

**`modules/rootsignal-server/Cargo.toml`** — add `clap` dependency.

**Files touched:**
- `modules/rootsignal-server/src/main.rs` — add clap, rewrite startup
- `modules/rootsignal-server/Cargo.toml` — add `clap`

### Phase 6: Update Prompt Call Sites

Replace inline `format!()` prompts with `PromptRegistry` lookups.

**`modules/rootsignal-domains/src/extraction/activities/extract.rs`:**
- Remove `system_preamble()` function (lines 24-68)
- `build_system_prompt()` becomes: `deps.prompts.extraction_prompt(&taxonomy)`
- Model name: `deps.file_config.models.extraction` instead of hardcoded `"gpt-4o"`

**`modules/rootsignal-domains/src/search/nlq.rs`:**
- Remove `nlq_system_preamble()` function (lines 8-39)
- System prompt becomes: `deps.prompts.nlq_prompt(&taxonomy, &today)`
- Model name: `deps.file_config.models.nlq` instead of hardcoded `"gpt-4o-mini"`

**`modules/rootsignal-domains/src/investigations/mod.rs`:**
- Remove inline preamble string (line 66)
- Use: `deps.prompts.investigation_prompt()`
- Model name: `deps.file_config.models.investigation`

**Files touched:**
- `modules/rootsignal-domains/src/extraction/activities/extract.rs`
- `modules/rootsignal-domains/src/search/nlq.rs`
- `modules/rootsignal-domains/src/investigations/mod.rs`
- Any other file reading `config.region_name`, `config.cluster_*`, etc.

### Phase 7: Ship Default Config

Create the default `config/` directory with the Twin Cities config as the shipped example.

**Files created:**
- `config/rootsignal.toml` — full config with current Twin Cities defaults
- `config/prompts/extraction.md` — extracted from current `system_preamble()` in `extract.rs`
- `config/prompts/investigation.md` — extracted from inline string in `investigations/mod.rs`
- `config/prompts/nlq.md` — extracted from `nlq_system_preamble()` in `nlq.rs`

### Not in Scope (Deferred)

- Translation and query-translation prompts (stay inline — simple one-liners)
- Hot-reloading of config/prompts (restart is sufficient for v1)
- `--validate-config` dry-run flag (follow-up)
- Env var overrides for TOML values (TOML is authoritative for non-secret config)
- Array template variable rendering (locale formatting stays in Rust)

## Acceptance Criteria

- [x] Server starts with `./server --config ./config/rootsignal.toml`
- [x] Server fatally errors with helpful message when config file is missing
- [x] Server fatally errors when a referenced `.md` prompt file is missing
- [x] Server fatally errors when a `{{config.*}}` variable in a prompt doesn't resolve
- [x] `{{config.*}}` variables resolve from the TOML tree at startup
- [x] `{{taxonomy}}` and `{{today}}` resolve at runtime per-request
- [x] Prompt file paths resolve relative to the config file's directory
- [x] Model names from `[models]` are used at AI call sites (not hardcoded)
- [x] Clustering params from `[clustering]` are used (not env var defaults)
- [x] Port and CORS origins from `[server]` are used
- [x] API keys and DB URL still load from env vars
- [x] Existing behavior is preserved — same prompts, same models, same params
- [x] `\{{` escapes to a literal `{{` in prompt templates

## Dependencies & Risks

- **New crate dependencies:** `toml = "0.8"` and `clap = "4"` in the workspace. Both are well-established, low-risk.
- **Breaking change:** Existing deployments must create a config file. No backward-compat env-var fallback. Mitigated by shipping a complete default config.
- **Prompt extraction risk:** Moving prompts from Rust to `.md` files could introduce subtle formatting differences. Mitigate by diffing the rendered output before/after.
- **Template engine scope creep:** Keep it minimal — variable substitution only, no loops, no conditionals. If more is needed later, consider a real template engine like `tera` or `handlebars`.

## References & Research

### Internal References
- Current config: `modules/rootsignal-core/src/config.rs`
- ServerDeps: `modules/rootsignal-core/src/deps.rs`
- Extraction prompt: `modules/rootsignal-domains/src/extraction/activities/extract.rs:24-68`
- NLQ prompt: `modules/rootsignal-domains/src/search/nlq.rs:8-39`
- Investigation prompt: `modules/rootsignal-domains/src/investigations/mod.rs:66`
- Tag instructions builder: `modules/rootsignal-domains/src/entities/models/tag_kind.rs:53`
- Server entry: `modules/rootsignal-server/src/main.rs`
- Dev CLI (clap example): `dev/cli/src/main.rs`

### Brainstorm
- `docs/brainstorms/2026-02-14-configurable-server-brainstorm.md`
