# Root Signal — Comprehensive Use Cases

## What This Document Is

These are not features to be built. They are behaviors predicted to emerge from a civic knowledge graph that faithfully ingests signal, detects tension, maps responses, and serves the result to anyone who asks.

Root Signal is a substrate. Build it correctly — ingest signal broadly using LLM-driven source discovery, represent civic reality as a living graph, detect when nodes cluster into tensions, track responses as they form — and the use cases in this document answer themselves. No one designs a "small business owner mode" or a "new parent pathway." The graph accumulates signal about businesses under stress when enforcement actions suppress foot traffic on Lake Street. It accumulates signal about parent groups and baby equipment swaps when someone posts about them. The architecture is the feature set. The emergence is the product.

Every use case below is a prediction: if the graph is faithful to civic reality, this query will find its answer — not because someone built that query path, but because the signal exists and the system makes it findable.

Organized into nine categories: Everyday Community Life, Life Transitions & Milestones, Civic Moments & Rapid Response, Ecological Stewardship, Ethical Consumption & Conscious Living, Professional & Organizational, Power User & Repeat Engagement, Alignment State, and Platform Builders & API Consumers.

---

## 1. Everyday Community Life

These are the most common emergent behaviors — someone who lives in the Twin Cities and wants to participate in the life of their community.

### Volunteering & Helping

**"I have a free Saturday and want to do something useful."** A person with unstructured free time and a general desire to help. They don't know what's needed. Root Signal surfaces volunteer opportunities happening this weekend — sorted by proximity, time commitment, and urgency. A park cleanup in their neighborhood. A food shelf that needs drivers. A literacy tutoring session at the library. They pick one and show up.

> *Today this means checking VolunteerMatch, HandsOn Twin Cities, individual org websites, Facebook groups, and Nextdoor — each with different interfaces, different freshness guarantees, and no shared awareness of each other. The graph collapses them because it already ingested all of them.*

**"My neighbor just had surgery — is there a meal train I can join?"** Someone who knows a specific person needs help and is looking for the organized response. Root Signal surfaces active meal trains, care calendars, and mutual aid requests in their immediate area. If one exists for their neighbor, they find it. If not, they find orgs that coordinate this kind of support.

**"I have a truck and want to help someone move."** Skill-specific volunteering. This person has a specific asset (vehicle, tool, expertise) and wants to match it to a need. Root Signal enables skill-to-need matching — someone posted they need moving help on a mutual aid page, and now it's discoverable.

**"I want to tutor kids but don't know where they need help."** A person with a skill (teaching, reading) looking for the right venue. Root Signal surfaces literacy programs, after-school tutoring, ESL conversation groups, and homework help initiatives — not as a directory, but as live opportunities with open slots.

**"I have winter coats to donate — where should I take them?"** Seasonal, time-sensitive. The answer changes month to month. Root Signal knows which shelters and orgs are actively collecting coats right now, not just which ones exist.

> *Google returns the same 5 Salvation Army and Goodwill results year-round. The graph knows that Bridging just posted an urgent call for winter coats on Instagram yesterday, that Simpson Housing has a coat drive this Saturday, and that the community fridge network on 38th is accepting warm clothing this week. The difference is freshness and specificity.*

**"We made way too much food — can anyone use it?"** Hyper-local, perishable signal. Community fridges, food rescue programs, shelters accepting prepared meals. This needs to be fast and nearby.

### Events & Gathering

**"What's happening in my neighborhood this weekend?"** The broadest community query. Root Signal aggregates across Facebook groups, Eventbrite, org websites, neighborhood association newsletters, and meetup pages to give a consolidated picture — block parties, open mic nights, community dinners, park events.

> *This information currently lives in at least 6-8 different platforms per neighborhood, none of which know about each other. A neighborhood association posts on their website. The community center posts on Facebook. The park board posts on Eventbrite. The church posts on Instagram. The mutual aid group posts on a Google Doc. The graph makes one query out of what currently takes an hour of manual checking.*

**"I just moved here — how do I meet people?"** The new resident use case. Root Signal surfaces low-barrier, welcoming community gatherings: potlucks, run clubs, community gardens, open volunteer days, neighborhood happy hours. Filtered for "newcomer friendly" signals.

