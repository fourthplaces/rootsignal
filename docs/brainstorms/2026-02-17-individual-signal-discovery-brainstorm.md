---
date: 2026-02-17
topic: individual-signal-discovery
---

# Individual Signal Discovery via Instagram

## What We're Building

A discovery loop that finds individuals broadcasting civic signal on Instagram — without anyone manually adding them. The system watches hashtags and content within geographic boundaries, evaluates whether posts contain civic signal, and tracks the people who post them as entities in the graph.

This is not about scraping org accounts. It's about finding the volunteer coordinator who posts daily about mutual aid, the person asking for food for a family, the individual running a GoFundMe in their own name because the org behind them can't be public. These people ARE the signal — sometimes there's an org behind them, sometimes there isn't, and the system never needs to know which.

## Why This Approach

**Approach chosen: Hashtag Discovery Loop**

We considered two approaches:

1. **Hashtag Discovery (chosen):** Search Instagram hashtags within geographic boundaries, find posts with civic signal, follow the people who post them. Maximally emergent, finds org-less signal, hands-off.

2. **Signal Backtracking (rejected for v1):** Mine existing content for mentions of individuals, follow those leads. Higher precision but only discovers people connected to known orgs — misses the most important cases (individuals broadcasting independently, individuals hiding the org behind them).

Backtracking fails the core use case: someone who represents an org that doesn't publicly announce. She's posting "food pantry open Saturday" but the org is invisible. Backtracking can't find her. Hashtag discovery finds her because she's broadcasting civic content in a watched geography.

## Key Decisions

- **Public is public.** If someone is broadcasting on Instagram with civic hashtags, they've opted into visibility. The system aggregates what's already public.
- **Entities, not people.** An individual is an entity in the graph, same as an org. The distinction is metadata (`entity_type: "individual"`), not architecture. Signals link back to the entity, and the entity has a contact surface (Instagram profile).
- **No org tracing.** The system never maps an individual to an org. If the graph later shows signals clustering around the same cause, that's emergent structure — not an explicit link. This is a structural privacy guarantee.
- **Signals, not posts.** Individual posts are processed through the LLM extractor like org content. The system creates Event/Give/Ask/Notice nodes. Users see actionable signals with a way to contact the source, not raw Instagram posts.
- **Contact is the point.** The whole purpose is making individuals reachable. A signal like "we need food for a family" is useless if you can't contact the person. The entity's Instagram profile is the contact surface.
- **Seed hashtags, then emergence.** City profiles include seed hashtags (like `geo_terms` today). The LLM extracts new hashtags from discovered content, expanding the search over time. Minimal curation, maximum emergence.

## The Discovery Loop

1. **Search:** Scrape posts from seed hashtags within the city's geographic focus
2. **Evaluate:** LLM determines which posts contain civic signal (Event, Give, Ask, Notice)
3. **Extract:** Create signal nodes from qualifying posts
4. **Discover:** Record the poster's username as a discovered entity
5. **Follow:** On subsequent runs, scrape discovered individuals' recent posts
6. **Expand:** Extract new hashtags from discovered content, feed back into step 1

## What This Requires

- **Apify hashtag scraper** — new actor alongside the existing profile scraper
- **Dynamic source storage** — discovered entities can't be hardcoded in `CityProfile`. Need persistent storage (graph nodes or config) for discovered Instagram accounts
- **Individual-voice extraction prompt** — the LLM needs to handle "I'm delivering groceries" (individual voice) alongside "we're hosting an event" (org voice)
- **Entity contact surface** — signals link to an entity, entity has an Instagram profile URL
- **Hashtag expansion** — LLM extracts hashtags from content to feed back into discovery

## Open Questions

- How aggressively should the system expand hashtags? Unconstrained expansion could drift far from civic signal.
- What's the quality bar for "this person is worth following"? One civic post? Three? Sustained posting?
- How does the system handle accounts that go private or get deleted?
- Should there be a cooldown on hashtag searches to respect Apify rate limits / costs?
- How does the individual-voice extraction prompt differ from the current org-focused prompt? (The current prompt says "preserve organization phone numbers" — needs to also handle individual contact info.)

## Next Steps

→ `/workflows:plan` for implementation details — specifically the Apify hashtag scraper integration, dynamic entity storage, and extraction prompt changes.
