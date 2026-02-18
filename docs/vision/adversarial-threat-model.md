# Root Signal — Adversarial Threat Model

## What This Document Is

Root Signal exists to make civic activity visible and actionable. That same visibility creates a surveillance surface. A system that maps who is organizing, what they're organizing about, where they gather, and how they respond to tensions is extraordinarily valuable to the people it serves — and equally valuable to those who would suppress, surveill, or target them.

This document names every adversarial threat the system creates or amplifies, assesses the severity, and defines structural mitigations. "Structural" means: designed into the architecture so the protection can't be toggled off, overridden by a future maintainer, or circumvented by a subpoena.

This is not a paranoid exercise. These are real threats. Activist surveillance by law enforcement is documented and ongoing. Corporate intelligence operations target organizers. Governments at every level have used public data to identify, track, and suppress dissent. If Root Signal succeeds, it becomes a target. This document exists to make sure it's not also a weapon.

---

## The Fundamental Tension

Root Signal's value proposition is: **make civic reality visible so people can act on it.**

The adversarial inversion is: **make civic reality visible so people can be targeted for acting on it.**

These use the exact same data. The system cannot make organizing more discoverable without also making organizers more discoverable. Every mitigation in this document navigates that tension — preserving the value of visibility while limiting the surface for surveillance and targeting.

The honest answer is that this tension cannot be fully resolved. It can only be managed through deliberate architectural choices about what the system stores, what it surfaces, how it surfaces it, and what it refuses to build.

---

## Threat Actors

### 1. Government and Law Enforcement
**Threat level: High**

Federal agencies (ICE, FBI, DHS), state law enforcement, and local police have documented histories of surveilling activists, infiltrating organizing groups, and using public data to identify and target individuals.

**What they want from Root Signal:**
- A map of who is organizing against their operations
- Real-time location data on activist gatherings
- Network graphs showing which people and organizations are connected
- Historical data on who has been involved in what actions over time
- Identity information on people reporting sensitive signal

**Specific scenarios:**
- ICE queries the API for Response nodes connected to immigration-related Tensions — but this information is already public (org websites, GoFundMe pages, public social media posts). Root Signal aggregates public signal; it doesn't create new exposure. The mitigations (no actor timelines, no network graph exposure, geographic fuzziness) limit what's queryable beyond what's already on the open web
- FBI uses the actor graph to map connections between advocacy organizations, identifying coordination patterns
- Local police use heat map data to pre-position resources at anticipated protest locations
- A subpoena demands server logs, query logs, or the graph itself

### 2. Corporate Intelligence
**Threat level: Medium-High**

Corporations targeted by boycotts, accountability campaigns, or environmental justice organizing have material incentives to identify and counter organizers. Corporate intelligence is a real industry.

**What they want from Root Signal:**
- Which organizations are leading campaigns against them
- Who funds those organizations (if "follow the money" works in both directions)
- Where boycott organizing is concentrated geographically
- How effective campaigns are (by tracking Response node density over time)
- Early warning on emerging tensions that name their company

**Specific scenarios:**
- A corporation facing a boycott monitors the graph for Response nodes connected to their Actor node, tracking who's organizing and where
- A company uses temporal analysis to measure whether a campaign is growing or fading, and adjusts their counter-strategy
- Corporate lawyers use the graph as evidence in lawsuits against organizers

### 3. Doxxing and Harassment Campaigns
**Threat level: Medium-High**

Online harassment campaigns target individuals — often women, people of color, LGBTQ+ people, and immigrants — using publicly available information to identify, locate, and threaten them.

**What they want from Root Signal:**
- Identity information on people who reported signal (especially sensitive signal)
- Location data that can be narrowed to an individual's home or workplace
- Organizational affiliations that reveal someone's political or social positions
- Any data that connects a pseudonymous online identity to a real-world person or location

**Specific scenarios:**
- Someone who reported a mutual aid need via direct intake gets doxxed because their submission metadata was stored
- An organizer's home address is inferred from the geographic clustering of events they organize
- A harassment campaign floods the system with false reports to discredit an organization or individual