**"My kid needs community service hours for school."** Practical and constrained. Needs to be age-appropriate, within a reasonable distance, and verifiable. Root Signal surfaces opportunities tagged for youth and tracks which orgs provide service hour documentation.

**"Is there a free community yoga or tai chi class near me?"** Wellness through community. Root Signal includes community-organized, free or sliding-scale wellness activities in shared spaces — not commercial studio schedules, but the tai chi group in Como Park, the meditation circle at the community center, the running club that meets at the lake.

**"Where's the closest community garden with open plots?"** Seasonal, waitlist-dependent. Root Signal tracks which gardens are accepting new members and when registration opens — information currently scattered across a dozen org websites.

**"Is there a tool library or fix-it clinic nearby?"** The sharing economy in practice. Tool libraries, repair cafes, fix-it clinics, seed libraries — community resources that are invisible if you don't already know about them.

**"I want to practice my Spanish — any conversation groups?"** Skill-building through community. Language exchange meetups, ESL conversation circles, cultural events conducted in specific languages.

### Mutual Aid & Direct Support

**"A family on my block lost everything in a fire — who's organizing help?"** Crisis response at the hyper-local level. Root Signal surfaces active GoFundMe campaigns, mutual aid calls, donation drives, and organized support efforts — aggregated from the scattered platforms where they were posted.

> *Within 24 hours of a house fire, a GoFundMe goes up, a mutual aid group posts a call on Instagram, the neighborhood association sends an email, and someone starts a meal train on MealTrain.com. Today, you'd have to know all four channels exist. The graph concentrates them the moment they appear.*

**"I'm having a tough month financially — what resources exist for me?"** Someone in need, not just someone wanting to help. Root Signal surfaces food shelves, rent assistance programs, utility bill help, free meal programs, clothing closets — current, active, accepting new clients.

**"Is there a community fridge near me?"** Direct, immediate need. Root Signal knows the locations, hours, and current status of community fridges, free food pantries, and blessing boxes.

**"Who's running mutual aid in my area?"** Someone who wants to connect with an existing network rather than start from scratch. Root Signal surfaces active mutual aid groups by geography.

---

## 2. Life Transitions & Milestones

These use cases emerge when someone's relationship to their community shifts. The graph naturally accumulates signal around the resources that cluster at life inflection points.

### New to the Area

**"I just moved to Saint Paul from out of state. Where do I start?"** The cold-start problem. This person has zero local context. Root Signal gives them an on-ramp: welcoming community events, active neighborhood associations, popular volunteer opportunities, nearby community spaces. Essentially: here's the social infrastructure of your new home.

**"Which neighborhood associations are active near me?"** Someone trying to find the governance layer of their community. Root Signal surfaces which associations meet regularly, have active committees, and are welcoming new members — not just which ones technically exist.

**"What's the local food scene beyond restaurants?"** Farmers markets, CSAs, food co-ops, community kitchens, cultural food events, community dining nights. The food landscape that isn't on Yelp.

### Family Changes

**"We just had a baby — what support is available?"** New parent looking for community resources: parent groups, lactation support, baby equipment swaps, postpartum mutual aid, infant-friendly community spaces.

**"My kids are starting school — what's the parent community like?"** Plugging into the school ecosystem: PTA/PTO groups, school garden programs, after-school activities, family volunteer opportunities.

**"My mom is aging and needs more help — what's out there in Roseville?"** Aging-in-place resources: senior services, home modification programs, meal delivery, companion visiting programs, caregiver support groups. Geo-filtered to a specific suburb.

**"We're going through a divorce — are there support groups nearby?"** Life transition requiring new social infrastructure. Root Signal surfaces peer support groups, family transition resources, and community-based counseling — not commercial therapy directories.

### Career & Financial

**"I just got laid off — what resources are available in Hennepin County?"** Acute need triggered by job loss. Workforce development programs, resume workshops, emergency financial assistance, food assistance, career transition support groups.

> *Hennepin County's own website lists some of these. So does 211. So does the workforce development board. So do a dozen nonprofits on their own sites. None of them reference each other. A person in crisis doesn't have the bandwidth to check 15 sources — they need one query that returns the full picture.*

**"I'm retiring and want to do something meaningful with my time."** Time-rich, looking for purpose. Root Signal surfaces volunteer leadership roles, board positions, mentoring programs, and long-term stewardship commitments — not just one-off events.

