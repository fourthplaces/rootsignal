# Power Scout: A Separate Crate for Structural Analysis

*Status: Vision / future work. Not currently in the pipeline.*

## The Missing Force

The scout pipeline has three irreducible investigation modes (see [investigation-modes.md](../architecture/investigation-modes.md)):

| Force | Question | Discovers |
|-------|----------|-----------|
| Diagnostic | "Why does this signal exist?" | Tensions |
| Instrumental | "What diffuses this?" | Solutions |
| Solidarity | "Where are people gathering?" | Community formation |

These are **observational** — they discover things that exist right now. Signals, tensions, organizations, events, gatherings. You can point to a URL, visit an event, call a legal clinic. The evidentiary standard is "does this thing exist?"

There's a fourth force that's categorically different: **"What keeps this wound open?"**

A community has a housing affordability crisis. The curiosity loop discovered the tension. The response scout found legal clinics and tenant advocacy orgs. The gravity scout found tenant solidarity potlucks and packed city council meetings. But nobody asked: *why does the city council keep voting down rent stabilization?* Who benefits from the status quo? What institutional forces maintain this structural condition?

This is the **structural** force. It's about why tensions persist — not what they are, not what solves them, not where people gather around them, but what sustains them.

## Why a Separate Crate

Power analysis is not a fourth atom in the scout pipeline. It's a different *category* of investigation with its own atoms.

The scout pipeline is observational and present-tense. Power analysis is structural, historical, and institutional. The differences are fundamental:

**Different evidentiary bar.** If the gravity scout gets something wrong, worst case is a stale event that ages out in 60 days. If the power analysis incorrectly attributes an action to an actor — "Mayor Smith blocked affordable housing" when it was a committee vote — that's misattribution with reputational consequences. Power claims need higher accuracy than "here's an event happening Tuesday."

**Different source ecosystem.** The scout uses Tavily web search, Chrome scraping, and social media APIs. Power analysis needs government databases, legislative records, campaign finance data, public meeting minutes, voting records, budget documents. Different tools entirely.

**Different pacing.** The scout runs every few days to stay current with community signals. Power structures change slowly. Zoning laws don't shift weekly. Legislative sessions have their own cadence. Weekly or monthly analysis is sufficient.

**Different complexity.** Power has many atoms of its own (see below). Cramming them into the scout pipeline would complicate a system that currently has clean, well-separated concerns.

**Different sensitivity.** The scout surfaces things that exist. Power surfaces *causal relationships* between actors and tensions. Even "just raising relationships" requires choosing which relationships to raise — and that's a framing choice. A separate crate can have its own review pipeline, accuracy thresholds, and sensitivity handling.

## The Atoms of Power

Power analysis isn't one question — it's several:

| Atom | Question | Discovers | Sources |
|------|----------|-----------|---------|
| **Policy** | "What rules sustain this tension?" | Zoning laws, legislation, regulations, executive orders | Legislative databases, government sites, legal analysis |
| **Money** | "Where does funding flow?" | Budget allocations, lobbying spend, campaign contributions, grants | Campaign finance records, budget documents, lobbying disclosures |
| **Actors** | "Who are the institutional players?" | Elected officials, agencies, corporations, advocacy orgs — their positions and actions | News archives, public statements, organizational websites |
| **Record** | "What did they promise vs. what did they do?" | Voting records, public commitments, accountability gaps | Legislative voting records, news archives, campaign platforms |
| **History** | "What decisions created this structural condition?" | Redlining, historical policy choices, institutional decisions that still echo | Historical records, academic research, investigative journalism |

Each has different search strategies, different source types, and different verification needs.

## Examples

### Housing Affordability (Minneapolis)

**Policy:** Minneapolis zoning allows single-family-only in 70% of residential land. State legislature preempted local rent control in 1984.

**Money:** Top 5 campaign contributors to city council members who voted against rent stabilization. Development company lobbying spend.

