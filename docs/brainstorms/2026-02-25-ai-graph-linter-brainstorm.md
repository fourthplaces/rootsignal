---
date: 2026-02-25
topic: ai-graph-linter
supersedes: 2026-02-24-signal-lint-brainstorm
---

# AI Graph Linter

## What We're Building

A general-purpose AI auditor for the graph database that replaces the existing hardcoded quality checks with something more capable. One system, three modes: two automated blocking gates in the pipeline (signals, situations) and a manual investigation mode for admins. The AI gets Cypher read access to freely traverse the graph. Write access is mode-dependent and scoped.

Signals are immutable after Gate 1. Situations are versioned — they evolve as new signals arrive, but every version passes Gate 2 before going live. Post-gate issues are gate problems, not data problems.

## Why This Approach

The existing quality checks are narrowly scoped — they verify specific fields against source content but can't reason across relationships, spot patterns, or adapt. A general AI linter with graph traversal can catch things hardcoded rules never will: semantic duplicates across sources, contradictory signals, situations that don't hold together, misattributed actors, stale schedules linked to active signals.

The key constraint is that write access must be tightly controlled. The two pipeline gates get scoped write access (fix and promote, or reject). Everything after that is read-only diagnosis. This keeps the data trustworthy — if it's published, it passed two AI gates with a strong model, and nothing mutates it afterward.

### Approaches Considered

1. **Expand existing lint checks** — add more hardcoded rules to the current pipeline. Rejected: doesn't scale, can't reason about relationships, every new check is custom code.

2. **General AI linter with graph access (chosen)** — one system that handles all three modes. AI builds its own Cypher queries, reasons about what it finds, applies mode-appropriate actions. Rules are concerns described in natural language, not code.

3. **Separate tools per mode** — dedicated signal linter, situation linter, admin investigator. Rejected: too much duplication. The core capability (read graph, reason, report) is the same across all three.

## Unified Review Status

Signals and situations share a single `ReviewStatus` enum for consistency across the pipeline. Replaces the current string-based `review_status` on signals (`"staged"`, `"live"`, `"rejected"`) and adds the same lifecycle to situations (which currently have no status field at all).

```rust
enum ReviewStatus {
    Draft,       // Just produced, awaiting gate review
    Published,   // Passed gate, visible to public
    Quarantined, // Gate flagged issues, needs human review (recoverable)
    Rejected,    // Definitely bad (terminal)
}
```

**State transitions:**

```
              ┌─────────────┐
              │    Draft     │  ← extraction / weaver writes here
              └──────┬───────┘
                     │ gate review
            ┌────────┼────────┐
            ▼        ▼        ▼
      Published  Quarantined  Rejected
                     │
              human review
            ┌────┴────┐
            ▼         ▼
      Published    Rejected
```

| Status | Who sets it | Meaning | Recoverable? |
|--------|-----------|---------|--------------|
| `Draft` | Pipeline (extractor, weaver) | Awaiting gate review | — |
| `Published` | Gate (automated) or admin (from quarantined) | Passed quality check, visible to public | — |
| `Quarantined` | Gate (automated) | Gate isn't confident — human should review | Yes → published or rejected |
| `Rejected` | Gate (automated) or admin (from quarantined) | Bad data, not recoverable | Terminal |