**"I'm starting a small business — are there any community-supported options?"** Co-ops, incubators, shared commercial kitchens, community lending circles, small business peer groups.

---

## 3. Civic Moments & Rapid Response

When something happens in the civic or political landscape, the graph's tension detection naturally shifts what surfaces. The system's value in these moments is concentration and speed — converting awareness into action.

### Rapid Response to Civic Events

**"ICE is conducting raids in Minnesota — where are the solidarity rallies and know-your-rights events?"** A federal enforcement action triggers community mobilization. Root Signal rapidly surfaces the organized community response: solidarity vigils, know-your-rights workshops, legal aid hotlines, rapid response networks, immigrant rights organizations hosting community meetings. Root Signal doesn't track enforcement activity — it amplifies the community's organized response to it.

> *Within hours of an enforcement action, signal explodes across platforms: ACLU Minnesota posts on Twitter, ISAIAH organizes a vigil on Facebook, Centro de Trabajadores puts know-your-rights materials on their website, a legal aid hotline number circulates on Instagram. Today, you'd only see the signals from platforms you already follow. The graph captures all of them because LLM-driven source discovery already found these organizations and their channels.*

**"The governor just signed a bill that affects [my community] — what's being organized?"** Legislative action triggers grassroots response. Root Signal surfaces community meetings, town halls, advocacy events, and organizations mobilizing around the issue. Within hours of a major legislative moment, the signal pipeline surfaces the response.

**"There's a police shooting in my neighborhood — where are the vigils and community spaces?"** Trauma response. Root Signal surfaces community-organized vigils, healing spaces, mental health resources, community meetings, and mutual aid activations. The signal is about what the community is building in response, not the incident itself.

**"Roe was overturned / [major Supreme Court decision] — what can I do locally?"** National moments that generate local action. Root Signal surfaces local rallies, community organizing meetings, mutual aid activations, and civic engagement events — turning national outrage into local participation.

**"Book bans are happening at my kid's school — who's organizing?"** Education policy trigger. Root Signal surfaces parent organizing groups, school board meeting schedules, community advocacy organizations, and upcoming public hearings.

**"They're trying to close our neighborhood school — how do I fight this?"** Hyper-local civic crisis. Root Signal surfaces the school board hearing schedule, existing organizing efforts, parent groups, community petitions, and advocacy resources.

### Ongoing Civic Engagement

**"When is the next city council meeting and what's on the agenda?"** The baseline civic engagement query. Root Signal aggregates public meeting schedules across city, county, school board, watershed district, and special purpose government bodies — information currently scattered across dozens of government websites.

> *The Twin Cities metro has hundreds of public bodies that hold open meetings: city councils, county boards, school boards, watershed districts, park boards, planning commissions, housing authorities, transit authorities. Each posts its own calendar on its own website in its own format. There is no single place to see civic process across jurisdictions. The graph is that place.*

**"There's a zoning change proposed for my block — how do I participate?"** Land use decisions that directly affect residents. Root Signal surfaces the public comment period, hearing dates, relevant neighborhood association meetings, and existing community positions on the proposal.

**"Public comment on the new transit plan closes Friday — what do I need to know?"** Time-sensitive civic participation. Root Signal surfaces the comment deadline, links to the plan, community meetings where it's being discussed, and organizations that have published positions.

**"Who's running for my school board and where can I meet them?"** Election engagement. Root Signal surfaces candidate forums, neighborhood meet-and-greets, voter guides from community organizations, and registration deadlines.

**"I want to testify at the legislature about housing — how?"** Someone ready to take civic action but lacking the procedural knowledge. Root Signal surfaces hearing schedules, advocacy organizations offering testimony training, carpools to the capitol, and upcoming committee meetings on housing legislation.

**"Is anyone challenging the proposed highway expansion?"** Residents want to know if organized opposition exists before starting from scratch. Root Signal surfaces existing advocacy campaigns, community coalitions, public comment opportunities, and upcoming hearings.

### Crisis & Emergency Response

**"There's a tornado warning — where are the shelters?"** Immediate physical safety. Root Signal surfaces open emergency shelters, mutual aid activations, and community spaces offering refuge. During crises, the signal pipeline accelerates its ingestion cadence.

