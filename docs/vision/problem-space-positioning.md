# Taproot — Problem Space & Positioning

## The Core Problem

There is massive signal loss between people who want to show up for the world around them and the actual opportunities to do so. The signal exists — people are broadcasting needs, organizations are posting volunteer shifts, fundraisers are live, cleanups are being organized, boycotts are being coordinated — but it's scattered across dozens of platforms, feeds, and walled gardens that were never designed to be searched together. The result is that most people who would act, don't — not because they don't care, but because the friction of finding where to plug in is too high.

The internet has made information abundant but orientation scarce. There is no reliable place to go to answer the question: **where is life asking for help right now, and how do I show up?**

---

## Problems Taproot Addresses

### 1. Fragmentation of Community Signal

Community needs and opportunities are distributed across platforms that don't talk to each other. A food shelf posts on its website, announces capacity changes on Instagram, lists volunteer shifts on VolunteerMatch, and shares events on Facebook. A person who wants to help has to check all of these independently, and most won't. The signal is there, but it's fragmented to the point of being effectively invisible.

**What exists today:** GoFundMe, VolunteerMatch, Eventbrite, Meetup, Facebook Groups, Instagram, org websites, Nextdoor, 211, church bulletins, neighborhood newsletters, municipal calendars — all operating as isolated silos.

**What's missing:** A single place that aggregates, normalizes, and geo-localizes all of this signal so it's discoverable in one search.

### 2. No "How Do I Help?" Infrastructure

Search engines can find you a nonprofit. Social media can surface a trending fundraiser. But no tool exists that is purpose-built to answer: "I'm here, I have time/money/skills/energy, what does my community need from me right now?" This is a fundamentally different query than what Google or Instagram are designed for. It requires intent-filtered, geo-localized, role-aware signal — not keyword matching or algorithmic feeds.

**What exists today:** Generalist search engines, algorithmic social feeds, curated directories that go stale.

**What's missing:** A signal service designed specifically for the intent of contributing — filtering by location, by how someone wants to help, by urgency, by category of need.

### 3. Signal Decay and Staleness

Community information goes stale fast. A volunteer page says "accepting volunteers" but the org quietly stopped months ago. A GoFundMe reached its goal but still appears in search results. An event was cancelled but the listing persists. People encounter dead signal constantly, which erodes trust and motivation. If you show up to help based on outdated information twice, you stop showing up.

**What exists today:** Static directories, listings that are updated manually (or not at all), no systematic freshness verification.

**What's missing:** A system that continuously re-checks sources, uses social media signals to detect staleness, and flags or expires outdated information automatically.

### 4. Ecological Signal Is Even More Buried

Environmental stewardship opportunities — beach cleanups, habitat restoration, watershed monitoring, citizen science, invasive species removal, reforestation events — are even more fragmented than human community signal. They're spread across niche org websites, government agency pages, academic platforms, and small local groups. The people who care about the planet often don't know that a river cleanup is happening 10 miles from their house this Saturday.

**What exists today:** Org-specific event pages, iNaturalist, scattered government portals, niche email lists.

**What's missing:** Ecological signal integrated alongside community signal, discoverable by geography and by the kind of stewardship someone wants to practice.

### 5. Invisible Supply Chains and Ethical Blind Spots

People want to align their daily habits with their values but can't see the threads connecting their purchases to environmental or social harm. Where does this product come from? What is this company doing with my money? Are there alternatives? This information technically exists but is buried in reports, investigative journalism, and activist networks. Economic boycotts, ethical consumption movements, and corporate accountability campaigns all suffer from the same signal loss — the information is out there, but it's not findable when you're standing in your kitchen making a decision.

**What exists today:** Scattered boycott lists on social media, investigative journalism, apps like Good On You (fashion-only), B Corp directories.

**What's missing:** Signal about corporate behavior, boycotts, and ethical alternatives concentrated alongside community action signal — so "how do I show up?" includes "what do I stop buying?"

### 6. No Local Utility for Civic Participation

Cities have 311 systems for complaints. They have websites for permits and services. But there is no utility — reliable, always-on, community-owned infrastructure — for civic participation in the positive sense. Not "I need to report a pothole" but "I want to contribute to where I live." This gap is felt acutely during crises (where do I donate after the tornado?) but it exists every single day in quieter ways (where can I volunteer this weekend?).

