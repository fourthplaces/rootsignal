# Resource Matching: Why It Matters

## The Gap

Root Signal knows *what's wrong* (tensions) and *who's responding* (signals). But it can't answer the simplest question a person walking through the door asks:

**"I have a car. Where am I needed?"**

Today, that person would have to browse every tension, read every Ask signal, and mentally filter for the ones that need a driver. The graph has no concept of "driver" as a matchable capability. The information is buried in freeform text — `what_needed: "volunteers with reliable transportation"` on one Ask, `summary: "seeking delivery drivers"` on another, `title: "Court accompaniment program needs rides"` on a third.

These are all the same need. The graph doesn't know that.

## What Resource Matching Opens Up

### 1. Instant capability matching

A person tells us one thing about themselves — what they can offer — and we show them everywhere that capability is needed, across every tension, every organization, every neighborhood.

**Person: "I have a car"**
- Loaves & Fishes: "delivery drivers for hot meals to homebound seniors" (food insecurity)
- NAVIGATE MN: "drivers to take immigrants to ICE check-ins" (immigration enforcement)
- Habitat for Humanity: "volunteers to transport building materials" (housing crisis)
- Simpson Housing Services: "shuttle drivers for residents to job interviews" (homelessness)

Four organizations, four different tensions, one capability. Today these are invisible to each other. Resource matching makes the connection explicit.

**Person: "I speak Somali"**
- Somali American Parent Association: "interpreters at school enrollment events" (education equity)
- Hennepin County: "bilingual navigators for public benefits applications" (poverty)
- CommonBond Communities: "translators for tenant rights workshops" (housing)
- Fairview Health: "medical interpreters for community clinics" (healthcare access)

**Person: "I have a commercial kitchen"**
- Abuelo's Kitchen: "shared kitchen space for community meal prep" (food insecurity)
- Appetite for Change: "certified kitchen for job training program" (economic development)
- Midtown Global Market Cooperative: "commissary access for emerging food entrepreneurs" (economic equity)

**Person: "I know employment law"**
- Mid-Minnesota Legal Aid: "volunteer attorneys for wage theft cases" (labor exploitation)
- Centro de Trabajadores Unidos: "legal clinic volunteers for worker rights" (immigration/labor)
- Tubman Center: "pro bono counsel for domestic violence survivors seeking employment" (gender-based violence)

### 2. The other direction: finding help

Resource matching works both ways. A person in need searches by what they're looking for, not by which tension they're experiencing.

**Person: "I need food"**
- Second Harvest Heartland: free groceries, Tuesdays and Thursdays (Give → Offers food)
- Community Emergency Service: emergency food shelf, walk-in (Give → Offers food)
- Loaves & Fishes: hot meals, daily at multiple locations (Give → Offers food)
- The Sheridan Story: weekend food packs for kids through schools (Give → Offers food)

They don't need to know whether their food insecurity is connected to the housing crisis, immigration enforcement, or a personal job loss. They need food. Resource matching lets them find it directly.

**Person: "I need legal help"**
- Mid-Minnesota Legal Aid: free civil legal services (Give → Offers legal-expertise)
- Volunteer Lawyers Network: pro bono representation (Give → Offers legal-expertise)
- NAVIGATE MN: immigration legal defense (Give → Offers legal-expertise)
- Immigrant Law Center of MN: asylum and deportation defense (Give → Offers legal-expertise)

**Person: "I need childcare"**
- Think Small: emergency childcare referrals (Give → Offers childcare)
- Way to Grow: home-based family support (Give → Offers childcare)
- Northside Achievement Zone: wraparound family services including childcare (Give → Offers childcare)

### 3. Cross-tension resource intelligence

Resources reveal hidden patterns that tension-by-tension analysis misses. When you aggregate across the whole graph, you see systemic bottlenecks.

**"What resources are most needed across all tensions in Minneapolis?"**

