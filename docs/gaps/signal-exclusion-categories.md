# Signal Inclusion and Exclusion Categories — Phenomena vs. Interpretation

## Purpose

The graph captures **phenomena** — things that exist or occur in the world independent of what anyone thinks or says about them. The lint gate question is always the same: **what in the world does this point to?**

If the answer is a citable event, act, condition, or artifact, it belongs. If the answer is another claim, an inference, or a projection, it doesn't.

---

## What Belongs — Phenomena

These are the categories of things the graph is built to hold. All of them can be pointed to. They exist or occurred outside of anyone's interpretation of them.

### Events

Something happened. A meeting was held. A vote was cast. A building was demolished. A species was listed. A storm made landfall. Discrete, datable, locatable occurrences.

### Conditions

Something exists in a persistent state. A neighborhood has no grocery store within two miles. A river is above flood stage. A building is condemned. A species population is below recovery threshold. Conditions are events that haven't resolved — they are the current state of something real.

### Statements

When a person or organization makes a public statement, that statement is itself an event. Not the belief behind it — the act of stating. A city council member said this, on this date, in this forum. That happened. It can be cited.

### Decisions

A policy was passed. A permit was granted. A contract was awarded. A lawsuit was filed. Decisions are events with actors, dates, and consequences that can be traced.

### Relationships

This organization receives funding from this source. This official sits on this board. This company owns this property. Documented, public, traceable connections between entities.

### Ecological phenomena

A wildfire burned this area. A wetland was filled. A migratory species stopped appearing. Water quality at this location tested at this level. The natural world generates real signal that belongs alongside the human signal.

### Absences

When something that should exist demonstrably doesn't. A neighborhood with no emergency services within response time. A community with no public green space. Absence is real when it can be documented.

---

## What Gets Flagged for Removal — Interpretation

These are categories the lint gate explicitly flags for removal. They share a common property: they describe states of mind, interpretive frames, or projections — not discrete things that happened and can be pointed to in the world.

### 1. Beliefs and ideas

"People believe X." "The community thinks Y." Beliefs are internal states. What belongs in the graph is the act that expresses the belief — a public statement, a vote, an organized action. The belief itself is an inference from those acts.

### 2. Predictions and forecasts

"This will happen." Even expert predictions. They aren't events yet. What belongs is when the predicted thing actually happens, or when the prediction was made as a documented act by a named entity — a published report, a council vote on a projection, a public filing.

### 3. Sentiment and opinion

"People are angry about X." Anger isn't an event. A protest is. A public statement is. A petition is. The feeling underneath doesn't belong — the visible acts that express it do.

### 4. Reputation and character claims

"This organization is corrupt." "This person is trustworthy." These are judgments, not events. What belongs is the documented acts that lead people to those conclusions — the audit finding, the lawsuit, the public record.

### 5. Trends and narratives

"There is a growing movement." "The mood is shifting." These are interpretive frames applied to collections of events, not events themselves. The underlying events belong. The narrative someone builds from them doesn't.

### 6. Intentions and motivations

"They did this because they wanted to..." Motivation is inference. The act belongs. The why behind it only belongs if it was explicitly stated publicly and can be cited — a press release, a public comment, a recorded statement.

### 7. Aggregated statistics without provenance

"Homelessness is up 20%." Where did that number come from? Who counted? When? A statistic without its methodology and source is closer to a claim than an event. What belongs is the published report with its author, date, and methodology — the statistic as a documented artifact, not as a floating fact.

---

## The Pattern

The graph holds **things that happened** — acts, artifacts, events, published documents, public statements. It does not hold interpretations of why things happened, predictions of what will happen, judgments about who is good or bad, or feelings about what's going on. Those are what humans and downstream consumers derive from the graph. The graph is the substrate, not the commentary.

---

## Relationship to Existing Principles

- The inclusion test's "grounded" requirement (`editorial-and-signal-inclusion-principles.md`) already implies most of this — these categories make the exclusions explicit and enumerable.
- The first-hand principle filters the *source*. These categories filter the *content type*. Both apply.
- The lint gate (Gate 1 in `ai-graph-linter`) is where enforcement happens. These categories become part of its concern set.

---

## Gate Enforcement

When the lint gate encounters content that falls into one of these categories, the question is not "is this true?" but "what event does this point to?"

- If it points to a citable event, the event is the signal. Strip the interpretive frame and keep the event.
- If it points to nothing concrete, quarantine or reject.
- If a statistic is cited, the published report is the artifact. The statistic enters the graph attached to its source document, not as a free-floating claim.
