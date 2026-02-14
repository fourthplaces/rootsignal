# Taproot — Signal Taxonomy

## Purpose

This document defines the full ontology of signal that Taproot recognizes, classifies, and serves. It is the conceptual backbone for every extraction and classification decision the system makes. When a scraper pulls raw content and an LLM extracts structured signal, this taxonomy is the reference frame.

---

## Signal Domains

All signal falls into one of four broad domains. These are not mutually exclusive — a single signal can span domains (a community garden event is both human services and ecological stewardship).

### 1. Human Services
Signal related to people helping people meet immediate and ongoing needs.

### 2. Ecological Stewardship
Signal related to caring for the land, water, air, and living systems we share.

### 3. Civic & Economic Action
Signal related to participating in the systems that shape how we live together — governance, economic behavior, corporate accountability, policy.

### 4. Knowledge & Awareness
Signal that doesn't ask for action directly but creates the understanding necessary for informed action — research, education, data, investigative work.

---

## Signal Types

Each signal record has a primary `signal_type` that describes what kind of opportunity or need it represents.

### Human Services Domain

**volunteer_opportunity** — An organization or individual is asking for people to give time. May be one-time or recurring. May require specific skills or be open to anyone. Examples: food shelf shift, hospital visit program, tutoring, event setup.

**donation_need** — A request for financial contributions. May be for an organization, a family, a cause, or a specific campaign. Examples: GoFundMe, org donation page, mutual aid fund.

**supply_drive** — A request for physical goods. Specific items needed, often with urgency. Examples: winter coat drive, school supply collection, disaster relief supply list, diaper drive.

**mutual_aid_request** — A direct, person-to-person or community-to-community request for help. Often informal, often urgent. Examples: "family needs help with rent," "looking for someone to drive my mom to chemo," neighborhood mutual aid post.

**fundraiser** — An organized campaign to raise money for a specific purpose. Distinct from general donation needs by having a defined goal and timeline. Examples: GoFundMe, benefit concert, charity run, crowdfunding campaign.

**event** — A gathering that people can attend to participate in community life. May be educational, social, cultural, or action-oriented. Examples: community meeting, neighborhood block party, cultural celebration, workshop.

**professional_skills_needed** — A request for specific expertise offered pro bono or at reduced cost. Examples: nonprofit needs a web developer, legal clinic needs attorneys, org needs accounting help.

**housing_assistance** — Signal related to housing needs — emergency shelter, transitional housing, rental assistance, housing navigation. Examples: shelter availability, rent assistance programs, housing rights clinics.

**food_access** — Signal specifically about food — food shelves, community meals, food rescue, SNAP assistance, community gardens producing food. Examples: food shelf hours and needs, community meal schedules, food rescue volunteer calls.

**health_services** — Community health signal — free clinics, health screenings, mental health resources, support groups, harm reduction. Examples: free dental clinic, narcan training, grief support group, community health fair.

**transportation_assistance** — Signal about getting people where they need to go. Rides, transit passes, vehicle repair assistance. Examples: volunteer driver programs, transit pass donations, bike repair co-ops.

**legal_aid** — Free or low-cost legal assistance, know-your-rights events, immigration legal clinics, tenant rights workshops. Examples: legal aid clinic hours, ICE raid know-your-rights training, expungement workshops.

**immigrant_services** — Signal specifically serving immigrant and refugee communities — language assistance, documentation help, cultural integration, community navigation. Examples: ESL classes, citizenship workshops, refugee resettlement volunteer needs.

### Ecological Stewardship Domain

**habitat_restoration** — Active work to restore degraded ecosystems. Examples: prairie restoration, wetland reconstruction, streambank stabilization, native planting.

**pollution_cleanup** — Removing pollution from the environment. Examples: beach cleanup, river trash collection, litter pickup, microplastic survey, hazardous waste removal.

**water_monitoring** — Observing and measuring water quality in rivers, lakes, streams, and coastlines. Examples: citizen water quality monitoring, volunteer stream gauging, lake clarity measurements.

