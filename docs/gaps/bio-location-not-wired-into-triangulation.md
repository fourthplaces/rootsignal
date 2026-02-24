---
date: 2026-02-23
topic: bio-location-triangulation
status: identified
related: docs/gaps/actor-location-investigation.md
---

# Gap: Actor Bio Location Not Wired Into Triangulation

## The Problem

`triangulate_actor_location` supports bio location as an input — a corroborated bio wins outright, an uncorroborated bio counts as one signal vote. But `enrich_actor_locations` always passes `None` for `bio_location`.

`ActorNode.bio` is raw text (e.g. "Based in Phillips"). There is no parsing step that extracts a structured `ActorLocation` (lat/lng/name) from bio text. The bio corroboration and uncorroborated-bio-as-one-vote code paths in `triangulate_actor_location` are unreachable in production.

## Impact

Actors whose bio says "Based in Phillips" but whose signals are split 2-2 between Phillips and Powderhorn will get an arbitrary location instead of Phillips (bio would break the tie). Actors with a clear bio location and only 1 signal get no location at all (bio + 1 signal = 2 votes, which would be sufficient).

## What's Needed

1. Bio-to-location parsing: extract neighborhood/city from `ActorNode.bio` text and geocode it to lat/lng/name.
2. Wire the parsed bio location into `enrich_actor_locations` as the `bio_location` parameter.

This could be LLM-based (ask the model to extract location from bio) or rule-based (regex for "Based in X", "Located in X", etc.). The IG bio-location chain already parses bio text upstream during social scrape — that parsed result could be stored on `ActorNode` and reused here.