### 4. Bad-Faith Data Submitters
**Threat level: Medium**

People who submit false or manipulative signal through human-reported channels or by gaming public platforms that Root Signal scrapes.

**What they want:**
- To map where people are by submitting false needs and seeing who responds
- To flood the graph with disinformation that degrades trust in the system
- To poison data about specific organizations or individuals
- To trigger false crisis-mode responses
- To create fake signals that waste community resources

**Specific scenarios:**
- An adversary submits fake ICE sighting reports to cause panic, then monitors the response to identify who's organized to resist enforcement
- A bad actor creates fake GoFundMe campaigns that get scraped into the graph, discrediting Root Signal when they're exposed
- Coordinated false reports about an organization drive down its trust score

### 5. Future System Operators
**Threat level: Medium**

If Root Signal succeeds and is deployed by cities, institutions, or other operators, those operators may face political pressure to use the system in ways that violate its principles — to suppress certain signal, to identify organizers, or to share data with law enforcement.

**What they want:**
- Admin access to query logs, unredacted data, or the full graph
- The ability to suppress signal about topics that are politically inconvenient
- Data-sharing agreements with law enforcement or other agencies

---

## Attack Surfaces

### The Graph Itself

The knowledge graph is the primary attack surface. It contains:
- **Actor nodes** with edges showing who organizes what, who funds whom, who responds to which tensions
- **Place nodes** showing where organizing happens at potentially precise geographic resolution
- **Evidence nodes** linking back to specific posts, articles, and reports — timestamped and attributed
- **Temporal data** showing when activity happened, enabling pattern analysis

**Risk:** The graph, if fully queryable, is a comprehensive surveillance database of civic activity. Its value for civic engagement is inseparable from its value for surveillance.

### The API

An open, public API is a core principle. But an open API means anyone can query it — including every threat actor above.

**Risk:** Rate limiting and access controls can slow down bulk extraction but can't prevent a determined actor from querying the same data a legitimate user would.

### Human-Reported Signal

People submitting signal directly (SMS, web form, email, voice) create a two-way channel. The system knows something about the submitter — at minimum, the channel they used. If metadata is stored, it can be traced.

**Risk:** Human-reported signal is the highest-quality signal source but also the most privacy-sensitive. A subpoena for submission records could expose people who reported sensitive information.

### Heat Maps and Aggregate Views

Geographic visualizations of signal density — where organizing is concentrated, where needs are highest, where tensions are hottest — are powerful for community awareness and for surveillance alike.

**Risk:** Geographic visualizations aggregate public signal in ways that reveal patterns. However, these organizations are already publicly broadcasting their work — sanctuary churches, legal aid clinics, and community orgs *want* to be found by the people they serve. The mitigation is aggregate-only heat maps for sensitive domains (density without individual node links), not suppression of the signal itself.

### Query Patterns

Even without query logging, the system's behavior reveals information. If synthesis agents generate different responses based on query context ("I'm undocumented"), the system's output leaks information about what it knows.

**Risk:** An adversary can probe the system with structured queries to reverse-engineer what's in the graph, even if the graph itself isn't directly accessible.

---

## Structural Mitigations

These are architectural decisions, not policies. They are designed into the system so they can't be toggled off.

### 1. No Query Logging

**The system does not store queries.** Queries are processed, answered, and discarded. No IP addresses, no device identifiers, no session tracking, no query history. There is nothing to subpoena.

This is not a policy. It's architecture. The query path does not write to any persistent store. Server logs at the infrastructure level retain no query content and auto-expire.

**Trade-off:** No usage analytics, no query-based improvement, no personalization. This is accepted.

### 2. No User Profiles

**The public interface has no accounts, no login, no profiles.** Every user is anonymous. The system cannot be compelled to produce "all queries made by person X" because it has no concept of person X.

**Trade-off:** No personalization, no saved preferences, no "what's new since last time." These features (if ever built) live in a separate, opt-in data plane that is architecturally isolated from the civic graph. See "Data Planes" below.

