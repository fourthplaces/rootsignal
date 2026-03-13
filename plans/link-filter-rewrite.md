# Plan: Rewrite link extraction filter from scratch

## Problem

`extract_links` in `link_promoter.rs` uses growing blocklists to reject junk URLs.
It's reactive (new junk → new entry) and leaks obvious non-content URLs like
`manifest.json`, `cash.app/`, and CDN asset paths. The function also mixes
structural concerns (is this a static asset?) with semantic ones (is google.com
relevant?) that belong in the LLM domain filter.

## Design

Replace `extract_links` with a clean three-pass structural filter. Each pass
answers one question. If a URL fails any pass, it's dropped.

### Pass 1 — Scheme gate

**Question:** Is this a web URL?

Accept only `http://` and `https://`. Rejects `mailto:`, `tel:`, `javascript:`,
`data:`, `ftp:`, fragment-only (`#anchor`).

### Pass 2 — Static asset gate

**Question:** Does this URL point to a file or build artifact?

Two checks:

1. **Extension**: reject if the path (before query string) ends with a known
   static extension. Comprehensive list — these don't change:
   `.css`, `.js`, `.json`, `.xml`, `.webmanifest`, `.map`,
   `.png`, `.jpg`, `.jpeg`, `.gif`, `.svg`, `.webp`, `.ico`, `.avif`,
   `.woff`, `.woff2`, `.ttf`, `.eot`,
   `.mp3`, `.mp4`, `.webm`, `.ogg`,
   `.pdf`, `.zip`, `.gz`, `.tar`,
   `.txt`, `.csv`, `.rss`, `.atom`

2. **Asset path pattern**: reject if the path contains a segment that signals
   build tooling or static hosting:
   `/_next/`, `/static/`, `/assets/`, `/dist/`, `/build/`,
   `/wp-includes/`, `/wp-content/plugins/`, `/wp-content/themes/`,
   `/cdn-cgi/`, `/wp-json/`, `/xmlrpc`, `/cgi-bin/`

   Removed from earlier draft: `/api/` (over-broad — catches real SPA content;
   `/wp-json/` and `/xmlrpc` already cover the WordPress API case),
   `/embed/` and `/oembed` (blocks community calendars and video embeds,
   contradicts unblocking YouTube from domain list).

   **Important:** Percent-decode the path before matching. `%2Fstatic%2F`
   must trigger the same rule as `/static/`.

### Pass 3 — Infrastructure host gate

**Question:** Is this host structurally non-content?

Two checks:

1. **Subdomain prefix**: reject if the host (after stripping `www.`) starts with
   `assets.`, `static.`, `cdn.`, `api.`, `fonts.`, `analytics.`,
   `accounts.`, `login.`, `auth.`.
   These are infrastructure subdomains regardless of the parent domain.

2. **Universal infrastructure domains**: a small, stable set of domains that are
   purely infrastructure. These will not grow over time — they're the bedrock of
   the web, not community content.

   **Matching rule:** exact domain or subdomain — `domain == X || domain.ends_with(".X")`.
   Never substring contains (otherwise `segment.com` matches `marketsegment.com`).

   - CDN/analytics: `googleapis.com`, `gstatic.com`, `googletagmanager.com`,
     `google-analytics.com`, `doubleclick.net`, `cloudflare.com`,
     `cdn.jsdelivr.net`, `unpkg.com`, `bootstrapcdn.com`, `fontawesome.com`
   - Web standards: `w3.org`, `ietf.org`, `iana.org`, `schema.org`, `ogp.me`,
     `xmlns.com`
   - Metadata/ontology: `purl.org`, `dublincore.org`, `rdfs.org`
   - Monitoring: `segment.com`, `hotjar.com`, `newrelic.com`, `sentry.io`

### Then: sanitize and dedup

Strip tracking params via existing `sanitize_url`, dedup by `canonical_value`.
Same as today — this part works fine.

## What gets removed

- `permissive` parameter — the strict/permissive toggle mixed structural and
  semantic concerns. The LLM domain filter handles semantic judgments.
- `STRICT_ONLY_BLOCKED_DOMAINS` (google.com, youtube.com, facebook.com, etc.) —
  these are semantic calls. A youtube.com link might be a community video. Let
  the LLM decide.
- `SKIP_PATH_SEGMENTS` for `/privacy`, `/legal`, `/terms`, `/cookie`, etc. —
  these are real pages. Whether they're worth scraping is a semantic judgment.
- `ALWAYS_BLOCKED_DOMAINS` entries that are niche or speculative (meyerweb.com,
  tantek.com, photomatt.net, myopenid.com, getty.edu, loc.gov, opensource.org,
  creativecommons.org). The structural filter shouldn't need to know about Eric
  Meyer's CSS reset blog.

## What stays the same

- Function signature: `extract_links(page_links: &[String]) -> Vec<String>`
  (drop the `permissive` param).
- Called from `web_scrape.rs` — update the one call site.
- `CollectedLink` struct, `PromotionConfig`, `promote_links` — untouched.
- Social handle extraction — untouched, separate concern.

## Call site changes

`web_scrape.rs:183`:
```rust
// Before:
let discovered = link_promoter::extract_links(&page_links, false);
// After:
let discovered = link_promoter::extract_links(&page_links);
```

Any other callers of `extract_links` with `permissive: true` — update to drop
the parameter.

## Tests

Rewrite the test suite for `extract_links` to match the new design. Tests
organized by pass:

1. **Scheme gate tests**: non-http schemes rejected, http/https accepted
2. **Static asset tests**: each extension category, asset path patterns
3. **Infrastructure host tests**: subdomain prefixes, universal domains
4. **Integration**: mixed bag of real Linktree links (from the bug report) —
   assert junk rejected, legitimate links preserved
5. **Sanitize + dedup**: tracking params stripped, duplicates collapsed

Delete all existing `extract_links` tests (they test the blocklist approach).

## Separate issues (not in this PR)

1. **Fail-open policy** in `domain_filter_gate.rs` that auto-accepts all sources
   when AI is unavailable. Needs its own fix — fail-closed or queue for later.

2. **Share-intent URLs** (`facebook.com/sharer/`, `twitter.com/intent/tweet`,
   `linkedin.com/sharing/`) are structural junk but look like social URLs.
   The `channel_type` classifier in `domain_filter_gate` auto-accepts social
   URLs, so share widgets get waved through. Fix belongs in channel_type
   classification, not the structural filter.

## Pressure test notes

Reviewed 2026-03-06. Key decisions:

- **`/api/` removed** from asset path patterns. Over-broad — modern SPAs route
  real content through `/api/`. The WordPress-specific `/wp-json/` and `/xmlrpc`
  already cover that case.
- **`/embed/` and `/oembed` removed**. Community calendars (`calendar.google.com/embed`)
  and video embeds are real content. Contradicts unblocking YouTube.
- **`accounts.`, `login.`, `auth.` added** to subdomain prefixes. High-volume
  structural junk from auth flows.
- **Exact-domain matching specified**. `domain == X || domain.ends_with(".X")` to
  prevent `segment.com` from matching `marketsegment.com`.
- **Percent-decoding required** before Pass 2 pattern matching. `%2Fstatic%2F`
  must not bypass `/static/`.
- **RSS/Atom/XML feeds stay blocked**. The origin domain will be discovered via
  other links on the same page. If the only link to a source is its feed URL,
  that's too thin a signal to act on.
- **Payment platforms (venmo, cashapp, paypal) not blocked**. Semantic call —
  LLM domain filter territory.
- **App store links not blocked**. Same — semantic, not structural.