**"The apartment building on Lake Street caught fire — how can I help the families?"** Acute community crisis. Within hours, GoFundMe campaigns, mutual aid calls, donation drives, and volunteer mobilizations appear. Root Signal concentrates them so donors and volunteers find the right channels immediately.

**"We had a major ice storm — what's open and who needs help?"** Infrastructure disruption. Root Signal surfaces warming centers, mutual aid deliveries, roads to avoid, and organizations coordinating response — pulled from community social media, org websites, and government emergency channels.

**"Flooding along the river — where are they sandbagging?"** Physical labor mobilization. Root Signal surfaces active volunteer staging areas, supply donation points, and community organizing efforts for flood response.

**"Power has been out for two days in North Minneapolis — what resources exist?"** Extended infrastructure failure in a specific geography. Root Signal surfaces charging stations, warming/cooling centers, food distribution points, and mutual aid networks active in the affected area.

### Post-Crisis Follow-Through

**"The emergency is over but those families still need help — what's ongoing?"** Crises fade from the news but recovery takes months. Root Signal tracks the transition from emergency response to long-term recovery: ongoing fundraisers, rebuilding efforts, case management services, and long-term support groups.

**"What happened with that ballot measure from November?"** Civic follow-through. Root Signal tracks implementation of passed measures, upcoming related hearings, and organizations monitoring compliance.

---

## 4. Ecological Stewardship

People who care about the land, water, and ecological health of their place. Ecological signal is first-class in the graph — a wetland restoration has the same structural weight as a food shelf volunteer call.

### Habitat & Land Care

**"Where are buckthorn pulls happening this month?"** Invasive species removal is one of the most common and accessible ecological volunteer activities in Minnesota. Root Signal aggregates pulls organized by parks departments, watershed districts, conservation corps, and neighborhood groups.

> *A single metro-wide query for "buckthorn pulls this month" currently requires checking Minneapolis Parks, Saint Paul Parks, Three Rivers Park District, at least 8 watershed districts, the Conservation Corps, Friends of the Mississippi River, and a dozen neighborhood-level stewardship groups — each with their own website and calendar. The graph already ingested all of them.*

**"I want to plant native species in my yard — who can help?"** Root Signal surfaces native plant sales, seed swaps, community nurseries, Master Gardener consultations, and educational workshops. Seasonal and time-sensitive.

**"Which parks near me have active volunteer stewardship groups?"** Someone ready to commit to ongoing land care. Root Signal maps stewardship groups to specific parks and natural areas, with meeting schedules and current projects.

**"Is there a prairie restoration project I can join?"** Specific ecological interest. Root Signal surfaces active restoration projects across the metro — prairie burns, savanna restoration, wetland rehabilitation — with volunteer opportunities.

**"Where can I learn to identify native plants?"** Educational pathway. Root Signal surfaces Master Naturalist programs, nature center classes, guided plant walks, and community education courses.

### Water & Watershed

**"How can I help with water quality in my watershed?"** Root Signal surfaces watershed district volunteer programs, rain garden workshops, shoreline restoration projects, water monitoring opportunities, and storm drain stenciling events.

**"Where can I do citizen science water monitoring?"** Specific skill-based ecological engagement. Root Signal connects volunteers to established monitoring programs like Minnesota Pollution Control Agency's citizen monitoring network, local watershed districts, and university research partnerships.

**"Are there any lake cleanups happening soon?"** Seasonal, event-based. Root Signal aggregates lake and river cleanups across the metro.

### Wildlife & Biodiversity

**"I found an injured bird — who do I call?"** Immediate, actionable. Root Signal surfaces wildlife rehabilitation centers, rescue hotlines, and species-specific guidance.

**"Where can I do bird surveys or Christmas bird counts?"** Citizen science engagement. Root Signal surfaces organized survey events, Audubon chapter activities, and eBird hotspots with active community involvement.

**"I want to build bat houses / bee hotels — is anyone coordinating this?"** Community-scale conservation. Root Signal surfaces community conservation projects, habitat creation workshops, and cooperative purchasing groups.

### Urban Ecology

**"What community composting options exist near me?"** Waste diversion through community infrastructure. Root Signal surfaces community composting sites, curbside organics programs, and composting workshops.

