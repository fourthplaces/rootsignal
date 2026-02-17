---
date: 2026-02-17
topic: email-ingest-for-signal
---

# Email Ingest for Signal Capture

## What We're Building

An automated email ingestion pipeline that subscribes to org newsletters and parses incoming emails through the existing LLM extraction pipeline. Instead of scraping org websites (which are often stale), the system receives the freshest signal orgs produce — their newsletters — delivered on the org's own cadence.

This is a new signal capture modality that complements web scraping and social media ingestion. It's particularly important for **grassroots and informal signal** — small orgs that maintain a Mailchimp list but not a well-maintained website.

## Why This Matters

The current pipeline is structurally biased toward institutional signal — orgs with websites, social media presence, and event platform listings. The biggest signal loss in Root Signal's model is grassroots signal: mutual aid groups, faith communities, small neighborhood orgs, and community networks that coordinate through email rather than the open web.

Newsletters are often the single freshest artifact an org produces. A food shelf that updates its website quarterly sends a weekly email saying "we're out of diapers and need 3 volunteers Saturday." That email is the signal. The website is the brochure.

## Why Email Ingest Over More Scraping

| Property | Web Scraping | Email Ingest |
|---|---|---|
| Freshness | Depends on scrape cadence | Real-time (org pushes to you) |
| Fragility | HTML changes break scrapers | Email format is stable |
| Cost | Per-scrape compute + API calls | Near-zero marginal cost |
| Anti-bot risk | Constant cat-and-mouse | None (you're a subscriber) |
| Signal density | Full page, mostly boilerplate | Curated — org tells you what matters this week |
| Coverage | Only what's on the public web | Reaches orgs with no web presence beyond email |
| Public signal? | Yes | Yes — public mailing list subscription |

## How It Works

```
Org newsletter arrives at ingest@rootsignal.org (or per-city addresses)
  → Email receiver (SES / Cloudflare Email Workers / IMAP poller)
    → HTML-to-text conversion (strip styling, extract links)
      → LLM extraction (same Extractor pipeline)
        → Dedup (same embed + title + hash layers)
          → Graph storage
```

The key insight: **the extraction pipeline already exists.** Email ingest is a new front door to the same pipeline, not a new pipeline.

### Email Receiver Options

**Cloudflare Email Workers** — Route emails to a Worker that calls the extraction API. Cheapest, serverless, integrates with Cloudflare DNS. Limitation: 25MB message size, but newsletters are tiny.

**AWS SES Inbound** — Receive emails, store in S3, trigger Lambda. Battle-tested at scale. More infrastructure to manage.

**IMAP Poller** — Set up a mailbox (Fastmail, Gmail), poll it on a cron. Simplest to build, easiest to debug. No webhook complexity. Could run as a mode of rootsignal-scout.

**Recommendation for first pass:** IMAP poller. It's the boring choice. You set up a mailbox, the scout checks it periodically, processes new messages. No new infrastructure. No webhook endpoints to secure. You can switch to something event-driven later if volume demands it.

### Email-to-Text Conversion

Newsletters are HTML emails. The same approach used for web scraping works here — convert HTML to readable text, strip navigation/footer boilerplate, preserve links. Libraries like `html2text` or feeding the raw HTML through the same markdown conversion the scraper uses.

One advantage over web scraping: newsletters have much less boilerplate. No nav bars, no cookie banners, no sidebars. The signal-to-noise ratio is higher in email than on a webpage.

### Source Trust and Attribution

- Source URL becomes the newsletter's archive link if one exists, otherwise `mailto:` or org domain
- Source trust inherits from the org's existing trust score, or gets a baseline `.org = 0.8` score
- Every extracted signal links back to the org (same attribution principle)
- The org mapping system already handles multi-source orgs — email becomes another source type alongside website, Instagram, Facebook

### Dedup Considerations

Email ingest creates a new dedup scenario: the same signal may appear in the org's newsletter AND on their website AND on their Instagram. This is actually a feature, not a bug — cross-source corroboration increases confidence. The existing dedup pipeline handles this naturally:

1. Title + type exact match → corroborate existing node
2. Vector similarity → corroborate if above threshold
3. New signal → create node

The newsletter version often has richer context than the website version, so if it arrives first, it becomes the primary node. If it arrives second, it corroborates.

## Bootstrapping the Subscription List

This is the real question. Three approaches, not mutually exclusive:

### Approach A: Manual Subscribe (Start Here)
Subscribe to newsletters from the orgs already in `curated_sources`. You already track ~50 orgs for Twin Cities. Most have newsletter signups on their websites. A human spends 2 hours subscribing to 30-40 lists. Done.

**Pros:** Immediate, high-quality, known-good orgs.
**Cons:** Doesn't scale. Doesn't discover new orgs.

### Approach B: Crawl for Signup Links
The scraper already visits org websites. Add a detection step: look for newsletter signup forms, Mailchimp/Constant Contact/Substack embed patterns, "subscribe" links. Build a list of discoverable newsletters. Could be automated or semi-automated (flag for human to confirm subscription).

**Pros:** Scales with source discovery. Finds newsletters you didn't know about.
**Cons:** Subscribing programmatically to forms is ethically gray and technically fragile.

### Approach C: Org Dashboard Onboarding (Milestone 6)
When orgs claim their profile in Root Signal, one onboarding step is: "Add signal@rootsignal.org to your newsletter list." The org opts in. Cleanest consent model. Highest quality.

**Pros:** Org-initiated, clear consent, builds relationship.
**Cons:** Depends on Milestone 6. Chicken-and-egg — orgs won't onboard until Root Signal has value.

**Recommendation:** Start with A. It proves the modality works with minimal investment. Move to C when the Org Dashboard exists. B is a nice-to-have for source discovery but not essential early.

## What This Unlocks

### Grassroots Signal That Doesn't Exist on the Web
The church with 200 members that sends a weekly email but has a website last updated in 2019. The mutual aid network that coordinates via Mailchimp but has no social media. The neighborhood association that sends a monthly digest. These orgs produce high-quality, hyper-local signal that the current pipeline can't see.

### Higher Freshness Baseline
Newsletters are inherently time-bound — "this week's volunteer needs," "upcoming events," "urgent ask." The signal arrives fresh by definition. This directly addresses the Dead Feed Problem (Kill Test #1).

### Org Relationship Foundation
Subscribing to a newsletter is the gentlest possible first contact with an org. When you later reach out to say "your newsletter is helping people find volunteer opportunities through Root Signal," that's a natural bridge to the Org Dashboard conversation.

### Signal Cadence Intelligence
Email arrival patterns reveal org activity rhythms. An org that usually sends weekly but sends two emails in a day is signaling urgency. An org that stops sending is signaling a potential closure or capacity issue. This is metadata the scraping pipeline can't capture.

## Open Questions

- **Unsubscribe handling:** If an org unsubscribes Root Signal, that's a clear signal to respect. But how do we detect and handle this gracefully?
- **Volume management:** Some orgs send daily. Is there a dedup/batching strategy needed to avoid re-extracting the same ongoing opportunities every day?
- **Multi-city scaling:** One shared inbox, or per-city inboxes? Per-city is cleaner for routing but more to manage.
- **Spam filtering:** A public email address will get spam. Need a pre-filter before hitting the LLM extraction pipeline (cost control).
- **Forwarding model:** Should community members be able to forward emails they receive? ("Forward any community newsletter to signal@rootsignal.org.") This is powerful for discovery but introduces trust/quality concerns.

## Next Steps

→ Prove the modality with a manual test: subscribe to 10 Twin Cities org newsletters, forward them to a test inbox, run them through the existing extraction pipeline, and assess signal quality vs. the same orgs' scraped web content. If the newsletter signal is meaningfully richer or fresher, build the automated pipeline.