**wildlife_rescue** — Helping injured, displaced, or endangered wildlife. Examples: wildlife rehab center volunteer shifts, bird rescue, marine mammal stranding response, bat colony monitoring.

**tree_planting** — Reforestation and urban canopy expansion. Examples: tree planting events, urban forestry volunteer days, orchard planting, nursery volunteering.

**invasive_species_removal** — Identifying and removing non-native species that threaten ecosystems. Examples: buckthorn removal events, garlic mustard pulls, invasive carp monitoring, volunteer weed warriors.

**citizen_science** — Contributing to scientific research through observation, data collection, or analysis. Examples: bird counts, butterfly surveys, iNaturalist observations, phenology monitoring, Zooniverse projects.

**land_stewardship** — Ongoing care for land — trail maintenance, erosion control, prescribed burns, land trust volunteering. Examples: state park trail crew, conservation easement monitoring, community garden maintenance.

**reef_restoration** — Coral reef monitoring and restoration. Examples: reef check dive surveys, coral nursery volunteering, reef cleanup dives.

**climate_action** — Direct action on climate — renewable energy projects, carbon offset programs, climate adaptation projects. Examples: community solar installation, climate resilience planning, carbon garden projects.

**soil_health** — Soil restoration, composting, regenerative agriculture. Examples: community composting programs, soil testing volunteer events, regenerative farm workdays.

**watershed_stewardship** — Caring for an entire watershed system — stormwater management, rain garden installation, watershed district volunteering. Examples: rain garden builds, stormwater monitoring, watershed cleanup days.

### Civic & Economic Action Domain

**economic_boycott** — Organized refusal to purchase from specific companies or industries, coordinated around shared values. Examples: boycott lists, alternative product guides, "buy from these instead" campaigns.

**ethical_consumption** — Information about the supply chain, labor, environmental, or social impact of products and companies. Examples: company accountability reports, product impact ratings, fair trade alternatives, corporate behavior tracking.

**advocacy_action** — Calls to contact representatives, sign petitions, attend hearings, show up at rallies. Examples: call-your-senator campaigns, petition drives, public comment periods, rallies and marches.

**policy_engagement** — Opportunities to participate in governance — public hearings, comment periods, town halls, ballot initiatives. Examples: city council public comment, zoning hearing, ballot measure education, participatory budgeting.

**voter_engagement** — Signal about voter registration, election information, candidate forums, ballot guides. Examples: voter registration drives, candidate forums, nonpartisan ballot guides.

**cooperative_formation** — People coming together to build cooperative economic structures. Examples: worker co-op formation, food co-op memberships, cooperative housing, mutual aid network formation.

### Knowledge & Awareness Domain

**educational_event** — Workshops, trainings, and learning opportunities that build capacity for action. Examples: know-your-rights training, CPR certification, master gardener class, financial literacy workshop.

**research_need** — Researchers or institutions looking for community participation in studies, surveys, or data collection. Examples: university research studies, community health surveys, participatory action research.

**awareness_campaign** — Campaigns designed to educate the public about an issue. Examples: plastic-free July, mental health awareness month, missing and murdered indigenous women awareness.

**investigative_signal** — Journalism, reports, or data that reveals something a community should know. Examples: pollution data releases, corporate accountability investigations, environmental impact assessments, public records revelations.

**community_data** — Open data that communities can use to understand their own conditions. Examples: air quality data, crime statistics, school performance data, public health dashboards, environmental justice screening results.

---

## Audience Roles

Each signal is tagged with one or more audience roles — the ways a person might act on it.

**volunteer** — Give time. Show up physically or virtually to do work.

**donor** — Give money. Contribute financially to a cause, campaign, or individual.

**attendee** — Show up. Be present at an event, gathering, or meeting.

**advocate** — Use voice and economic power. Contact representatives, sign petitions, boycott, buy differently.

**skilled_professional** — Give expertise. Offer specific professional skills pro bono or at reduced cost.