**"I want to reduce my lawn — who's doing pollinator gardens in my area?"** Landscape transformation. Root Signal surfaces Lawns to Legumes programs, pollinator pathway projects, neighborhood garden tours, and educational resources.

**"Where are the best foraging spots and what workshops exist?"** Ethical foraging. Root Signal surfaces foraging walks, wild edibles classes, and community harvesting events — not specific spots (to prevent over-harvesting), but educational pathways.

---

## 5. Ethical Consumption & Conscious Living

People making intentional choices about where their money, time, and attention go.

### Local & Values-Aligned Commerce

**"Where are the worker-owned co-ops in the Twin Cities?"** Root Signal maps the cooperative economy: worker co-ops, consumer co-ops, housing co-ops, and cooperative businesses. Not a commercial directory — a map of alternative economic structures.

**"Which farmers markets are open this week and where?"** Seasonal, rotating. Root Signal aggregates market schedules, vendor lists, and special events across the metro's 50+ farmers markets.

> *Minnesota Grown maintains a directory. So does the Farmers Market Association. Individual markets post their own schedules on their own websites and Facebook pages. Some list vendors, some don't. Hours change seasonally. Markets open and close year to year. The graph tracks all of this as live signal, not as a static directory entry.*

**"I want to join a CSA — what are my options?"** Time-sensitive (signup windows). Root Signal surfaces CSA farms serving the Twin Cities, with share sizes, pickup locations, and registration status.

**"Where can I buy directly from local farms?"** Farm stands, farm stores, buying clubs, and direct-to-consumer operations. The supply chain Root Signal helps people access.

**"Is there a Black-owned bookstore near me?"** Values-aligned commerce. Root Signal surfaces community-curated directories of businesses owned by specific communities — not as a marketing tool, but as a community asset.

### Reduce & Repair

**"Where's the nearest repair cafe or fix-it clinic?"** The counter-consumerist use case. Root Signal tracks fix-it clinics (many are monthly, rotating locations), repair cafes, and community makerspaces.

> *Hennepin County runs fix-it clinics. So does Ramsey County. So do several independent groups. They rotate locations monthly. The schedule is on each county's website separately, in different formats, updated at different cadences. A single query in the graph returns all of them, sorted by date and distance.*

**"I want to organize a clothing swap — has anyone done this nearby?"** Community organizing around waste reduction. Root Signal surfaces existing swap events and organizations that support them.

**"Where can I donate working electronics instead of throwing them away?"** Responsible disposal. Root Signal surfaces reuse organizations, electronics donation programs, and community tech refurbishment projects.

**"Are there any Buy Nothing groups in my area?"** Gift economy. Root Signal surfaces Buy Nothing groups, free stores, everything-is-free days, and community sharing networks.

### Food Systems

**"Where can I get affordable, healthy food in my neighborhood?"** Food access, especially in food-scarce areas. Root Signal surfaces food co-ops, farmers markets accepting SNAP/EBT, community kitchens, food shelves with fresh produce, and community meal programs.

**"I want to learn to preserve food — any community classes?"** Skill-building. Root Signal surfaces canning workshops, fermentation classes, community kitchen programs, and extension service offerings.

**"What's in season right now at Minnesota farms?"** Seasonal awareness. Root Signal tracks what's available at local markets and farms, connecting consumption to agricultural rhythms.

### Boycotts & Economic Action

**"I want to support the boycott of [company] — what are the local alternatives?"** Economic activism. Root Signal surfaces community-identified alternatives to boycotted companies, local substitutes, and cooperative options. The signal is affirmative: here's where to redirect your spending.

**"How do I move my money to a community bank or credit union?"** Financial system alternatives. Root Signal surfaces community development financial institutions, credit unions, and community banks in the area.

---

## 6. Professional & Organizational Use Cases

People using Root Signal in the context of their work or organizational role. These emerge because the graph naturally contains the landscape information that professionals currently assemble manually.

### Nonprofit & Community Org Staff

**"What other organizations in Phillips are working on food access?"** Landscape mapping. A program director wants to understand who else is operating in their space to avoid duplication and find collaboration partners. Root Signal provides a living map of active organizations, programs, and initiatives in a specific geography and domain.

