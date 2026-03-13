# Scout Event Audit

**Date:** 2026-03-07 (refreshed)
**Scope:** Every event emitted in the scout engine causal chain
**Goal:** Verify events are domain facts, not commands in disguise

---

## Genuine Facts (these belong)

Events that record something that actually happened — a real observation, decision, or outcome.

| Event | Why it's a fact |
|---|---|
| **WorldEvent::\*** (all variants) | Observable facts about the world |
| **SystemEvent::\*** (most) | Editorial decisions the system made |
| **TelemetryEvent::\*** (all) | Infrastructure observations |
| **ScrapeEvent::WebScrapeCompleted** | "We scraped N URLs and extracted N signals" |
| **ScrapeEvent::SocialScrapeCompleted** | "We scraped N social sources" |
| **ScrapeEvent::TopicDiscoveryCompleted** | "We searched N topics and found N signals" |
| **SignalEvent::DedupCompleted** | "Dedup ran and produced these verdicts" |
| **DiscoveryEvent::SourcesDiscovered** | "We found new sources from these links" |
| **DiscoveryEvent::ExpansionQueryCollected** | "This query was collected for future expansion" |
| **DiscoveryEvent::SocialTopicCollected** | "This topic was collected" |
| **DiscoveryEvent::SocialTopicsDiscovered** | "These topics were discovered" |
| **ExpansionEvent::ExpansionCompleted** | "Expansion ran and found N sources/queries" |
| **PipelineEvent::HandlerFailed** | "Handler X failed after N attempts" |

Note: scrape completion events also carry `extracted_batches: Vec<UrlExtraction>` — in-memory state handed off to signals dedup via event payload rather than stashing on PipelineState.

---

## Commands in Disguise

Events that don't record a domain fact — they exist because handler B needs to know handler A finished.

### 1. `LifecycleEvent::SourcesPrepared`

Massive state-carrier (tension_count, response_count, source_plan, actor_contexts, url_mappings, web_urls, web_source_keys, pub_dates, query_api_errors). Hands off a blob of data to scrape handlers. Really saying "go scrape these things now."

The *fact* is "sources were queried and classified" but the payload is structured as instructions for downstream handlers.

### 2. `LifecycleEvent::NewsScanRequested`

Explicitly a command. The name even says "Requested."

### 3. Enrichment markers: `ActorsExtracted`, `DiversityScored`, `ActorStatsComputed`, `ActorsLocated`

Four unit-variant marker events. Each enrichment handler emits one when it finishes. Downstream handlers gate on all four being present. These don't describe what enrichment *found* — just that a slot completed. The actual findings are emitted as SystemEvents (ActorIdentified, SignalDiversityComputed, etc.).

### 4. Synthesis markers: `SimilarityComputed`, `ResponsesMapped`, `SeverityInferred`

Same barrier pattern. Each synthesis handler emits one. `weave_situations` gates on the superset. The actual results are SystemEvents (SimilarityEdgesRebuilt, ResponseLinked, etc.).

### 5. `DiscoveryEvent::SourceExpansionCompleted` / `SourceExpansionSkipped`

Pure coordination. Tells `scrape:resolve_new_source` "expansion is done (or skipped), you can start the response phase now."

### 6. `ScrapeEvent::SourcesResolved`

State-carrier like `SourcesPrepared`. Hands off `web_urls`, `web_source_keys`, `url_mappings`, `pub_dates` to response-phase scrape handlers. Really means "go scrape these response sources now."

### 7. `ScrapeEvent::ResponseScrapeSkipped`

Terminal coordination signal: "there's nothing to scrape in the response phase, so skip ahead."

### 8. `SituationWeavingEvent::SituationsWeaved` / `NothingToWeave`

Coordination to trigger supervisor. The actual facts (situations created, signals assigned) are already emitted as SystemEvents.

### 9. `SupervisorEvent::SupervisionCompleted` / `NothingToSupervise`

Terminal signal that marks end-of-run. The actual findings are SystemEvents.

---

## Patterns

The coordination events fall into three categories:

### Barrier events (fan-in)

Enrichment markers (`ActorsExtracted`, `DiversityScored`, `ActorStatsComputed`, `ActorsLocated`) and synthesis markers (`SimilarityComputed`, `ResponsesMapped`, `SeverityInferred`). These implement "wait for N parallel things to finish." They encode pipeline topology, not domain facts.

### Handoff events (state carriers)

`SourcesPrepared`, `SourcesResolved`. These pass a blob of computed state to the next handler. The handler that receives them doesn't care *that* something happened — it cares about the data payload so it can do its work.

### Phase-complete signals

`SourceExpansionCompleted`, `SituationsWeaved`, `SupervisionCompleted`, `ResponseScrapeSkipped`. Just "my phase is done, trigger the next phase."

---

## Also Suspicious

- `LifecycleEvent::ScoutRunRequested` — intentionally a command (entry point), fine as-is.
- `DiscoveryEvent::ExpansionQueryCollected` and `SocialTopicCollected` — may not be directly emitted by discovery handlers. Possibly dead.