| Resource | Requires edges | Top tensions |
|----------|---------------|--------------|
| vehicle | 23 | food insecurity, immigration enforcement, housing, elder care |
| bilingual-spanish | 18 | immigration enforcement, labor exploitation, education equity |
| legal-expertise | 15 | immigration enforcement, housing crisis, labor exploitation |
| physical-labor | 14 | housing crisis, environmental cleanup, disaster response |
| childcare | 11 | housing crisis, education equity, workforce participation |
| reliable-internet | 9 | education equity, poverty, digital divide |
| mental-health | 8 | homelessness, immigration enforcement, gender-based violence |

This table tells you something no single tension can: **vehicles and bilingual Spanish speakers are the most leveraged capabilities in Minneapolis right now.** A single person with a car and Spanish fluency can help across 6+ tensions. That's not visible in any individual story or tension view.

**"What resources does the housing crisis specifically need?"**

| Resource | Requires count | Example Asks |
|----------|---------------|-------------|
| physical-labor | 8 | Habitat builds, cleanup crews, move-in help |
| legal-expertise | 5 | Tenant defense, eviction prevention, lease review |
| vehicle | 4 | Material transport, client rides to appointments |
| financial-donation | 3 | Emergency rent assistance funds |
| skilled-trade | 3 | Electricians, plumbers for rehab projects |
| childcare | 2 | Childcare during tenant organizing meetings |

### 4. Resource gap analysis

When you can see both sides — what's offered (Gives) and what's needed (Asks) — gaps become visible.

**"Where are needs unmet?"** — Resources with high Requires count but low Offers count:

| Resource | Asks (need) | Gives (available) | Gap |
|----------|------------|-------------------|-----|
| bilingual-hmong | 7 | 1 | -6 |
| reliable-internet | 9 | 2 | -7 |
| mental-health | 8 | 3 | -5 |
| childcare | 11 | 4 | -7 |
| skilled-trade | 6 | 1 | -5 |

This tells the community — and funders, policy makers, organizers — where investment has the most leverage. Childcare and reliable internet are massive unmet needs. Bilingual Hmong speakers are critically scarce relative to demand.

Contrast with well-served resources:

| Resource | Asks (need) | Gives (available) | Gap |
|----------|------------|-------------------|-----|
| food | 12 | 15 | +3 |
| clothing | 4 | 7 | +3 |
| legal-expertise | 15 | 11 | -4 |

Food and clothing are relatively well-served. Legal expertise has a gap but it's not as severe as childcare or internet access.

### 5. Compound matching and partial fits

Real needs are rarely one-dimensional. An organization needs "Spanish-speaking volunteers with cars on Saturday mornings." That's three resources: `bilingual-spanish`, `vehicle`, and `physical-labor` (with a schedule constraint on the edge).

Resource matching handles this gracefully:

**Ask: "Spanish-speaking drivers for Saturday food delivery"**
- Requires: vehicle, bilingual-spanish
- Edge notes: "Saturday mornings, 8am-12pm"

**Match results for a person who has a car but doesn't speak Spanish:**
- Score: 0.5 (1 of 2 Requires met)
- Still surfaces — they're half the solution. The org might pair them with a Spanish speaker.

**Match results for a person who speaks Spanish and has a car:**
- Score: 1.0 (all Requires met)
- Top of the list.

**Match results for a person who speaks Spanish but has no car:**
- Score: 0.5 (1 of 2 Requires met)
- Still useful — maybe the org has a vehicle but needs a navigator.

No one gets zero results. Partial matches still move the needle.

### 6. Temporal resource dynamics

Because signals have timestamps and freshness scores, resource needs change over time. After a winter storm:

- `physical-labor` Requires edges spike (snow removal, pipe repair, shelter setup)
- `vehicle` Requires edges spike (supply transport, emergency rides)
- `shelter-space` Requires edges spike (warming centers, overflow housing)

Two weeks later, those subside and the baseline pattern reasserts. Resource matching captures this naturally — it's just the aggregation of live signal data. No special temporal logic needed.

