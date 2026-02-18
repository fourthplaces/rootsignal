---
date: 2026-02-17
topic: community-signal-scoring
---

# Community Signal Scoring

## What We're Building
Replace raw corroboration_count with a "community signal" score that measures genuine community attention rather than source posting volume. A signal mentioned by 5 different organizations is meaningful. A signal mentioned 30 times by its own Facebook page is not.

## Why This Approach
The original idea was to add social media attention weighting to boost Asks/Gives when people are talking about an issue. Investigation of live data revealed the prerequisite problem: corroboration_count conflates self-promotion with community attention. Fixing that IS the feature — no external sentiment API needed.

## Key Decisions
- **Source diversity over volume**: Count distinct domains/orgs, not total evidence nodes
- **External ratio matters**: Mentions from sources other than the signal's own org are the real signal
- **No external API**: The data already exists in the graph; it's a scoring fix, not a new system
- **Two-component score**: (1) unique source count, (2) external mention ratio

## Evidence from Data
- Habitat Winter Warriors: corr=30, but only 2 sources (own Facebook + own Instagram)
- End Hunger Radiothon: corr=29, but only 2 sources (own Facebook + own Instagram)
- Bridging Volunteer: corr=28, but only 1 source (single Facebook page)
- ICE-related signals naturally have higher source diversity (MIRAC, NHN, news outlets, community aid networks)

## Open Questions
- Should self-sourced evidence count at all, or just be weighted lower?
- How to detect "same org" across different domains (e.g. facebook.com/tchabitat and instagram.com/tchabitat)?
- Should the community signal score feed back into confidence, or be a separate ranking dimension?

## Next Steps
→ `/workflows:plan` for implementation details