### 3. No Reporter Identity Storage

**Human-reported signal enters the graph as a node. The identity of the reporter is not stored.** The submission channel (SMS, web form, email) is used to receive the signal and then discarded. The graph node has no edge back to a person.

If a reporter explicitly chooses to be credited, that's their choice — stored as a public attribution, not as hidden metadata.

**Trade-off:** No ability to contact reporters for follow-up. No ability to build trust scores for individual reporters. This is accepted — trust is built at the source level (channels, organizations), not the individual level.

### 4. Geographic Fuzziness for Sensitive Signal

**The public-facing graph never surfaces exact locations for sensitive signals.** The system knows precise data internally (for deduplication and graph operations), but the public view is deliberately blurred. The level of blur scales with the sensitivity classification.

- Sensitive tensions (enforcement activity, location of vulnerable populations): city or region level only
- Responses to sensitive tensions (sanctuary churches, legal aid): neighborhood level, not street address
- General civic signal (volunteer events, food shelves): full precision, since these are designed to be found

**Implementation:** Sensitivity classification is a property of the graph, not a filter applied at query time. The public API physically cannot return precise coordinates for sensitive nodes — the precision is reduced before it reaches the API layer.

### 5. No Network Graph Exposure

**The API does not expose raw graph traversals that would reveal organizational networks.** A user can see: "These organizations are responding to this tension." A user cannot run: "Show me all organizations connected to Organization X across all tensions and all time."

The system uses the graph internally for context and synthesis, but the API surfaces curated views — not raw graph queries. There is no public Cypher endpoint.

**Trade-off:** Power users can't do deep graph exploration. This is accepted. The graph's full topology is an internal capability, not a public feature.

### 6. No Temporal Analysis on Actor Nodes

**The public API does not support historical queries on actor activity.** "What is Organization X doing right now?" is a valid query. "What has Organization X been involved in over the past two years?" is not publicly available.

The graph retains historical data for freshness and deduplication, but the API surfaces current state, not history. An adversary cannot use the system to build a dossier on an organization's activity over time.

**Trade-off:** No "organization timeline" feature. No "how has this issue evolved" for specific actors. Temporal analysis is available for tensions and places (which are civic context), not for actors (which enables surveillance).

### 7. Aggregate-Only Heat Maps for Sensitive Domains

**Heat maps for sensitive signal categories show aggregate patterns, not individual nodes.** "There is elevated immigration-related civic activity in South Minneapolis" — yes. Clicking through to see each individual sanctuary church — no.

For non-sensitive signal (volunteer events, cleanups, public meetings), individual nodes are visible on the map as expected.

**Implementation:** Sensitivity classification controls whether heat map tiles link to underlying nodes or only show density.

### 8. Subpoena Resistance by Architecture

**The system is designed so that there is nothing useful to subpoena.**

- No query logs → can't reveal who asked what
- No user profiles → can't reveal who used the system
- No reporter identity → can't reveal who submitted signal
- Geographic fuzziness → precise locations for sensitive signal don't exist in the API layer
- No actor timelines → can't produce a dossier

The graph itself contains public information organized — the same content that's on org websites, GoFundMe pages, and government databases. If subpoenaed, it reveals nothing that wasn't already public. The value of the system to an adversary is in the *organization* and *connection* of that data — which is why the mitigations above limit what organizational structure is queryable.

### 9. Canary and Transparency

**If Root Signal ever receives a government request for data, a legal demand for access, or a national security letter, the system publishes a transparency report.** If a canary warrant is ever triggered (i.e., the system is compelled to provide data and gagged from disclosing it), the absence of the canary statement is the disclosure.

This is a policy, not architecture — but it's listed here because it's non-negotiable.

---

## Data Planes

The adversarial model requires a strict separation between two data planes:

### Public Civic Graph (Anonymous)
- The core product. No auth. No profiles. No logging.
- Contains: civic signal organized as a knowledge graph
- Queryable by: anyone
- Protected by: all structural mitigations above

