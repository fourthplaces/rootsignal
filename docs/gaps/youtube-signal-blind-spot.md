---
date: 2026-02-22
category: discovery
source: signal-collection-analysis
---

# YouTube Signal Blind Spot

## The Gap

The scout has no YouTube integration. YouTube carries signal types that no other platform captures well: city council meeting recordings, community organization livestreams, local journalist video reports, neighborhood association updates, and civic event documentation. This content is long-form, high-context, and often the primary record of community decision-making.

## Why It Matters

YouTube is where the institutional and grassroots record lives:

- **City council / school board meetings**: Full recordings of public hearings, votes, and testimony. Often the only accessible record of what was said and decided. Community members testifying about local issues produce raw first-hand signal.
- **Community organization channels**: Mutual aid groups, legal clinics, and advocacy orgs post update videos — "here's what we did this week", "here's what we need", "here's what's happening in the community"
- **Local journalists and community reporters**: Independent reporters covering hyper-local stories that never make it to mainstream news. Video reports from the scene of events, interviews with affected people.
- **Livestreams of events**: Rallies, community meetings, resource distributions, protests — often streamed live and preserved as recordings
- **Explainer and know-your-rights content**: Organizations posting "what to do if ICE comes to your door", "how to access emergency housing", "where to get free legal help" — direct Aid signals

These map directly to the system's signal types: Gatherings (meeting recordings), Needs (testimony about community problems), Aid (resource guides, know-your-rights), Tensions (public hearing debates, journalist reports).

## Why YouTube Is Different

Unlike Instagram stories (visual, ephemeral, needs vision), YouTube content is:

- **Persistent** — videos stay up indefinitely, unlike stories
- **Already transcribed** — YouTube auto-generates captions for most videos; many creators also upload manual transcripts
- **Long-form and high-context** — a 2-hour city council meeting has dense signal that no tweet or Instagram post captures
- **Structured around channels** — an organization's YouTube channel is a durable, follow-able source, similar to an Instagram profile

The transcription angle means YouTube integration wouldn't need media processing — just fetch the transcript text via YouTube's caption API or an Apify actor, then feed it through the existing text extraction pipeline.

## Scale of the Blind Spot

For a mid-size city (200k-500k population):
- City council posts 2-4 meeting recordings per month (2-4 hours each)
- School board posts 1-2 per month
- 5-15 active local journalist/community reporter channels posting weekly
- 3-10 community organization channels posting 1-4x per month

This is hundreds of hours of high-quality community signal per month that the system cannot see.

## Technical Path

YouTube transcripts are text — once fetched, they feed directly into the existing extraction pipeline with no vision or audio processing needed. Apify has YouTube scraper actors that can fetch video metadata and transcripts. The main challenge is handling long-form content (a 2-hour meeting transcript is much longer than an Instagram caption) — likely needs chunking or summarization before extraction.

## Related

- Platform coverage bias: `docs/audits/scout-bias-brittleness-2026-02-19.md` (finding B8)
- Web-to-web flywheel gap: `docs/gaps/web-to-web-flywheel.md`
- Instagram stories blind spot: `docs/gaps/instagram-stories-blind-spot.md`