**citizen_scientist** — Give observation. Contribute to scientific understanding through data collection, monitoring, or analysis.

**land_steward** — Give care to place. Maintain, restore, or protect land, water, and ecosystems.

**conscious_consumer** — Give attention to impact. Change purchasing behavior, support ethical alternatives, align economic life with values.

**educator** — Give knowledge. Teach, mentor, tutor, or facilitate learning.

**organizer** — Give coordination. Bring people together, build networks, facilitate collective action.

---

## Categories

Categories describe the subject area of a signal. A signal can have multiple categories. Categories cut across domains.

### Human Needs
food_security, housing, health, mental_health, legal_aid, transportation, clothing, employment, childcare, elder_care, disability_services, immigrant_services, refugee_services, addiction_recovery, domestic_violence, homelessness, financial_assistance, utility_assistance

### Community & Culture
arts_culture, youth, seniors, education, literacy, sports_recreation, neighborhood_development, community_building, interfaith, cultural_preservation, lgbtq_services, veterans_services, returning_citizens

### Ecological
water_quality, ocean_conservation, reforestation, soil_health, biodiversity, habitat_restoration, invasive_species, pollution_monitoring, climate_action, wildlife_protection, watershed_health, urban_ecology, regenerative_agriculture, coral_reef, wetlands, prairie, air_quality, composting, renewable_energy

### Civic & Economic
economic_justice, corporate_accountability, ethical_consumption, labor_rights, voting_rights, housing_policy, environmental_policy, immigration_policy, criminal_justice_reform, cooperative_economics, community_land_trust, participatory_governance

### Crisis & Emergency
disaster_relief, emergency_shelter, emergency_food, emergency_medical, crisis_response, fire_relief, flood_relief, storm_relief, pandemic_response

---

## Urgency Levels

**immediate** — Needed right now. Hours matter. Crisis-level signal.

**this_week** — Time-sensitive. An event coming up, a drive with a deadline, a need that's acute but not emergency.

**this_month** — Active and relevant but not urgent. Ongoing volunteer opportunity, recurring event, campaign with a longer timeline.

**ongoing** — Persistent need with no specific deadline. Standing volunteer position, always-accepting food shelf, continuous monitoring program.

**flexible** — No time pressure. Systemic information, educational content, ethical consumption guidance that's relevant whenever someone encounters it.

---

## Signal Quality Dimensions

Every signal has implicit quality attributes that affect how it should be ranked and served.

**Actionability** — Can someone do something concrete with this signal right now? "We need volunteers Saturday 9am at the food shelf" is highly actionable. "Homelessness is a problem" is not.

**Specificity** — How specific is the ask? "We need 5 people who can lift 40 pounds" is more useful than "we need volunteers."

**Freshness** — How recently was this signal produced or confirmed? A post from yesterday is worth more than one from 6 months ago.

**Source credibility** — Does this come from a verified organization, a known community leader, or an anonymous post? Multiple sources confirming the same signal increases confidence.

**Completeness** — Does the signal contain everything someone needs to act? Location, time, what to bring, who to contact, where to show up?

**Geographic precision** — Is this signal tied to a specific address, a neighborhood, a city, or is it location-ambiguous?

---

## Cross-Domain Signal Relationships

Signal types often connect across domains. These relationships matter for discovery and for building richer experiences.

A **pollution_cleanup** (ecological) might be connected to a **health_services** signal (human) if the pollution affects community health.

An **economic_boycott** (civic) might be connected to **ethical_consumption** (civic) and **water_quality** (ecological) if the boycott targets a company polluting a local waterway.

A **food_access** signal (human) connects to **soil_health** and **regenerative_agriculture** (ecological) when a community garden feeds both a food shelf and the land it grows on.

A **disaster_relief** signal (crisis) connects to **habitat_restoration** (ecological) when the disaster damaged ecosystems alongside human infrastructure.

These relationships are not strictly modeled in the initial schema but should be considered as the system matures — signal that surfaces one need can and should lead people to discover connected needs across domains.