### 7. Organization profiles enriched by resources

Today, an ActorNode (organization) connects to signals via `ActedIn` edges. With Resources, you can derive an organization's **capability profile**:

**Second Harvest Heartland:**
- Offers: food, storage-space, vehicle (they have trucks)
- Requires: physical-labor, vehicle (volunteer drivers), financial-donation

**NAVIGATE MN:**
- Offers: legal-expertise, bilingual-spanish
- Requires: vehicle (client transport), financial-donation, bilingual-hmong

This turns every organization from a name into a capability map. "What does this org need? What do they give?" — answered by graph traversal, not manual research.

### 8. The coordination multiplier

The deepest value isn't any single query. It's the **coordination** that emerges when needs and capabilities are visible to each other.

Today: A church has 15 volunteers with cars sitting idle on Saturday. Three miles away, a food bank is desperately short on delivery drivers. Neither knows about the other. The information exists in the graph as freeform text but there's no connection.

With Resources: The food bank's Ask has `Requires(vehicle)`. The church's members search for `Resource(vehicle)` in their area. The match is instant. No coordinator needed. No phone tree. No "I think I heard someone at a meeting mention they need drivers."

This is what turns Root Signal from a signal discovery system into a **community coordination engine**. The graph already has the information. Resource nodes make it actionable.

---

## Beyond Minneapolis: Ecological Disaster and Global NGO Coordination

Everything above describes a single city. But the Resource architecture doesn't assume locality. Tensions are already global (a tension discovered in Minneapolis gets gravity-scouted in Portland). Resources work the same way — and the coordination failures they solve are *dramatically* worse at disaster scale.

### The coordination problem in disaster response

When a disaster hits, dozens of NGOs converge on the same geography within hours. Each brings different capabilities. Each has different needs. And the coordination between them is shockingly manual — spreadsheets, WhatsApp groups, UN OCHA cluster meetings, and a lot of people on radios asking "does anyone have a water purification unit?"

Resource matching turns that chaos into a queryable graph.

---

### Scenario: Earthquake — Hatay Province, Turkey (2023 pattern)

**Tension:** `Earthquake devastation in Hatay Province` — severity: critical, cause_heat: 0.98

Within 48 hours, the scout discovers signals from NGOs deploying to the region:

**Gives (what NGOs are bringing):**

| Organization | Signal | Offers |
|---|---|---|
| Doctors Without Borders (MSF) | "Mobile surgical teams deploying to Hatay" | `surgical-team`, `medical-supplies`, `field-hospital` |
| World Central Kitchen | "Hot meals from mobile kitchen units in Antakya" | `food`, `kitchen-equipment`, `logistics-team` |
| Team Rubicon | "Veteran-led search and rescue teams active in collapsed structures" | `search-and-rescue`, `heavy-equipment-operator`, `physical-labor` |
| Turkish Red Crescent | "Emergency shelter kits distributed at 12 sites" | `shelter-materials`, `blankets`, `water-purification` |
| UNHCR | "Refugee camp infrastructure repurposed for earthquake survivors" | `shelter-space`, `registration-system`, `protection-officer` |
| MapAction | "GIS mapping of damage and access routes" | `gis-mapping`, `satellite-imagery`, `logistics-intelligence` |

**Asks (what NGOs need on the ground):**

| Organization | Signal | Requires | Prefers |
|---|---|---|---|
| MSF | "Need Arabic/Turkish interpreters for triage" | `bilingual-arabic`, `bilingual-turkish` | `medical-knowledge` |
| World Central Kitchen | "Need local drivers who know road conditions" | `vehicle`, `local-knowledge` | |
| Team Rubicon | "Need heavy equipment operators for rubble removal" | `heavy-equipment-operator` | `structural-engineering` |
| UNHCR | "Need protection officers for unaccompanied minors" | `protection-officer`, `bilingual-arabic` | `child-welfare-expertise` |
| Mercy Corps | "Need warehousing for incoming aid shipments" | `storage-space`, `forklift-operator` | `cold-chain-capability` |
| All Hands and Hearts | "Need skilled carpenters for temporary shelter construction" | `skilled-trade`, `physical-labor` | `local-building-codes` |

