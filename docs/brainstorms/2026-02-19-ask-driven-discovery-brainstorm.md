---
date: 2026-02-19
topic: ask-driven-discovery
---

# Ask-Driven Discovery: Volunteer & Donation Opportunities

## Observation

The system captures volunteer and donation opportunities when it stumbles on them during scraping, but **Asks are never used as seeds for further discovery.** Only Tensions drive the Response Scout and Gravity Scout. This means:

- An Ask like "South High School needs mentors for immigrant families" sits in the graph but never triggers a search for similar mentor programs
- Volunteer opportunities are underrepresented (7 out of 114 signals) despite infrastructure existing (VolunteerMatch, Eventbrite sources)
- The scraping depth on listing pages (VolunteerMatch, Eventbrite) is shallow — listings have thin content, no click-through to detail pages

## Key Principle

**Volunteer/donation queries should emerge from clear needs expressed in the system** — not from generic "volunteer opportunities Minneapolis" queries. If the graph has an Ask about food insecurity, THEN searching for "volunteer food shelf Minneapolis" makes sense. The system's own knowledge should drive what it looks for.

## Simplest Approach Considered

The bootstrap query generator already uses an LLM to create Tavily search queries per city. It could also:

1. Query the graph for existing Asks
2. Feed them to the LLM alongside city context
3. Generate discovery queries that would find similar opportunities

This keeps volunteer/donation discovery grounded in real expressed needs rather than generic categories. The existing scrape → extract → dedup pipeline handles the rest.

## What's Already Working

- Extraction layer correctly classifies donations → Ask, volunteer calls → Ask
- 15+ fundraiser signals already in graph (GoFundMe, church-linked, mutual aid)
- Prompt changes shipped (2026-02-19) to broaden donation language beyond GoFundMe in Response Scout, Gravity Scout, and Bootstrap
- VolunteerMatch and Eventbrite sources are bootstrapped per city

## Open Questions

- Should this be a lightweight addition to bootstrap, or a proper "Ask Scout" that investigates Asks the way Response Scout investigates Tensions?
- How to avoid generating noise (generic volunteer queries that flood the graph with irrelevant signals)?
- Is the VolunteerMatch/Eventbrite listing page problem worth solving (link-following to detail pages)?