---

## Full Causal Chain

```
ScoutRunRequested
├─ lifecycle:find_stale_signals → SystemEvent::SignalsExpired
├─ lifecycle:prepare_sources → SourcesPrepared
│  ├─ scrape:start_web_scrape → WebScrapeCompleted
│  ├─ scrape:start_social_scrape → SocialScrapeCompleted
│  └─ discovery:bootstrap_sources → SourcesDiscovered
│     └─ discovery:filter_domains → SystemEvent::SourcesRegistered
│
├─ [on any ScrapeCompleted / TopicDiscoveryCompleted]:
│  ├─ signals:dedup_signals → DedupCompleted + SystemEvents
│  ├─ discovery:promote_links → SourcesDiscovered
│  └─ discovery:expand_sources → SourceExpansionCompleted/Skipped
│     └─ scrape:resolve_new_source → SourcesResolved (Phase B)
│        ├─ scrape:process_web_results → WebScrapeCompleted
│        ├─ scrape:process_social_results → SocialScrapeCompleted
│        └─ scrape:discover_topics → TopicDiscoveryCompleted
│
├─ [on response-phase ScrapeCompleted]:
│  ├─ enrichment:extract_actors → ActorsExtracted + SystemEvents
│  ├─ enrichment:score_diversity → DiversityScored + SystemEvents
│  ├─ enrichment:compute_actor_stats → ActorStatsComputed + SystemEvents
│  └─ enrichment:resolve_actor_locations → ActorsLocated + SystemEvents
│
├─ [all enrichment done]:
│  └─ expansion:expand_signals → ExpansionCompleted + SourcesDiscovered
│
├─ [on ExpansionCompleted]:
│  ├─ synthesis:compute_similarity → SimilarityComputed + SystemEvents
│  ├─ synthesis:map_responses → ResponsesMapped + SystemEvents
│  └─ synthesis:infer_severity → SeverityInferred + SystemEvents
│
├─ [all synthesis done]:
│  └─ situation_weaving:weave_situations → SituationsWeaved/NothingToWeave + SystemEvents
│
└─ [on SituationsWeaved/NothingToWeave]:
   └─ supervisor:run_supervisor → SupervisionCompleted/NothingToSupervise + SystemEvents
```

---

## Handler Reference

| Handler ID | Input Event | Output Events |
|---|---|---|
| lifecycle:find_stale_signals | ScoutRunRequested | SystemEvent:SignalsExpired |
| lifecycle:prepare_sources | ScoutRunRequested | SourcesPrepared |
| discovery:bootstrap_sources | ScoutRunRequested | SourcesDiscovered |
| discovery:filter_domains | SourcesDiscovered | SystemEvent:SourcesRegistered |
| discovery:promote_links | ScrapeEvent (completion) | SourcesDiscovered |
| discovery:expand_sources | ScrapeEvent (tension completion) | SourceExpansionCompleted/Skipped |
| scrape:start_web_scrape | SourcesPrepared | WebScrapeCompleted |
| scrape:start_social_scrape | SourcesPrepared | SocialScrapeCompleted |
| scrape:resolve_new_source | SourceExpansionCompleted/Skipped | SourcesResolved / ResponseScrapeSkipped |
| scrape:process_web_results | SourcesResolved (response) | WebScrapeCompleted |
| scrape:process_social_results | SourcesResolved (response) | SocialScrapeCompleted |
| scrape:discover_topics | SourcesResolved (response) | TopicDiscoveryCompleted |
| signals:dedup_signals | ScrapeEvent (completion) | DedupCompleted + SystemEvents |
| enrichment:extract_actors | ScrapeEvent (response completion) | ActorsExtracted + SystemEvents |
| enrichment:score_diversity | ScrapeEvent (response completion) | DiversityScored + SystemEvents |
| enrichment:compute_actor_stats | ScrapeEvent (response completion) | ActorStatsComputed + SystemEvents |
| enrichment:resolve_actor_locations | ScrapeEvent (response completion) | ActorsLocated + SystemEvents |
| expansion:expand_signals | all enrichment done | ExpansionCompleted + SourcesDiscovered |
| synthesis:compute_similarity | ExpansionCompleted | SimilarityComputed + SystemEvents |
| synthesis:map_responses | ExpansionCompleted | ResponsesMapped + SystemEvents |
| synthesis:infer_severity | SimilarityComputed + ResponsesMapped | SeverityInferred + SystemEvents |
| situation_weaving:weave_situations | all synthesis done | SituationsWeaved/NothingToWeave + SystemEvents |
| supervisor:run_supervisor | SituationsWeaved/NothingToWeave | SupervisionCompleted/NothingToSupervise + SystemEvents |

---

## Projection Rules

- **World + System events** → projected to Neo4j graph via `neo4j_projection_handler`
- **Telemetry events** → projected to events table, printed via `system_log_projection`
- **Domain coordination events** → persisted to event store but NOT projected to graph
- **PipelineEvent::HandlerFailed** → persisted, counted in stats, not projected to graph