**Now the queries that become possible:**

**"I'm a structural engineer arriving in Hatay. Where am I needed?"**
- Team Rubicon: Prefers(structural-engineering) for search and rescue assessment — match score 0.2 (Prefers only)
- All Hands and Hearts: related to shelter construction — partial match via `skilled-trade`
- Turkish government assessment teams: structural safety inspections

But also — Resource aggregation reveals that `structural-engineering` has 6 Requires edges and 0 Gives. **Critical unmet need.** This person's skills are the scarcest resource in the zone. The graph tells them that immediately.

**"I speak Arabic and Turkish"**
- MSF: Requires(bilingual-arabic, bilingual-turkish) — score 1.0, full match
- UNHCR: Requires(bilingual-arabic) — score 1.0
- Multiple other NGOs: Prefers(bilingual-turkish) for local coordination

**Cross-organization coordination: "What does everyone need that nobody has?"**

| Resource | Requires | Offers | Gap |
|---|---|---|---|
| bilingual-arabic | 8 | 2 | -6 |
| heavy-equipment-operator | 5 | 1 | -4 |
| structural-engineering | 6 | 0 | -6 |
| cold-chain-capability | 3 | 0 | -3 |
| child-welfare-expertise | 4 | 1 | -3 |
| water-purification | 7 | 2 | -5 |

This table — generated automatically from the graph — is what the UN OCHA cluster coordination meeting tries to build manually over days of meetings. Resource matching produces it in real time.

---

### Scenario: Oil Spill — Coastal Louisiana (Deepwater Horizon pattern)

**Tension:** `Massive oil spill contaminating Gulf Coast wetlands and fisheries` — severity: critical

**The temporal arc of resource needs:**

**Week 1: Emergency containment**

| Organization | Signal | Requires |
|---|---|---|
| US Coast Guard | "Boom deployment teams needed along 100 miles of coastline" | `boat-operator`, `physical-labor`, `hazmat-certification` |
| BP response contractors | "Skimmer vessel operators for open-water recovery" | `boat-operator`, `marine-diesel-mechanic` |
| Louisiana DOTD | "Road closures — need traffic management at 23 beach access points" | `vehicle`, `traffic-management` |
| National Wildlife Federation | "Oiled bird rescue teams deploying to Grand Isle" | `wildlife-rehabilitation`, `boat-operator`, `veterinary` |

**Week 3: Sustained cleanup**

| Organization | Signal | Requires |
|---|---|---|
| Ocean Conservancy | "Beach cleanup volunteers — 50 miles of shoreline" | `physical-labor`, `hazmat-certification` |
| Restore the Mississippi River Delta | "Wetland assessment teams needed" | `marine-biology`, `gis-mapping`, `boat-operator` |
| Louisiana Bucket Brigade | "Air quality monitoring volunteers" | `environmental-monitoring`, `scientific-equipment` |
| Gulf Restoration Network | "Legal documentation of damage for litigation" | `legal-expertise`, `photography`, `gis-mapping` |

**Month 3: Long-term recovery**

| Organization | Signal | Requires |
|---|---|---|
| Catholic Charities | "Mental health support for displaced fishing families" | `mental-health`, `bilingual-vietnamese`, `bilingual-spanish` |
| Southern Mutual Help | "Economic recovery counselors for small fishing businesses" | `financial-counseling`, `small-business-expertise` |
| Coalition to Restore Coastal Louisiana | "Volunteer mangrove replanting crews" | `physical-labor`, `marine-biology` |
| Oxfam America | "Community organizers to support fishermen's advocacy" | `community-organizing`, `legal-expertise`, `bilingual-vietnamese` |

**What this reveals:**

