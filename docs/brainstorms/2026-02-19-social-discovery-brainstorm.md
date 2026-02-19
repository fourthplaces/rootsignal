---
date: 2026-02-19
topic: social-discovery
---

# Social Discovery: Find the Visible Layer

## What We're Building

The scout's discovery engine generates web search queries (TavilyQuery) to fill gaps in the tension landscape. But the most important signals for active community tensions don't live on the web — they live on social media, posted by individuals who are publicly visible while the organizations they protect stay hidden.

We extend the discovery engine to generate **social discovery topics** (hashtags + search terms) alongside web queries. These feed into platform-specific pipelines that search Instagram, X/Twitter, TikTok, and GoFundMe for individual voices, advocacy posts, and fundraiser campaigns. When discovery finds someone posting community signals, we auto-follow them as a Source so they get scraped on future runs.

## Why This Approach

### The problem

From the [volunteer coordinator interview](../interviews/2026-02-17-volunteer-coordinator-interview.md): organizations helping with immigration enforcement fear in Minnesota deliberately hide from public visibility (fear of ICE showing up at churches, daycares, food distribution points). Coordination happens via private group chats, word of mouth, and text. The organizations are invisible to web search.

But individuals step forward publicly. The volunteer posts every day on Instagram. GoFundMe campaigns are created under personal names to protect the churches. The signal is on social media — posted by people who are willing to be visible so the organizations don't have to be.

The scout currently only generates TavilyQuery sources (Google/web search). Social media scraping exists but only hits known accounts bootstrapped at cold start. The hashtag discovery pipeline (`discover_from_topics`) is fully built but the topics list is hardcoded to `Vec::new()` — dead code.

### Why LLM-generated topics

The discovery engine's LLM already sees the full tension landscape — unmet tensions, response shapes, emerging stories. It knows what's missing. Extending its output to include social-specific topics is one additional field in the same call, zero extra cost. The LLM can generate platform-appropriate queries: hashtags for Instagram (`#MNimmigration`, `#SanctuaryMN`), search terms for X/Twitter and GoFundMe, trending topics for TikTok.

### Why all platforms

Each platform carries different signal:
- **Instagram**: Individual volunteers, mutual aid posts, community organizing
- **X/Twitter**: Breaking advocacy, real-time organizing, policy response
- **TikTok**: Community stories, awareness campaigns, younger demographic
- **GoFundMe**: Fundraiser campaigns with structured data (amount, goal, location, organizer)

The Apify scrapers for all four already exist in `apify-client`. The marginal cost of wiring them in is low compared to the signal gain.

## Key Decisions

- **LLM generates social topics alongside web queries**: One call, two output types. The discovery LLM already understands the tension landscape — it generates hashtags and search terms in the same response.
- **Extract + auto-follow**: When hashtag/keyword discovery finds an individual posting community signals, create a Source node so they get scraped on future runs. Builds a growing network of individual voices. (This is what the existing `discover_from_topics` pipeline already does.)
- **GoFundMe campaigns are signals, not sources**: Each campaign becomes a signal directly (Ask or Give). The structured data (title, description, amount raised, goal, organizer, location) is rich enough to extract from without scraping the page.
- **Only the visible layer**: We find publicly posted content. We never expose what's hidden. This is the entire point — surface the people who chose to be visible so others can find them and help.

## Platform Wiring

| Platform | Apify scraper | Discovery method | Output | Auto-follow? |
|----------|--------------|-----------------|--------|-------------|
| Instagram | `apify/instagram-hashtag-scraper` | Hashtag search | Posts → LLM extraction | Yes |
| X/Twitter | `apidojo/tweet-scraper` | Keyword/hashtag search | Tweets → LLM extraction | Yes |
| TikTok | `clockworks/tiktok-scraper` | Keyword search | Captions → LLM extraction | Yes |
| GoFundMe | `jupri/gofundme` | Keyword search | Campaigns → signals directly | No (campaigns are signals) |

## Open Questions

- **Topic budget**: How many social topics per run? Apify calls cost money. 3-5 topics across platforms is probably right, but needs tuning.
- **X/Twitter trait integration**: The existing `SocialScraper` trait has `search_posts` (by account) and `search_hashtags` (Instagram-specific). X/Twitter keyword search may need a new trait method or generalization.
- **TikTok caption quality**: TikTok posts are video-first. Caption text may be sparse. Is caption-only extraction sufficient, or do we need video transcription eventually?
- **Facebook keyword search**: Apify's Facebook scraper only scrapes known pages, not keyword search. Facebook is not included in discovery — only existing page scraping continues.
- **Rate limiting**: Multiple platform searches per run could spike Apify costs. May need per-platform budgets.

## Next Steps

→ `/workflows:plan` for implementation details