> *Today, a nonprofit program director builds this picture through word of mouth, attending community meetings, and checking org websites one by one. The picture is always incomplete and always stale. The graph has this picture as a natural byproduct of ingesting all the signal these organizations produce.*

**"We're planning a fall fundraiser — what else is happening that weekend?"** Event deconfliction. Organizations want to avoid scheduling on top of each other. Root Signal's aggregated event data makes this possible without calling every other org.

**"We need 20 volunteers for a one-day event — where do we post?"** Supply-side use case. Organizations with volunteer needs use Root Signal to understand where volunteers are looking and how to reach them.

**"Which mutual aid networks are active in Brooklyn Park?"** Organizational awareness. A new organization wants to connect with existing networks rather than duplicate infrastructure.

### Educators & Schools

**"I need age-appropriate service learning opportunities for my 8th graders."** Teachers with specific constraints: age-appropriate, within bus range, verifiable hours, and connected to curriculum. Root Signal filters opportunities accordingly.

**"Can my class do a citizen science project this spring?"** Root Signal surfaces student-appropriate monitoring programs, school garden partnerships, and nature center education programs.

**"Where can my students see local democracy in action?"** Civic education. Root Signal surfaces city council meetings, legislative sessions, community forums, and mock government programs open to youth.

### Journalists & Researchers

**"What are the most underserved neighborhoods for mutual aid in the metro?"** Heat map query. A journalist analyzing service gaps. Root Signal's geographic signal density data reveals where community infrastructure is concentrated and where it's thin.

**"How has volunteer activity changed in the last six months?"** Trend analysis. Root Signal's time-series data on signal volume and type enables longitudinal analysis of community engagement.

**"Which environmental issues are communities organizing around most?"** Thematic analysis. Root Signal's signal categorization enables researchers to identify emerging community priorities.

### Local Government

**"What are residents in Ward 7 organizing around?"** A council member wanting to understand constituent priorities through the lens of community self-organization, not just 311 complaints.

**"Where are the gaps in community services on the North Side?"** Planning and resource allocation. Root Signal's geographic data helps government identify where community needs are highest and community infrastructure is thinnest.

**"What community resources exist for disaster preparedness in our jurisdiction?"** Emergency management. Root Signal maps the informal community infrastructure that supplements official emergency response.

### Funders & Grantmakers

**"What organizations are doing ecological stewardship work in Ramsey County?"** Landscape scan for grant targeting. Root Signal provides a current, comprehensive view of who's doing what — more current than any grant database.

**"Are there gaps in youth programming on the East Side?"** Needs assessment. Root Signal reveals both what exists and what's absent, helping funders direct resources where they're most needed.

---

## 7. Power User & Repeat Engagement

These use cases emerge from people who've used Root Signal before and are coming back regularly.

**"What's new since last time I checked?"** The return visit. Root Signal surfaces new signal since the user's last interaction — new events, new needs, new organizations, new opportunities.

**"Show me opportunities similar to the river cleanup I did last month."** Recommendation based on history. Root Signal surfaces ecologically similar opportunities: other cleanups, stewardship events, restoration projects.

**"I said I care about housing — what's happened this week?"** Topic tracking. Root Signal monitors a specific domain and surfaces relevant new signal: housing-related volunteer opportunities, advocacy events, policy developments, and mutual aid needs.

**"Track upcoming public hearings about transit for me."** Civic monitoring. Root Signal watches for transit-related public meetings, comment periods, and community forums.

**"What are the most urgent needs in the metro right now?"** Urgency-sorted overview. Root Signal ranks current needs by time-sensitivity, impact, and volunteer gap — helping people direct their energy where it matters most.

**"Which of my saved organizations have upcoming events?"** Organizational following. Root Signal notifies users when organizations they follow post new opportunities.

**"I finished Master Naturalist training — what's next for me?"** Progression. Root Signal surfaces advanced ecological stewardship opportunities appropriate for someone with specific credentials.

**"What's trending in community activity this month vs. last?"** Meta-analysis. Root Signal's time-series data reveals shifts in community priorities and energy — more food shelf needs this month, more civic engagement last month.

---

## 8. Alignment State

These queries are native to the alignment machine — the emergent property of a graph that faithfully tracks Need nodes, Response nodes, and Tension clusters over time. They don't ask "what can I do?" — they ask "what is the state of civic life?" Understanding the state is the first step to knowing where to show up.