The same disaster creates completely different resource profiles over time. In week 1, it's `boat-operator` and `hazmat-certification`. By month 3, it's `mental-health` and `bilingual-vietnamese` (Louisiana's Vietnamese fishing community was devastated by Deepwater Horizon).

Resource matching captures this automatically — as old Asks expire and new ones emerge, the resource profile of the disaster shifts. No one has to manually update a "needs assessment." The graph reflects reality.

**The hidden bottleneck:** `bilingual-vietnamese` appears in month 3 with 5 Requires edges and 0 Offers. A critical gap that no one anticipated in week 1. Traditional disaster coordination doesn't surface this until community leaders spend weeks advocating for it. Resource matching surfaces it the moment the first Ask is extracted.

---

### Scenario: Wildfire Complex — Maui (2023 Lahaina pattern)

**Tension:** `Wildfire destroys historic Lahaina town` — severity: critical

**Unique resource dynamics of an island disaster:**

| Organization | Signal | Requires | Edge notes |
|---|---|---|---|
| American Red Cross | "Shelter management at War Memorial Gymnasium" | `shelter-management`, `bilingual-ilocano`, `bilingual-tagalog` | "24/7 staffing needed" |
| Maui Food Bank | "Emergency food distribution — need refrigerated trucks" | `vehicle-refrigerated`, `physical-labor`, `forklift-operator` | "Cold chain critical — 90°F days" |
| Maui Humane Society | "Animal rescue and temporary sheltering" | `animal-care`, `vehicle`, `kennel-space` | |
| Hawaiian Electric (HECO) | "Utility line clearance and grid restoration" | `electrical-lineworker`, `heavy-equipment-operator` | "Hazmat zones" |
| Maui Mutual Aid | "Native Hawaiian cultural artifacts recovery" | `cultural-preservation`, `archaeology`, `physical-labor` | "Kuleana land documentation" |
| Council for Native Hawaiian Advancement | "Housing navigators for displaced families" | `bilingual-hawaiian`, `housing-navigation`, `legal-expertise` | "ʻŌlelo Hawaiʻi preferred" |
| Direct Relief | "Medical supply distribution to remaining clinics" | `medical-logistics`, `pharmacy`, `vehicle` | |

**Cross-org resource gap:**

| Resource | Requires | Offers | Gap | Why it matters |
|---|---|---|---|---|
| bilingual-ilocano | 4 | 0 | -4 | Large Filipino community in West Maui, many elderly speakers |
| cultural-preservation | 3 | 0 | -3 | Lahaina was the historic capital of the Hawaiian Kingdom |
| vehicle-refrigerated | 5 | 1 | -4 | Island logistics — can't just drive trucks in from another state |
| electrical-lineworker | 6 | 2 | -4 | Island has limited utility workforce |
| housing-navigation | 7 | 1 | -6 | Housing was already scarce before the fire |

The island constraint makes resource gaps existential. You can't backfill `electrical-lineworker` by driving them in from the next county. Resource matching makes the scarcity visible immediately — and shows which resources need to be flown in, which changes the logistics calculus.

**Compound needs on Maui:**

Ask: "Housing navigators for Native Hawaiian families"
- Requires: `housing-navigation`, `bilingual-hawaiian`
- Prefers: `legal-expertise`, `cultural-preservation`

A mainland housing counselor (score 0.5) is helpful but can't navigate kuleana land rights. A Native Hawaiian speaker with housing experience (score 1.0 + Prefers bonuses) is transformative. The scoring system surfaces the right people.

---

### Scenario: Flooding — Pakistan (2022 monsoon pattern)

**Tension:** `Catastrophic monsoon flooding displaces 33 million people across Sindh and Balochistan` — severity: critical

**Scale changes everything about resource coordination:**

| Organization | Signal | Offers |
|---|---|---|
| Pakistan Army Corps of Engineers | "Pontoon bridges deployed on 4 major routes" | `engineering-heavy`, `bridge-construction`, `logistics-fleet` |
| WHO | "Disease surveillance teams in 12 flood-affected districts" | `epidemiology`, `laboratory`, `medical-supplies` |
| UNICEF | "Water purification units serving 500,000 people" | `water-purification`, `water-trucking` |
| Islamic Relief | "2000 family shelter kits distributed in Sindh" | `shelter-materials`, `food`, `hygiene-kits` |
| WaterAid | "Emergency latrine construction in displacement camps" | `sanitation-engineering`, `physical-labor` |

| Organization | Signal | Requires |
|---|---|---|
| WHO | "Need epidemiologists — cholera outbreak confirmed" | `epidemiology`, `bilingual-sindhi`, `laboratory` |
| UNICEF | "Need water trucking capacity — 200 tankers/day needed, have 45" | `water-trucking`, `vehicle-heavy`, `fuel-supply` |
| IRC | "Need psychosocial support counselors in Urdu and Sindhi" | `mental-health`, `bilingual-urdu`, `bilingual-sindhi` |
| Save the Children | "Need 500 temporary learning spaces" | `education-materials`, `tent-structures`, `bilingual-sindhi` |
| Edhi Foundation | "Need body bags and mortuary capacity" | `mortuary-services`, `vehicle-refrigerated`, `religious-burial` |

**The resource profile of a 33-million-person disaster:**

| Resource | Requires | Offers | Gap |
|---|---|---|---|
| water-trucking | 15 | 3 | -12 |
| bilingual-sindhi | 22 | 4 | -18 |
| epidemiology | 8 | 2 | -6 |
| sanitation-engineering | 11 | 3 | -8 |
| tent-structures | 14 | 5 | -9 |
| mental-health | 9 | 1 | -8 |
| fuel-supply | 7 | 1 | -6 |

`bilingual-sindhi` is the single biggest gap. Every NGO needs local language capacity and almost none of them brought it. The resource graph makes this visible on day 2 instead of week 4.

`water-trucking` gap of -12 tells UNICEF exactly how much additional capacity to request. Not "we need more water trucks" but "we need 155 more tankers per day." The edge attributes carry the quantities.

---

### What disaster scenarios reveal about the architecture

1. **Resource nodes don't assume locality.** The same `water-purification` Resource connects to Asks in Turkey, Louisiana, Maui, and Pakistan. The graph structure is identical. Only the signals and geography change.

2. **Temporal resource profiles are automatic.** Week 1 of an earthquake looks nothing like month 3. The graph reflects this as Asks expire and new ones emerge. No manual "needs assessment update" required.

3. **Language resources are consistently the biggest hidden gap.** In every disaster scenario, bilingual capabilities (`bilingual-vietnamese`, `bilingual-ilocano`, `bilingual-sindhi`) emerge as critical unmet needs that traditional coordination misses. Resource matching surfaces them immediately because they're first-class nodes, not buried in freeform text.

4. **Compound needs matter more at scale.** "Epidemiologist who speaks Sindhi" is not two separate needs — it's one person. The fuzzy-AND scoring means a Sindhi speaker (score 0.5) and a non-Sindhi epidemiologist (score 0.5) both surface, but someone who is both (score 1.0) tops the list.

5. **The gap table is the killer feature for coordination.** Every disaster coordination center tries to build this table manually. It takes days. Resource matching produces it from graph aggregation in seconds. And it updates continuously as the situation evolves.

6. **Island/remote constraints make gaps existential.** On Maui, you can't backfill a gap by driving in from the next state. The resource gap table combined with geography tells coordinators exactly what must be flown in — changing logistics decisions from guesswork to data.

7. **NGO capability profiles enable pre-positioning.** If you know that MSF Offers `surgical-team` and `field-hospital`, and Team Rubicon Offers `search-and-rescue` and `heavy-equipment-operator`, you can model coverage before anyone deploys. "If MSF and Team Rubicon both deploy to Hatay, what gaps remain?" is a graph query, not a phone call.