**Migration from current model:**
- Signal `review_status: String` → `review_status: ReviewStatus` enum
- `"staged"` → `Draft`
- `"live"` → `Published`
- `"rejected"` → `Rejected`
- Add `Quarantined` (new state — currently the signal linter either passes or rejects, no middle ground)
- Add `review_status` field to `SituationNode` (doesn't exist today)
- Add `rejection_reason` and `quarantine_reason` fields to `SituationNode`

## Architecture

### Core: The Linter

The linter is an AI agent with:

- **Context appropriate to its mode** — gates get specific nodes handed to them, investigation gets graph traversal
- **A set of concerns** to check (natural language descriptions, not code)
- **Mode-dependent tools** that control what actions it can take
- **A stronger model** than what runs during extraction — this is the final check

The same core runs in all three modes. What changes is the trigger, the tool set, and how data is provided.

### Cypher Sandbox Module

A standalone, generic library for validating and constraining Cypher queries. Used **only by admin investigation mode** — the gates don't need freeform Cypher.

**What it does:**
- Parses Cypher into an AST
- Walks the AST and validates against a permission set
- Rejects disallowed queries with a clear error (so the AI can retry)
- Logs every query for audit trail

**Permission set (configurable per caller):**
- **Readable labels** — which node types can be queried (e.g. `Signal`, `Situation`, `Actor`, `Source`)
- **Traversable relationships** — which relationship types can be followed (e.g. `CITES`, `LINKED_TO`, `AUTHORED_BY`)
- **Blocked operations** — no write operations ever (`CREATE`, `SET`, `DELETE`, `MERGE`, `REMOVE`)
- **Complexity limits** — max traversal depth, no unbounded `*..` patterns, query timeout

This module is generic — it doesn't know about linting or signals. Any part of the system that wants to give an AI controlled Cypher access uses it.

### Gate Context Loader

The gates don't use freeform Cypher. They get their data from a purpose-built context loader:

- Fetch the batch of draft nodes from the current run
- For Gate 1: load each signal + its archived source content
- For Gate 2: load draft situation + its linked signals + previous published version (if amending)
- Hand the data directly to the AI as structured context

No query construction, no validation needed — the gates always look at exactly what was just produced.

### Mode 1: Signal Gate

| | |
|---|---|
| **Trigger** | Automated, after signal extraction completes |
| **Input** | Batch of signals with `status = 'draft'` from the current scout run |
| **Graph access** | Full read |
| **Write access** | Scoped — can update signal fields, change status to `published` or `quarantined` |
| **Model** | Stronger model (final check before publish) |

**What gets checked:**
- Extraction fidelity — does the signal match its source content?
- Completeness — any signals missed that were clearly present?
- Structural integrity — dates, URLs, coordinates, schedules, types
- Semantic dedup — is this signal already represented by an existing published signal?
- Spam/astroturf detection — does this look like legitimate community content?

**Outcomes per signal:**
- **Publish** — signal is correct, flip to `published`
- **Correct + publish** — fixable issues, auto-correct with change reason logged
- **Quarantine** — unfixable issues, mark with reason, flagged for admin review

### Mode 2: Situation Gate

| | |
|---|---|
| **Trigger** | Automated, after situation weaver creates or amends a situation |
| **Input** | Draft situation (new or amended) |
| **Graph access** | Full read (including linked signals and previous version if amending) |
| **Write access** | Scoped — can update situation fields, reattach/detach signals, change status |
| **Model** | Stronger model |

**Situations are versioned, not immutable.** A situation is a living narrative identified by a stable slug. As new signals arrive, the situation weaver amends the situation — updating the summary, linking new signals, adjusting severity. Each amended version goes through Gate 2 as a draft before replacing the live version. Previous versions are retained for history.

**Dispatches** are append-only and individually immutable. Each dispatch is a moment-in-time snapshot that accumulates alongside the evolving situation summary.

**The amendment cycle:**
```
New signals arrive
    ↓
Situation weaver creates/amends situation (draft)
    ↓
Gate 2 lints the draft
    ↓
Published → replaces live version under same slug
    ↓
Previous version archived in version history
```

**What gets checked:**

For new situations:
- Narrative coherence — does the summary accurately represent its linked signals?
- Signal coverage — are all relevant signals attached? Any misattributed?
- Overlap — does this duplicate or substantially overlap an existing situation?
- Severity calibration — does the assessed severity match the evidence?

For amended situations (additional checks):
- Narrative evolution — does the update accurately reflect the new signals? Has the narrative drifted or distorted from the previous version?
- Signal continuity — were any previously linked signals dropped without justification?
- Dispatch consistency — do the dispatches still make sense in the context of the updated summary?

**Outcomes per situation:**
- **Publish** — situation is sound, replaces live version
- **Correct + publish** — adjust narrative, relink signals, recalibrate severity, then publish
- **Quarantine** — situation doesn't hold together, needs human review (live version remains unchanged)

### Mode 3: Admin Investigation

| | |
|---|---|
| **Trigger** | Manual — admin selects nodes in the UI and runs a prompt |
| **Input** | Selected signals, situations, entities, or any combination |
| **Graph access** | Full read |
| **Write access** | None — never writes to the graph |
| **Output** | Structured audit note |

**How it works:**
1. Admin selects one or more nodes in the admin UI
2. Optionally provides a prompt ("how are these related?", "what's wrong here?", "diagnose this situation")
3. AI investigates — builds Cypher queries, traverses relationships, pulls in context
4. Produces a structured audit note (never mutates data)

**Audit note structure:**

| Field | Description |
|-------|-------------|
| **Summary** | One-line finding |
| **Evidence** | Cypher queries run, node IDs examined, relationships traversed |
| **Diagnosis** | What's wrong and why, with references to specific nodes |
| **Recommendation** | Gate improvement, flag for removal, code change suggestion |
| **Severity** | Informational / Warning / Actionable |
| **References** | Links to examined nodes, source URLs, related audit notes |

The audit note is stored and visible in admin. It feeds back into improving the gates — if the admin investigation finds a pattern the gates should have caught, that becomes a new concern for Gate 1 or Gate 2.

## Data Lifecycle

```
Source content
    ↓
Signal extraction (draft)
    ↓
┌─────────────────────────┐
│  Gate 1: Signal lint     │  ← AI with scoped write access
│  Fix, publish, or reject │
└─────────────────────────┘
    ↓
Published signals (immutable)
    ↓
Situation weaver creates/amends situation (draft)
    ↓
┌──────────────────────────────┐
│  Gate 2: Situation lint       │  ← AI with scoped write access
│  Compare against prev version │
│  Fix, publish, or reject      │
└──────────────────────────────┘
    ↓
Published situation (replaces live version under same slug)
Previous version → archived in version history
Dispatches → append-only, individually immutable
    ↓
┌─────────────────────────┐
│  Admin investigation     │  ← AI with read-only access
│  Produces audit notes    │     Findings improve gates
└─────────────────────────┘
```

**Signals** are immutable after Gate 1. **Situations** are versioned — each version is immutable once published, but a new version can replace it (after passing Gate 2). **Dispatches** are append-only and never modified. Post-gate problems are gate problems. Fix the process, not the data.

## AI Tools by Mode

### Gate 1: Signal lint

Data provided via context loader — no freeform Cypher.

- `read_signal(id)` — get full signal node with all properties
- `fetch_source(url)` — retrieve archived source content
- `update_fields(id, changes, reason)` — update specific signal fields with audit trail
- `set_status(id, status, reason)` — publish or quarantine with reason

### Gate 2: Situation lint

Data provided via context loader — no freeform Cypher. Context includes the previous published version when amending.

- `read_situation(id)` — get full draft situation with all properties
- `read_previous_version(slug)` — get the currently published version for comparison (if amending)
- `read_linked_signals(situation_id)` — get all signals attached to a situation
- `read_dispatches(slug)` — get all dispatches for this situation
- `update_fields(id, changes, reason)` — update specific situation fields with audit trail
- `set_status(id, status, reason)` — publish (replaces live version) or quarantine (live version unchanged)
- `relink(situation_id, add_signals, remove_signals, reason)` — adjust signal attachments

### Admin investigation

Freeform exploration via Cypher sandbox. No write tools. No exceptions.

- `read_node(id)` — get full node with all properties
- `cypher_query(query)` — run Cypher through the sandbox (validated, logged, read-only)
- `fetch_source(url)` — retrieve archived source content
- `create_audit_note(summary, evidence, diagnosis, recommendation, severity, references)` — record findings

## Current State vs Target

### What exists today

**Signal lint (Gate 1 precursor):**
- Signal extraction writes signals with `review_status = "staged"` (string, not enum)
- `SignalLinter` runs as a Restate workflow after extraction
- Groups signals by source URL, fetches archived content, sends batch to LLM
- LLM returns `Pass`, `Correct { corrections }`, or `Reject { reason }` per signal
- Corrections applied, status set to `"live"` or `"rejected"`
- No `quarantined` state — binary pass/reject
- Uses same model tier as extraction (no stronger model for the gate)

**Situation weaver (Gate 2 precursor):**
- Weaver runs after synthesis, assigns unlinked signals to existing or new situations
- Situations have **no `review_status` field** — immediately queryable when created
- Properties updated in-place (headline, lede, structured_state patched directly)
- No draft stage, no gate before going live
- Post-dispatch verification exists but is rule-based (citation check, PII, uncited claims)
- No version history — only latest state stored

**Dispatches:**
- Already append-only and individually immutable
- `supersedes` field for corrections (lightweight version chain)
- `flagged_for_review` + `flag_reason` for quality issues
- Dispatch types: Update, Emergence, Split, Merge, Reactivation, Correction

### What needs to change

| Change | Current | Target |
|--------|---------|--------|
| **Review status type** | `String` on signals, missing on situations | `ReviewStatus` enum on both |
| **Status values** | `staged` / `live` / `rejected` | `Draft` / `Published` / `Quarantined` / `Rejected` |
| **Situation draft stage** | None — immediately live | Weaver writes as `Draft`, Gate 2 promotes to `Published` |
| **Situation versioning** | Mutable singleton, in-place updates | Version replacement — new version replaces live under same slug, previous versions archived |
| **Gate model tier** | Same as extraction | Stronger model for gates (final check) |
| **Quarantine state** | Doesn't exist | Gate flags uncertain cases for human review |
| **Situation lint** | Rule-based post-dispatch checks only | Full AI gate with narrative coherence, drift detection, amendment comparison |
| **Admin investigation** | Doesn't exist | Read-only AI with Cypher sandbox, produces audit notes |
| **Public API filtering** | No filtering (all situations visible) | Filter to `review_status = Published` only |

### What stays the same

- Signal extraction pipeline (unchanged — just writes `Draft` instead of `staged`)
- Situation weaver logic (unchanged — just writes `Draft` instead of immediately live)
- Dispatch model (already append-only, immutable, with supersedes chain)
- Temperature / arc derivation (computed from graph, unaffected by status)
- Signal lint workflow structure (Restate workflow — enhanced, not replaced)

## Key Decisions

- **One system, three modes** over separate tools: same core capability, different permissions. Less code, consistent behavior.
- **Blocking gates** over optimistic publish: nothing goes live without passing a stronger model. Draft → lint → publish.
- **Signals immutable, situations versioned**: signals never change after Gate 1. Situations evolve via version replacement — each version passes Gate 2, previous versions archived. Dispatches are append-only. No direct patches to published data.
- **Gates get data via context loader, not Cypher**: gates always know exactly what to check — the draft nodes from the current run. No query construction overhead, no validation needed, cheaper to run.
- **Freeform Cypher only for admin investigation**: the only mode that genuinely needs to explore the graph. Routed through the Cypher sandbox.
- **Cypher sandbox as a standalone module**: generic library for validating Cypher against a permission set. Doesn't know about linting — reusable anywhere an AI needs controlled graph access.
- **Lint concerns in natural language**: the AI reasons about concerns, not pattern-matches against rules. Plain text descriptions, not structured rule DSLs.
- **Stronger model for gates**: extraction uses a fast model for throughput. Gates use a stronger model for accuracy. Worth the cost — this is the last check.
- **Audit notes over direct fixes** for admin mode: investigation findings are documentation, not mutations. They drive process improvement.
- **Unified `ReviewStatus` enum** for signals and situations: `Draft` → `Published` / `Quarantined` / `Rejected`. Replaces the current string-based status on signals and adds status to situations. Consistent state machine across the pipeline.
- **Supersedes signal-lint brainstorm** (2026-02-24): this is the expanded vision. Signal lint becomes Gate 1 within this system.

## Open Questions

- **Gate failure handling**: if a gate quarantines a signal that a situation depends on, does the situation get re-evaluated?
- **Audit note storage**: graph node? Postgres? Separate collection? Needs to be searchable and linkable.
- **Admin UI for investigation**: inline panel in existing pages, or a dedicated "investigate" page?
- **Quarantine lifecycle**: how long do quarantined nodes stay before pruning?
- **Cypher sandbox implementation**: full AST parser, or simpler regex/pattern-based validation? AST is more robust but more work.
- **Version history storage**: separate nodes per version linked to the slug? A version array on the situation node? Needs to be efficient to query for "current" while retaining full history for admin investigation.

## Next Steps

→ `/workflows:plan` for implementation details, starting with Gate 1 (signal lint) since it supersedes the existing signal-lint brainstorm.