### Community Stress

**"Where is my community under the most stress right now?"** The alignment machine's fundamental query. The graph surfaces geographies where Need nodes are clustering, where Tension nodes are active, and where the ratio of Needs to Responses is highest. This isn't a feature someone builds — it's what falls out of a graph that tracks both.

**"What problems are getting addressed vs. getting worse?"** Temporal alignment detection. When Response nodes cluster around a Need and that Need's signal frequency decreases over time, alignment is being restored. When Need nodes keep growing despite Responses, the gap is widening. The graph reflects this naturally.

**"What are immigrant-run businesses struggling during this time?"** A tension cluster around immigration enforcement connects to Place nodes (Lake Street corridor, Little Mekong), to Evidence nodes (news coverage, business closure announcements), and to Response nodes (solidarity campaigns, mutual aid activations). The query finds its answer not because someone designed an "immigrant business" query path, but because the graph faithfully represents the civic reality that includes struggling businesses.

> *No existing system can answer this query. Google returns news articles. Yelp has reviews. Neither connects enforcement actions → community stress → business impact → organized response. The graph does, because it represents the relationships between civic actors, tensions, and responses — not just individual listings.*

**"Is the housing crisis in North Minneapolis getting better or worse?"** The graph tracks housing-related Need nodes (eviction prevention requests, rent assistance calls, shelter capacity signals) and Response nodes (legal aid clinics, tenant organizing meetings, housing advocacy events) in a specific geography over time. The trajectory of the ratio tells the story.

**"What tensions have cooled recently — what's working?"** The alignment machine's most hopeful query. When a Tension cluster's associated Need nodes stop appearing and Response nodes plateau, something worked. The system doesn't know what — maybe people engaged through Root Signal, maybe through word of mouth, maybe through a thousand other channels. It doesn't need to know. When the condition changes, the graph changes. The quiet is the signal.

**"Where are responses forming but needs still growing?"** The gap query. Where are people organizing but the underlying stress is still intensifying? This identifies places where more energy, more resources, or a different approach might be needed. It emerges naturally from comparing the temporal trajectories of Need and Response node clusters in the same geography and domain.

**"Which neighborhoods are seeing the most new civic activity?"** Energy detection. Where are new organizations forming, new events appearing, new volunteer calls emerging? Not necessarily tension — sometimes a neighborhood just wakes up. The graph reflects it because the signal appears.

**"What does civic life look like in my zip code right now?"** The broadest alignment query. Not "what can I do?" but "what is happening?" The full picture: active needs, ongoing responses, upcoming events, current tensions, recent resolutions. A snapshot of civic reality in a place.

---

## 9. Platform Builders & API Consumers

These use cases are for developers, organizations, and community builders who want to build on Root Signal's infrastructure.

### App Developers

**"I want to build a neighborhood app — can I pull volunteer opportunities from Root Signal?"** The core API consumer use case. A developer building a hyperlocal community app uses Root Signal's API to populate their volunteer section without building their own scraping infrastructure.

**"I'm building a civic engagement tool — can I get public meeting data?"** Specialized API consumption. A civic tech developer pulls only the civic engagement signal domain to power their hearing tracker.

**"I want to build a crisis response dashboard for my community."** Emergency-focused API consumption. A developer builds a crisis lens on Root Signal data that activates during emergencies and surfaces only urgent, actionable signal.

### Organizational Integrations

**"Can our mutual aid network's requests automatically flow into Root Signal?"** Signal production. Organizations become both consumers and producers — posting their needs through an API that feeds directly into Root Signal's pipeline.

**"We use Circle for our community — can members discover opportunities through Root Signal?"** Platform integration. Online communities embed Root Signal's signal feed into their existing platforms, giving members a discovery layer they'd otherwise lack.

**"Can our church's volunteer portal pull from Root Signal?"** Institutional integration. Faith communities, schools, and other institutions use Root Signal to populate their own engagement platforms.

### Data & Research

**"I want to study patterns of community organizing in the Twin Cities."** Academic research. Root Signal's structured, time-series data on community signal enables quantitative research on community engagement patterns, geographic disparities, and seasonal trends.