**What exists today:** 311 systems (complaint-oriented), municipal websites (service-oriented), United Way 211 (often outdated).

**What's missing:** A local utility for contribution — something a community can depend on being there, updated, and trustworthy, that answers "how do I participate in making this place better?"

### 7. Power Asymmetry Between Platforms and People

The platforms where community signal currently lives — Facebook, Instagram, TikTok, GoFundMe — are owned by corporations whose incentives are misaligned with community wellbeing. They optimize for engagement, not for action. They fragment information across walled gardens. They extract value from community organizers who create content on their platforms. People and communities have no ownership over the signal they produce.

**What exists today:** Corporate-owned platforms that host community signal as a byproduct of their engagement business model.

**What's missing:** Open-source, community-owned infrastructure that concentrates signal and returns it to the people as a public utility — not a product.

---

## Where Taproot Fits

Taproot is not a social network. It's not a nonprofit directory. It's not a search engine in the traditional sense. It's a **signal utility** — infrastructure that sits beneath any number of applications and surfaces.

It occupies a space that doesn't currently exist: the layer between the fragmented platforms where signal originates and the people and communities who want to act on it.

```
 Fragmented Signal Sources                     People Who Want to Show Up
 ┌──────────────────────┐                      ┌────────────────────────┐
 │ GoFundMe             │                      │ Volunteers             │
 │ Instagram            │                      │ Donors                 │
 │ Facebook Groups      │                      │ Attendees              │
 │ Org websites         │                      │ Advocates              │
 │ Eventbrite           │                      │ Skilled professionals   │
 │ Meetup               │                      │ Citizen scientists     │
 │ News outlets         │                      │ Land stewards          │
 │ Government sites     │                      │ Conscious consumers    │
 │ iNaturalist          │                      │ Community members      │
 │ Newsletters          │                      │                        │
 │ ...dozens more       │                      │                        │
 └──────────┬───────────┘                      └────────────┬───────────┘
            │                                               │
            │          ┌─────────────────────┐              │
            │          │                     │              │
            └─────────►│      Taproot        │◄─────────────┘
                       │                     │
                       │  Discovers signal   │
                       │  Concentrates it    │
                       │  Geo-localizes it   │
                       │  Makes it findable  │
                       │  Keeps it fresh     │
                       │  Serves it to all   │
                       │                     │
                       └─────────────────────┘
```

### What Taproot Is Not

Taproot is not competing with these platforms. It doesn't replace GoFundMe or Eventbrite or VolunteerMatch. It reads from them. It concentrates what they fragment. It links back to them for action. It makes their content more discoverable, not less valuable.

### What Taproot Becomes

In the near term: a signal pipeline proving that concentrated local signal is valuable and actionable.

In the medium term: an open API that any community app, city dashboard, or organization can plug into to serve their people better.

In the long term: a community-owned, open-source utility — deployable by any city, any neighborhood, any community — that answers the most fundamental civic question: **how do we show up for each other and for the world we live in?**

---

## The Audience

Taproot serves people through roles, not demographics. The same person might occupy multiple roles at different times:

**Volunteer** — "I have time. Where is it needed?"

**Donor** — "I have money. Where does it do the most good right now?"

**Attendee** — "I want to show up. What's happening near me?"

**Advocate** — "I want to align my economic and civic behavior with my values. What should I know?"

**Skilled professional** — "I have expertise. Who needs it?"

**Citizen scientist** — "I want to contribute to understanding the world. What can I observe or monitor?"

**Land steward** — "I want to care for the land and water around me. Where do I start?"

**Conscious consumer** — "I want to stop doing harm I can't see. What are the threads I'm not aware of?"

**Educator** — "I have knowledge to share. Who needs it?"

**Organizer** — "I want to bring people together. Where's the energy?"

---

## The North Star

A world where the distance between caring and acting is zero. Where no one who wants to help has to spend an hour searching for how. Where every community — human and ecological — has signal that is heard, concentrated, and acted on. Where the infrastructure for showing up is as reliable and available as running water.

Taproot is the root system beneath all of that.
