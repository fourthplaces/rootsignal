# Scout Architecture

Scout is the automated signal collection engine for Root Signal. It discovers, extracts, deduplicates, and graphs **signals** — actionable information about community resources, events, needs, and tensions.

## Documents

| Document | Contents |
|----------|----------|
| [Overview](overview.md) | System overview, data model, module map |
| [Event Engine](event-engine.md) | Seesaw engine, dispatch loop, aggregate, projection |
| [Event Taxonomy](event-taxonomy.md) | Three-layer event taxonomy with every event type |
| [Event Flow](event-flow.md) | Complete causal chain from EngineStarted to RunCompleted |
| [Domains](domains.md) | Each domain's handlers, events, activities, and responsibilities |
| [Dedup Pipeline](dedup-pipeline.md) | 4-layer signal deduplication |
| [Testing](testing.md) | Test philosophy, levels, patterns |
| [Event-Sourcing Exceptions](event-sourcing-exceptions.md) | Components that bypass the event-sourced chain |
| [Known Gaps](known-gaps.md) | Architectural gaps and future work |
