---
date: 2026-02-20
category: discovery
source: social-signal-brainstorm
---

# Web-to-Web Discovery Flywheel

## The Gap

The discovery and expansion systems create a self-reinforcing loop where web sources beget more web sources. Social media has no equivalent flywheel. The result: the system's heat and ranking metrics primarily reflect media coverage intensity, not community attention.

## The Flywheel

Every Tension extracted from a web page generates up to 3 `implied_queries` — web search strings routed through Serper. These become new `WebQuery` `SourceNode` entries. The next scout run scrapes them, extracts more Tensions, generates more queries. The loop:

```
Serper query → news article → LLM extracts Tension → implied_queries (up to 3)
  → new WebQuery sources created → Serper finds more articles
  → heat rises (more source_diversity) → more expansion queries → repeat
```

This is by design — the curiosity engine and signal expansion system are meant to chase emerging stories. The problem is that the loop is exclusively web-to-web. A Tension extracted from a Reddit post generates `implied_queries` that become Serper web searches, not social media keyword searches. The expansion never stays in the social channel.

## Social Has No Equivalent Loop

Social media discovery operates under structural constraints that prevent flywheel formation:

- **Hard caps**: 5 new social accounts per run maximum, only 3 of 5 generated topics actually searched (`MAX_SOCIAL_SEARCHES = 3`)
- **End-of-run topics discarded**: The `_end_social_topics` variable is explicitly ignored — social topics from the second discovery pass are thrown away
- **No social-to-social expansion**: `implied_queries` from social-sourced Tensions become `WebQuery` sources, not social keyword searches
- **Mechanical fallback is web-only**: When LLM budget runs out, `discover_from_gaps_mechanical()` generates only web queries — zero social topics
- **Reddit and Facebook excluded from keyword discovery**: `search_topics()` returns `Ok(Vec::new())` for these platforms — topic discovery only searches Instagram, Twitter, and TikTok

## Structural Disadvantages

Beyond the missing flywheel, social sources face compounding headwinds:

- **Budget cost**: Social scraping costs 2 cents per operation vs. 1 cent for web scraping (`APIFY_SOCIAL = 2` vs. `CHROME_SCRAPE = 1`)
- **Bootstrap ratio**: Cold start seeds ~25 web queries vs. ~8 social sources (Reddit subreddits + RSS)
- **Discovery ratio**: Per run, the LLM generates 3–7 web queries vs. 2–5 social topics
- **Platform coverage**: Only 5 social platforms are scraped. Nextdoor (hyper-local by design), YouTube (local creators, city council recordings), Google/Yelp reviews (place-level sentiment), and Meetup (direct gathering signal) are absent entirely
- **Bluesky recognized but silently skipped**: URLs are classified as `Social(Bluesky)` but the scrape phase executes `continue` and drops them

## The Epistemological Problem

`cause_heat` is defined as an epistemological measure — how well does the system understand why a signal exists? But the formula `heat += similarity × source_diversity` counts entity diversity without distinguishing channel diversity. Twelve news articles from different outlets produce high `source_diversity` and therefore high heat. The same tension with zero social engagement — no one on Reddit discussing it, no GoFundMe responding to it, no Instagram posts from affected community members — gets the same heat score.

This means heat measures **how much the media is covering something**, not **how much the community cares about it**. These are correlated but epistemologically distinct. A topic can dominate local news (city council budget vote) while generating zero social engagement. A topic can have massive social engagement (mutual aid organizing, neighborhood safety concerns) with zero news coverage.

The system's stated goal is to surface community signal. But the flywheel and heat formula together produce a media echo score wearing a community attention costume.

## What's Needed

1. **Channel diversity in heat**: Track which channel types (press, social, direct action, community media) have evidence for each signal. Use channel diversity as a multiplier in `cause_heat` so that cross-channel corroboration is rewarded over single-channel volume.

2. **Social-to-social expansion**: When a Tension is extracted from a social source, generate social discovery topics (hashtags, keywords) alongside web queries. Keep the expansion in-channel.

3. **Lift social caps**: Raise `MAX_SOCIAL_SEARCHES` and `MAX_NEW_ACCOUNTS`, use end-of-run social topics, add Reddit and Facebook to keyword discovery.

4. **Platform expansion**: Add Nextdoor, YouTube, Google Reviews, Meetup, and Bluesky via Apify actors. These carry signal types that web sources rarely capture.

5. **Mechanical fallback parity**: When LLM budget runs out, the mechanical gap generator should produce social topics alongside web queries.

6. **Budget rebalancing**: The 2x cost for social scraping disproportionately throttles the already-capped social discovery pipeline.

## Related

- Bias audit finding B8 (Platform Coverage Bias): `docs/audits/scout-bias-brittleness-2026-02-19.md`
- Signal scoring: `docs/brainstorms/community-signal-scoring-brainstorm.md`
- Heat model: `docs/vision/tension-gravity.md`
- Feedback loops: `docs/architecture/feedback-loops.md`
