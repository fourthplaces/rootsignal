---
date: 2026-02-21
topic: censorship-resilient-signal-collection
---

# Censorship-Resilient Signal Collection

## What We're Building

A trust-anchored signal collection architecture that centers direct account-following of verified civic voices as the primary social media channel, with platform search/feeds retained as a secondary discovery and censorship-measurement channel. This makes the scout resilient to the three layers of information corruption: platform censorship, AI-generated noise, and algorithmic manipulation.

## Why This Approach

The information environment is actively hostile. Social media platforms censor civic content through algorithmic suppression, content removal, and API restrictions. Simultaneously, the open web is flooded with AI-generated content, astroturfing campaigns, and coordinated political noise that drowns out genuine community voices. Hashtag queries return walls of political commentary from people with no connection to the issue.

The self-evolving system vision assumed a noisy-but-honest web where sources could emerge through open discovery and earn trust through evidence. That assumption no longer holds. When the environment itself is corrupted, open ingestion produces garbage. Letting sources "emerge" from a poisoned environment means the emergent behavior inherits the corruption.

The correction: **the bootstrap isn't just a seed — it's the immune system.** Trusted sources with real-world touchpoints (physical presence, human staff, in-community work) are the anchor. Discovery still runs, but it expands outward from trusted nodes rather than discovering in the wild and hoping the evidence-based trust catches the bad ones.

## Key Decisions

- **Watchlist is primary, not supplementary**: Direct account-following of verified civic voices is the main social media signal channel — not a supplement to search
- **Trust-anchored discovery**: Seed watchlists manually per region, then let scout discovery agents expand by following the social graph of trusted accounts (mentions, retweets, interactions) — not by searching the open web
- **Account types**: Civic orgs with physical presence, local journalists with beats, community activists/organizers with direct involvement — all per-region, all verifiable in the real world
- **Search/feeds as measurement**: Platform search continues running primarily to measure censorship (compare what watchlist accounts post vs. what appears in search) and as a secondary discovery channel understood to be filtered and noisy
- **Censorship as signal**: Track suppression patterns as a first-class civic tension
- **First-hand filter on search/feeds**: When platform search does produce results, apply strict first-hand filtering at the LLM extraction layer to separate lived experience from political noise

## Architecture Sketch

### Layer 1: Trusted Accounts (Primary Channel)
- Curated accounts per region across platforms (Instagram, Facebook, Reddit, TikTok)
- Seeded manually with sources that have verifiable real-world touchpoints
- Expanded by scout discovery agents following the social graph outward from trusted nodes
- Accounts still enter the evidence-based trust pipeline — watchlist membership doesn't grant blanket trust
- Posts scraped directly from account profiles via Apify

### Layer 2: Search/Feeds (Secondary — Measurement + Discovery)
- Existing platform search and feed scraping continues
- Understood as corrupted: algorithmically filtered, flooded with noise, actively censored
- First-hand filter applied at LLM extraction layer (see Signal Filter below)
- Primary value: censorship measurement and occasional discovery of new community voices
- Trust score adjusted downward based on censorship metrics

### Layer 3: Censorship Observatory
- Compare trusted account posts against search/feed visibility
- Track suppression rates per platform, topic, and region
- Surface censorship patterns as Tension signals in the graph
- Feed censorship metrics back into source weighting and system confidence

## Signal Filter: Two Layers

Root Signal maps civic reality, not political discourse. Two layers determine whether a post becomes signal:

### Layer 1: First-Hand Only

Is this person in this? Is this their life? A post must describe something happening to the poster, their family, their community, or their neighborhood — or be a request for help. Political commentary from people not personally affected is noise regardless of viewpoint.

**First-hand:** "My family was taken." "There were raids on 5th street today." "This happened in my neighborhood." "Our food bank is out of supplies." "Does anyone know a tenant lawyer?"

**Not first-hand:** "ICE is doing great work." "The housing crisis is a failure of capitalism." "This is why we need border security."

### Layer 2: The Existing Inclusion Test

Even first-hand content must connect to action or context. Root Signal's attention flows toward the wound and the response — needs, tensions, responses, gatherings, requests for help. First-hand endorsement of the thing causing harm isn't a need, a response, or a call for help. It doesn't help anyone act.

The kids hiding in their house — that's the signal. The legal aid hotline — that's the signal. The neighbor organizing a know-your-rights training — that's the signal. Everything else is noise.

### Scope

The two-layer filter applies specifically to **platform search/feed scraping** (Layer 2 of the architecture) — the channel where political noise and AI-generated content are most concentrated. It is enforced at the LLM signal extraction layer.

It does NOT apply to:
- **Trusted account posts** (Layer 1) — these sources are already vetted for real-world involvement
- **News feeds** — already editorially filtered
- **Org websites** — already meet the inclusion test by nature
- **Government/institutional sources** — grounded by definition

The watchlist approach makes this natural — the accounts worth following are the ones that consistently post from direct involvement or relay help requests. The first-hand filter catches what slips through on the noisier channels.

## Open Questions

- What's the right Apify actor pattern for direct account scraping vs. search scraping?
- How do we model watchlist accounts in the graph? New node type or extension of Source?
- What threshold of suppression constitutes a reportable censorship tension?
- How do we handle account churn (deactivations, bans, pivots)?
- Rate limiting / cost implications of scraping individual accounts at scale?

## Next Steps

→ `/workflows:plan` for implementation details