**Actors:** Which council members voted which way. Which developers testified at hearings. Which advocacy organizations pushed for/against.

**Record:** Council member X campaigned on affordable housing in 2023, voted against rent stabilization in 2024.

**History:** Redlining maps from the 1930s overlay almost exactly with today's lowest-income neighborhoods.

Each of these is a documentable, verifiable relationship. `(Minneapolis City Council) -[SHAPES {action: "voted down rent stabilization 3x", dates: ["2023-03", "024-01", "2024-09"]}]-> (Housing Affordability Crisis)` is a fact in a graph.

### Immigration Enforcement Fear (Twin Cities)

**Policy:** 287(g) agreements between local police and ICE. Federal executive orders expanding enforcement priorities. State policies on sanctuary status (or lack thereof).

**Money:** Federal funding tied to immigration enforcement cooperation. ICE detention contracts with private companies.

**Actors:** County sheriff's cooperation posture. Federal officials directing enforcement. Community organizations challenging enforcement.

**Record:** Did the city actually implement its declared sanctuary policies? Gap between stated policy and on-the-ground reality.

**History:** How immigration enforcement patterns have shifted across administrations.

### Youth Violence (North Minneapolis)

This is where power analysis gets hard. The sustaining forces are **diffuse** — historical disinvestment, poverty, gun access, underfunded schools, systemic racism. You can't point to one actor's specific action. But you can trace:

**Policy:** Funding formulas that shortchange North Minneapolis schools. Zoning that concentrates poverty. Gun access laws at the state level.

**Money:** Per-capita public investment in North Minneapolis vs. other neighborhoods. Youth program funding trends over 10 years.

**History:** Redlining, highway construction that bisected the community, urban renewal displacement.

The diffuse cases are harder but potentially the most valuable — they surface the structural conditions that no single actor controls but that institutional choices collectively maintain.

## Relationship to the Scout Pipeline

The scout discovers tensions and community responses. The power crate takes those tensions and asks why they persist. The data flows in both directions:

**Scout → Power:** High-heat tensions become targets for power analysis. The scout feeds the power system's target queue.

**Power → Stories:** Power relationships become context for story weaving. A story about housing affordability that includes the tension, the legal clinics, the tenant potlucks, *and* the three failed rent stabilization votes is a complete portrait of a community wound.

**Power → Community:** The most radical thing Root Signal can do is make structural relationships visible. Not editorialize. Not take sides. Just raise the relationships that exist and let the community interpret them. "Here's who voted how on the thing that affects you" is information that's technically public but practically invisible.

## The "Just Raising Relationships" Principle

Power analysis is inherently political. The charge is real. But the system can take some of the charge out by being rigorously factual and relational:

- Surface *actions*, not *motives*. "Council member X voted against rent stabilization" — not "Council member X doesn't care about tenants."
- Surface *relationships*, not *judgments*. The graph shows connections between actors and tensions. The community draws conclusions.
- Surface *records*, not *accusations*. Voting records, public statements, budget allocations — all publicly available information, organized and made visible.

The power isn't in editorializing. It's in **making the already-public legible**. Most civic information is technically accessible but practically invisible — buried in meeting minutes, scattered across government websites, fragmented across news cycles. Organizing it into a graph of actor-tension relationships is a service, not an opinion.

## Open Questions

- What's the right verification pipeline for power claims? Human review before publishing? Confidence thresholds? Source requirements?
- How do you handle contested causation? Immigration enforcement: one framing says federal policy sustains fear, another says unauthorized immigration itself sustains enforcement. The system has to navigate this.
- How do you handle diffuse vs. specific causation? "Systemic disinvestment" is real but not a SHAPES edge. "City allocated $X per capita to North Minneapolis vs. $Y to Southwest" is.
- What's the right pacing? Monthly? Per legislative session? Triggered by heat spikes?
- What APIs and data sources are needed? OpenStates for legislation? FEC for campaign finance? Local government meeting minutes?
