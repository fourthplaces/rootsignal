# Evidence Surfacing Audit — 2026-02-17

Scrutiny of the web layer evidence surfacing work against vision, emergence principles, and data model.

## What's Working Well

### Evidence as provenance, not authority
The web layer passes evidence through faithfully — URLs, snippets, counts — without editorializing. Aligns with: *serve signal, not algorithm*. Humans see the evidence trail and make their own trust decisions.

### Snippet surfacing closes a real gap
Before this change, evidence was a bare URL requiring click-through. Now the snippet gives immediate context about *what* the evidence says. The signal itself carries meaning, not just a pointer.

### Evidence counts on stories support pattern recognition
`evidence_count` on the story list answers "how grounded is this cluster?" without requiring drill-down. Stories with deep evidence trails vs. thin ones tell you where the system has confidence and where it doesn't.

---

## Findings

### 1. Evidence relevance type is dropped

The investigator evaluates each piece of evidence as `DIRECT`, `SUPPORTING`, or `CONTRADICTING` (the `relevance` field in `EvidenceItem`). But this is never stored in `EvidenceNode` — it's lost at write time. A contradicting evidence node looks identical to a direct one. This undermines "let humans make their own trust decisions" — we're withholding the one piece of semantic information the LLM already produced.

- [x] Add `relevance: Option<String>` to `EvidenceNode` in `rootsignal-common/src/types.rs`
- [x] Store `relevance` in the Evidence graph node (`writer.rs` `create_evidence`)
- [x] Parse `relevance` from graph in `extract_evidence()` (`reader.rs`)
- [x] Surface `relevance` in `EvidenceView` and all API/HTML evidence rendering
- [x] Pass `relevance` through from `EvidenceItem` in `investigator.rs`

### 2. Evidence confidence is dropped

The investigator scores each evidence 0.0–1.0 (filtering below 0.5), but the surviving score isn't stored. Two evidence nodes at 0.51 and 0.99 look the same. Evidence quality *is* signal quality.

- [ ] Add `confidence: Option<f32>` to `EvidenceNode`
- [ ] Store evidence confidence in the graph
- [ ] Parse and surface in reader/web layer
- [ ] Use confidence to order evidence display (highest first)

### 3. Tension nodes are semantically thin

`TensionNode` has `severity` and shared `NodeMeta`, but no fields for *what the tension is between*, *what would resolve it*, or *what responses exist*. Compare to `AskNode` which has `what_needed` and `goal`. Tension is the node type the whole theory of change revolves around, but it's the least expressive.

- [ ] Evaluate whether `TensionNode` needs `category: Option<String>`
- [ ] Evaluate a `what_would_help: Option<String>` field
- [ ] Design a `RESPONDS_TO` edge type (Ask/Give/Event → Tension) to close the feedback loop
- [ ] Update LLM extraction prompts to populate new Tension fields
- [ ] Surface tension→response linkage in story detail views

### 4. No story-level evidence path

Evidence is always mediated by signals (`Story → Signal → Evidence`). If investigation discovers evidence about a *story pattern* rather than a single signal, there's nowhere to put it.

- [ ] Evaluate whether stories need a direct `SOURCED_FROM` relationship to evidence
- [ ] Decide if this is needed now or deferred until investigation evolves

### 5. N+1 queries in `api_story_detail`

Each signal in a story triggers `get_signal_evidence()` which iterates 5 node types. A story with 8 signals means up to 40 queries.

- [ ] Add `get_story_signal_evidence(story_id)` batch method to `reader.rs`
- [ ] Cypher: `MATCH (s:Story {id: $id})-[:CONTAINS]->(n)-[:SOURCED_FROM]->(ev:Evidence) RETURN n.id AS signal_id, collect(ev) AS evidence`
- [ ] Replace per-signal loop in `api_story_detail` with single batch call

### 6. Feedback loop not visible

The vision describes: *"when needs stop clustering, the graph gets quiet — silence signals alignment was restored."* But the web layer has no temporal dimension. Stories have `velocity` and `energy` but these aren't surfaced.

- [ ] Add `velocity` and `energy` to `api_stories` response
- [ ] Add `velocity` and `energy` to `api_story_detail` response
- [ ] Consider a `trend` indicator (heating/cooling) derived from velocity history
- [ ] Surface temporal change in story list UI (e.g. rising/falling badges)

### 7. No response-tension linkage surfaced

The vision's core emergent property: *"Needs/Tensions = misalignment, Responses/Resources = alignment being restored."* But the web layer treats all signals in a story as a flat list. No way to see which Gives/Events respond to which Asks/Tensions.

- [ ] Design `RESPONDS_TO` edge semantics and creation logic
- [ ] Update clustering or investigation to detect and link response→tension pairs
- [ ] Surface grouped tension→response structure in `api_story_detail`
- [ ] Render tension→response grouping in story detail HTML

---

## Priority Order

1. **Store and surface evidence relevance** — low effort, high trust value (finding 1)
2. **Store and surface evidence confidence** — low effort, completes evidence semantics (finding 2)
3. **Batch story evidence query** — low effort, fixes scaling issue (finding 5)
4. **Surface story velocity/energy** — already computed, just not exposed (finding 6)
5. **Enrich Tension semantics** — medium effort, unlocks the feedback loop (finding 3)
6. **Design tension→response linkage** — larger design work, core to emergence (finding 7)
7. **Story-level evidence** — deferred, evaluate when investigation matures (finding 4)
