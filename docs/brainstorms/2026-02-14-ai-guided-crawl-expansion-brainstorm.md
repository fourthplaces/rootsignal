---
date: 2026-02-14
topic: ai-guided-crawl-expansion
---

# AI-Guided Crawl Expansion

## What We're Building

A feedback loop that uses extraction quality to drive crawl behavior. Rather than adding a separate AI triage step before extraction, we use the extraction results themselves — which we already produce — as the signal for where to crawl next. High-quality extractions expand the crawl frontier; low-quality ones naturally dead-end.

The crawl becomes self-directing: seed sources are crawled broadly, and the system learns which pages, domains, and content patterns are worth pursuing more of.

## Why This Approach

The alternative was an AI pre-filter that evaluates each page before extraction to decide if it's worth processing. But since you already have to fetch the page to evaluate it, the real cost being saved is the expensive extraction step — not the crawl itself. That means you'd be adding a cheap AI pass to gate an expensive AI pass, which adds complexity without leveraging the signal you already produce.

By using extraction confidence scores (`confidence_overall`, `confidence_ai`) as the crawl heuristic, you avoid a whole new evaluation layer and build on data that already exists.

## The Flywheel

After each extraction batch, three expansion mechanisms work together:

### 1. Link Following
Good page at `example.com/events/pottery-night` → crawl sibling pages at `example.com/events/*`. High-confidence extractions from a URL path trigger broader crawling of adjacent paths on the same domain.

### 2. Semantic Discovery
Take the best extractions, generate search queries from their content, and feed those into Tavily to discover entirely new sources elsewhere on the web. The system finds more of what's already working.

### 3. Domain Trust Scoring
Track what percentage of pages from a given domain produce good extractions. High hit rate → increase `max_crawl_depth`, mark `is_trusted`, crawl more aggressively on the next cycle. Low hit rate → reduce depth, deprioritize.

## Existing Infrastructure

These pieces already exist and feed directly into this design:

- `confidence_overall` / `confidence_ai` on extractions → quality signal
- `WebsiteSource.is_trusted` and `max_crawl_depth` → domain trust knobs
- Tavily adapter → semantic discovery engine
- Firecrawl with `max_depth` → link following
- Source config JSONB → flexible per-source tuning
- Restate workflows → durable orchestration

## Key Decisions

- **Feedback loop as a separate workflow:** A `CrawlExpansionWorkflow` that runs after extraction completes, keeping the existing scrape → extract pipeline clean and making the feedback loop independently observable and tunable.
- **Extraction quality drives crawl, not a pre-filter:** No separate triage AI step. The extraction pipeline is both the processor and the evaluator.
- **Three expansion mechanisms combined:** Link following, semantic discovery, and domain trust scoring all feed the crawl frontier together.

## Open Questions

- What confidence threshold triggers expansion? Static cutoff vs. relative (top N% of extractions)?
- How aggressively should domain trust decay? If a previously good source starts producing low-quality results, how fast do we dial it back?
- Should semantic discovery (Tavily) queries be generated automatically from extraction data, or should there be a human-in-the-loop step for new source approval?
- Rate limiting / budget — how do we cap the expansion so the flywheel doesn't spiral into unbounded crawling?
- Should we track a "crawl frontier" explicitly (a queue of candidate URLs with priority scores) or keep it implicit in source configuration?

## Next Steps

→ `/workflows:plan` for implementation details
