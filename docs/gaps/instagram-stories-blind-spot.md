---
date: 2026-02-22
category: discovery
source: signal-collection-analysis
---

# Instagram Stories Blind Spot

## The Gap

The scout scrapes Instagram post captions but is completely blind to Instagram stories. Many organizations — mutual aid groups, legal clinics, community organizers, grassroots collectives — run campaigns primarily through stories. Event flyers, urgent calls to action, meeting announcements, resource distribution updates, and real-time incident reports are posted as stories, not feed posts.

Stories are ephemeral (24h) and primarily visual (text overlaid on images, flyer graphics, screenshot shares). The current text-only pipeline cannot see them.

## Why It Matters

Stories are the primary communication channel for time-sensitive community organizing:

- **Event flyers**: Community meetings, rallies, resource distribution events are announced as story images with date/time/location details
- **Calls to action**: "We need volunteers tonight", "Bring supplies to this address", "Call this number if you see ICE" — posted as stories for urgency
- **Real-time updates**: "Distribution happening now at [location]", "Legal observers needed" — ephemeral by design
- **Campaign coordination**: Multi-day campaigns where each day's story builds on the last, with different asks and updates
- **Information sharing**: Screenshots of news articles, government announcements, or resource guides shared as story images with annotation

These are exactly the signal types the system exists to capture — Gatherings, Needs, Aid, Tensions — but they're invisible because they're images, not text.

## Scale of the Blind Spot

For an active community organization with 5k+ followers:
- Feed posts: 2–5 per week (mostly curated, polished content)
- Stories: 5–20 per day during active campaigns (raw, real-time, operational)

The system sees 2–5 signals per week from the feed. It misses 35–140 story signals per week from the same account. The ratio inverts further during crisis events when story posting accelerates and feed posting stops.

## Technical Path

The Apify `louisdeconinck/instagram-story-details-scraper` actor (ID: `9pQFsbs9nqUI64rDQ`) can fetch active stories for any public account without authentication. It returns image URLs, timestamps, user metadata, and link stickers.

Claude Haiku 4.5 supports vision and can extract text from story images at ~$0.0005 per image. The extracted text feeds into the existing `SocialPost` → extraction → dedup pipeline with no changes to downstream processing.

Full implementation plan: `docs/plans/2026-02-22-feat-instagram-stories-vision-pipeline-plan.md`

## Related

- Platform coverage bias: `docs/audits/scout-bias-brittleness-2026-02-19.md` (finding B8)
- Web-to-web flywheel gap: `docs/gaps/web-to-web-flywheel.md`
- Entity-centric signal collection: `docs/plans/2026-02-21-feat-censorship-resilient-signal-collection-plan.md`
- Social discovery plan: `docs/plans/2026-02-19-feat-social-discovery-plan.md`