### Opt-In Personalized Layer (Authenticated)
- Future products like Signal Match, saved preferences, digest subscriptions
- Requires: explicit opt-in, account creation, informed consent
- Contains: user preferences, saved searches, notification settings
- **Architecturally isolated** from the civic graph — the personalized layer reads from the graph but the graph knows nothing about the personalized layer
- Protected by: standard data protection (encryption at rest, minimal retention, user-controlled deletion)
- **Never shared** with the public graph, the API, analytics, or any third party

The boundary between these planes is structural, not policy. The civic graph cannot query the personalized layer. They are separate systems that share a read-only interface.

---

## What Root Signal Will Not Build

Some features are too dangerous regardless of how useful they'd be:

**No organizer profiles or dashboards in the public graph.** The system does not build public profiles of individuals who organize, report, or respond. Actor nodes exist for organizations and institutions — not for private individuals — unless the individual has explicitly made themselves a public figure in the civic context (e.g., an elected official).

**No social graph of volunteers or reporters.** The system does not track who shows up to what, who donates to what, or who reports what. There are no edges between individual people in the graph.

**No real-time location tracking of any kind.** The system maps where civic activity happens. It does not track where people are or where they go.

**No predictive modeling of organizing activity.** The system does not predict where protests will happen, which organizations will act, or which communities will mobilize. It reports current state. It does not forecast.

**No data export of actor networks.** The API does not support bulk export of actor-to-actor, actor-to-tension, or actor-to-response relationships. The graph's relational structure is an internal capability.

**No integration with law enforcement systems.** Root Signal does not feed data to, accept data from, or interoperate with any law enforcement, immigration enforcement, or intelligence system. This is absolute and permanent.

---

## Unresolved Tensions

These are honest acknowledgments of problems this document does not fully solve:

**The aggregation problem.** Even with all mitigations, Root Signal aggregates public data in ways that make patterns visible that weren't visible before. A sanctuary church's public Instagram post is harmless in isolation. A hundred sanctuary churches mapped on a heat map is a network. The mitigations limit how this aggregation is queryable, but the underlying reality — that the system knows things about civic life that weren't knowable at scale before — is inherent to what it is.

**The open-source tension.** Root Signal is open source. Anyone can fork it and remove the mitigations. A government could deploy a modified version with full query logging, actor timelines, and network graph export. The mitigations protect the canonical deployment. They don't prevent misuse of the codebase.

**The "legitimate use" blur.** A journalist investigating which organizations are responding to a crisis is doing legitimate work. A government agent doing the same thing for surveillance purposes is asking the same queries. The system cannot distinguish intent. The mitigations limit the *tools* available, not the *users*.

**Scale changes the threat model.** A small system serving one metro with modest traffic is not an interesting surveillance target. A national system mapping civic activity across every city becomes one. Mitigations that are sufficient at small scale may be insufficient at national scale. This document should be revisited at each phase of growth.

**Over-suppression is itself a threat.** The impulse to protect vulnerable people by suppressing public civic signal is well-intentioned but misguided. The people posting about enforcement activity, organizing sanctuary responses, and fundraising for affected families on public platforms are doing so deliberately — they want visibility. They are acting in the open because that's how community response works. The people who need protection (undocumented individuals, at-risk families) are not the ones broadcasting on Reddit or Bluesky. They communicate through encrypted channels, through proxies, through trusted in-person networks. If Root Signal suppresses or holds back public civic signal out of fear of bad actors, it doesn't protect vulnerable people — it silences the community members trying to help them. The structural mitigations above (geographic fuzziness, no query logging, no organizer profiles) are the right protections. Muting the signal is not.

---

## Review Cadence

This document is reviewed:
- Before any new product surface is launched (new API endpoints, new visualization types, new data surfaces)
- Before expanding to a new geography
- Before any partnership with a government entity or institution
- Whenever a new threat scenario is identified
- At minimum, annually

The question at every review: **Could this system be used to hurt the people it's meant to serve? If so, what structural change eliminates that possibility?**