**"Can we use Root Signal data for our community needs assessment?"** Planning and evaluation. Nonprofits, government agencies, and funders use Root Signal's data to supplement their own assessments.

**"I want to build a visualization of civic engagement across the metro."** Data journalism and public information. Root Signal's geographic and temporal data enables compelling visualizations of community life.

---

## The Rapid Response Pattern

Several use cases above involve time-sensitive civic or crisis events. This is worth calling out as a distinct pattern because it has specific infrastructure implications.

### How Rapid Response Works

When a major civic event or crisis occurs, the signal landscape shifts. Normal community signal continues, but a new wave of response signal emerges: solidarity events, mutual aid activations, know-your-rights workshops, community meetings, volunteer mobilizations, donation drives.

Root Signal's value in these moments is concentration and speed. Within hours of a triggering event, the platform surfaces the organized community response — not the threat, not the crisis itself, but what people are building in response to it.

### The Pattern

The triggering event is external (a policy decision, an enforcement action, a natural disaster, a community crisis). Root Signal does not track or surface the triggering event itself. Root Signal surfaces the community's organized response: events being created, organizations mobilizing, resources being deployed, mutual aid activating. The signal pipeline accelerates its ingestion cadence for affected geographies and domains. As the acute response transitions to long-term recovery, Root Signal continues tracking the ongoing efforts.

### Examples of the Pattern

**Federal immigration enforcement action →** Root Signal surfaces know-your-rights workshops, legal aid clinics, solidarity vigils, community safety planning meetings, immigrant support organization activations. Not ICE locations. Not fear. The response.

**Major legislation signed →** Root Signal surfaces community meetings to discuss implications, advocacy organizations mobilizing, civic forums, town halls, and opportunities to provide public comment on implementation.

**Natural disaster →** Root Signal surfaces open shelters, volunteer staging areas, donation collection points, mutual aid deliveries, and recovery organizations. During the acute phase, ingestion cadence accelerates. During recovery, long-term rebuilding efforts stay visible.

**Police violence →** Root Signal surfaces community vigils, healing spaces, community meetings, mental health resources, and civic advocacy events. The community's constructive response, not the incident.

**School closures / institutional changes →** Root Signal surfaces parent organizing meetings, advocacy campaigns, community forums, and alternative resource options.

### What This Requires Technically

Rapid response use cases imply specific capabilities in the signal pipeline: the ability to accelerate ingestion cadence for a geography or domain, the ability to tag signal as "rapid response" or "crisis-related," the ability to surface time-sensitive signal with appropriate urgency, and the ability to transition from acute response tracking to long-term recovery tracking. This is a specialized mode of the same underlying infrastructure — not a different product.

---

## How These Use Cases Map to Signal Domains

| Use Case Category | Human Needs | Ecological | Civic | Ethical Consumption |
|---|---|---|---|---|
| Everyday Community Life | ●●● | ● | ● | ● |
| Life Transitions | ●●● | | ● | ● |
| Civic Moments & Rapid Response | ●● | | ●●● | ● |
| Ecological Stewardship | | ●●● | ● | ● |
| Ethical Consumption | ● | ● | | ●●● |
| Professional & Organizational | ●● | ●● | ●● | ● |
| Power User | ●● | ●● | ●● | ●● |
| Alignment State | ●●● | ●● | ●●● | ●● |
| Platform Builders | ●● | ●● | ●● | ●● |

Most real queries are cross-domain. Someone asking "what can I do about the water quality in my lake" touches ecological stewardship (monitoring programs), civic engagement (watershed district meetings), and community needs (volunteer cleanup events). Root Signal's value is that it doesn't force people to think in domains — it returns the full picture. The domains aren't categories the system enforces. They're patterns that emerge from the signal.

---

## The Unifying Thread

Every use case in this document answers one of two questions:

**"What can I do?"** — Here is what your community is building, here is where you can participate, here is how you show up.

**"What is happening?"** — Here is the state of civic life where you are. Here is where the stress is. Here is where alignment is being restored. Here is where help is still needed.

The first question is about agency. The second is about awareness. Together they form a complete picture: understand your community's state, then act on it.

These aren't features. They're what a faithful civic knowledge graph naturally produces. The system ingests signal, detects tension, maps responses, and serves the result. The use cases emerge. The architecture is the product.

That's the signal.
