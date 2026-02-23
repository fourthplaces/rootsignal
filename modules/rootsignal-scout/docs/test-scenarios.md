
// some way to plumb this into archive

let queries = {
    "site: linktr.ee mutual aid": vec![
        Link {
            url: "http://some-site"
        }
    ],
    "http://some-site": Page {
        body: "<html></html>"
    }
}


cases:

- actor is in MN, post is about texas
    - signal is in texas, actor is in minnesota
- actor is in MN, post location is absent
    - post location is tagged in MN
- search `site:linktr.ee mutual aid` -> scrape pages
    - should pull instagram links, google links, links of interest
    - should pull actor if resent
    - should create actor if NOT present and link to all sources
        - linked_from actor
    - ensure that links that do NOT belong to actor are not attached as so. Could just be links out
- when scanning instagram profile, check for actor
    - create actor if present
- combine actors if possible. E.g: instagram + facebook account should share same actor
    - potentially infer ownership based on links rather than being explicit


  Location mismatch scenarios:
  - Actor bio says "Portland, OR" but they post about a local restaurant in
  "Portland, ME" → actor in OR, signal in ME
  - Actor has no location in bio, but posts about a specific address in Chicago →
  actor has no geo, signal pinned in Chicago
  - Actor bio says "NYC" and posts a generic opinion piece with no location cues →
  actor in NYC, signal has no geo

  Ambiguity scenarios:
  - Actor bio says "Springfield" (no state) and posts about a Springfield event →
  how is disambiguation handled?
  - Actor says "Bay Area" in bio → does that resolve to San Francisco, or a broader
  region?

  Multi-location scenarios:
  - Actor bio says "Based in Denver, traveling in Austin" → which location wins for
  the actor?
  - A single post mentions events in both Dallas and Houston → does the signal get
  pinned to one, both, or neither?

    Edge cases:
  - Actor bio says "Worldwide" or "Everywhere" → no specific geo
  - Actor posts about "the best pizza in New York" but they're clearly in LA
  reviewing a local spot inspired by NY-style → signal should be LA, not NY
  - Actor bio has a location in a non-US country → should it be filtered out or
  kept?

  Consistency scenarios:
  - Actor posts 10 signals all in the same city that differs from their bio location
   → does anything update?
  - Actor bio changes from Minnesota to Texas between runs → actor geo should update

  Want me to look at the actual test files and data structures to make these more
  concrete to your codebase?



  Linktree → Social Profiles
  1. Search site:linktr.ee mutual aid Minneapolis → Serper returns Linktree page
  URLs → scrape page → find instagram.com/mplsmutualaid link → promoted as Social
  source → next run scrapes Instagram posts

  Linktree → Google Doc
  2. Same Linktree page has docs.google.com/document/d/ABC123/edit?usp=sharing link
  → classified as GoogleDoc → normalized to /d/ABC123 → promoted as WebPage source →
   next run fetches via HTML export (the change we already landed) → markdown
  extracted → signals found

  Linktree → GoFundMe
  3. Linktree links to gofundme.com/f/help-displaced-families-mpls → classified as
  Fundraiser → promoted as Response source → next run scrapes campaign page →
  extracts Aid/Need signals from campaign description

  Linktree → Eventbrite
  4. Link to eventbrite.com/e/mutual-aid-distribution-12345 → classified as Event →
  promoted as Response source → next run scrapes event page → extracts Gathering
  signal with date/location

  Linktree → Amazon Wishlist
  5. Link to amazon.com/hz/wishlist/ls/ABC123?ref_=cm_wl_huc_do → classified as
  AmazonWishlist → tracking params stripped → promoted as Response source → scrape
  reveals needed supplies (diapers, sleeping bags, etc.)

  Linktree → Discord/Telegram
  6. Link to discord.gg/mutualaid → classified as CommunicationChannel → promoted
  with low weight (0.25) → metadata captured but content mostly private

  Linktree → Another Org Website
  7. Link to crownheightsmutualaid.org → classified as WebPage (catch-all) →
  promoted → next run scrapes the org's homepage → finds more links (their own
  Instagram, Google Docs, etc.) → recursive discovery

  Blocklist Filtering
  8. Same page has links to fonts.googleapis.com, cdn.jsdelivr.net,
  googletagmanager.com, linktr.ee/someuser (self-referential aggregator) → all
  filtered out, never become sources

  Dedup
  9. Two different Linktree pages both link to the same instagram.com/mplsmutualaid
  → normalized to same canonical key → upsert_source MERGE semantics → one source,
  not two

  Social Mention Path (backward compat)
  10. Instagram posts from @mplsmutualaid mention @northsidemutualaid in post text →
   collected as DiscoveredLink::Social(Instagram, ...) → promoted same as before →
  next run scrapes that account

  Google Sheets
  11. Linktree links to docs.google.com/spreadsheets/d/XYZ/edit → classified as
  GoogleDoc → normalized → community resource tracker spreadsheet becomes a source

  PDF
  12. Org website links to
  example.org/know-your-rights-guide.pdf?utm_source=linktree → classified as Pdf →
  tracking params stripped → promoted → next run fetches PDF content

  Change.org Petition
  13. Link to change.org/p/stop-rent-increases-in-mpls → classified as Petition →
  promoted as Tension source → scrape reveals petition text describing community
  tension

  Volume Control
  14. A page with 50+ outbound links → max_per_source: 20 cap kicks in → only 20
  links promoted (classified variants should be prioritized over generic WebPage)

    - Signal has coordinates → actor gets the signal's exact location + location_name
  - Signal has no coordinates → actor gets the region center (e.g. Minneapolis city
  center) + region name as a rough starting point

  --

  A mutual aid org posts on Instagram about a free food distribution. The org
  ("Northside Mutual Aid") becomes the author actor. The post mentions "Second
  Harvest Heartland" as the food supplier — that's a mentioned actor. Both get
  pinned to the post's location (the church parking lot at 45.01, -93.28).

  A Star Tribune article covers a housing crisis. The author actor is "Star Tribune"
   (the publication). The article mentions "Simpson Housing Services", "Hennepin
  County", and "Minneapolis Public Housing Authority" as involved parties. All four
  actors emerge. The article has no coordinates, so they all get pinned to the
  Minneapolis region center.

  The same org shows up in three different articles over two weeks. First mention
  creates the actor. Second and third mentions reuse it and create new edges. The
  actor's location stays wherever the first signal placed it.

  A Linktree page for a community coalition is scraped. The coalition ("Northeast
  Community Defense") is the author actor. Their Linktree links to five partner orgs
   — the extraction pulls those as mentioned actors. All six actors emerge from one
  page.

  Someone posts a flyer with no byline. No author can be determined — author_actor
  is null. But the flyer mentions "Pillsbury United Communities" as the host — that
  still emerges as a mentioned actor.

  Two orgs have slightly different names across sources. One article says "Simpson
  Housing" and another says "Simpson Housing Services." These create two separate
  actors. (This is a known gap — fuzzy matching is a future concern, not solved
  today.)

  A page is scraped that mentions an org already known from a previous run. The org
  already has a precise location from its own website. The new signal's location (or
   region fallback) does NOT overwrite the existing actor's location.

  Bootstrap runs for a brand new region (Portland, OR). Linktree queries
  (site:linktr.ee mutual aid Portland) appear as regular sources alongside GoFundMe
  and Eventbrite queries. When scraped next run, any orgs on those Linktree pages
  emerge as actors automatically.

  A government meeting agenda PDF is scraped. No clear single author. But it
  mentions "Portland Bureau of Transportation", "Multnomah County Health Dept", and
  three neighborhood associations. All five emerge as actors typed Organization,
  pinned to the PDF's extracted location or the Portland region center.

  An anonymous Reddit post describes a neighbor dispute. No author actor
  (anonymous). No mentioned actors (no orgs named). Zero actors emerge — and that's
  correct. Not every signal produces actors.


  -- 


  These test additional extract_links / strip_tracking_params scenarios not covered
  by the existing 9 tests:

  - www prefix dedup — https://www.example.com/page and https://example.com/page
  should dedup (they don't today — canonical_value preserves www)
  - Fragment stripping — https://example.com/page#section should dedup with
  https://example.com/page (today fragments are preserved)
  - Self-referential links — a link back to the source page should be excluded or at
   least dedup
  - Trailing slash normalization — https://example.com/page vs
  https://example.com/page/
  - Google Docs format variants — /edit, /view, /pub should dedup (they don't today)
  - URL shorteners pass through — https://bit.ly/abc is accepted (correct behavior,
  downstream scraping handles it)
  - Redirect wrapper URLs — https://l.instagram.com/?u=... passes through as-is
  (correct for now)
  - Case sensitivity — https://Example.COM/Page vs https://example.com/page
  - GoFundMe with platform params — non-tracking params are preserved correctly
  - Empty/whitespace links — gracefully skipped
  - Ports — standard ports (443) vs non-standard (8080)

  2. Source Location Bug (TDD — tests that SHOULD fail today)

  The bug: every promoted source gets the discovering region's center coordinates,
  regardless of where the linked content actually is. These tests expose this by
  construction:

  - Cross-region org on Linktree — Minneapolis scout finds a Texas org's GoFundMe
  linked from a Minneapolis Linktree. The source gets Minneapolis coords (wrong —
  it's in Texas).
  - National org website — A link to a national nonprofit (e.g.,
  https://mutualaid.org) from a local page gets tagged as local.
  - Multi-region actor — An actor operates in both Minneapolis and Chicago. Their
  Chicago GoFundMe gets Minneapolis coords when discovered by the Minneapolis scout.
  - Source loaded by wrong region — A source tagged at Minneapolis center (44.97,
  -93.27) gets loaded by a scout running for St. Paul (44.95, -93.09) because the
  1.5x bounding box overlaps. The source then produces signals geotagged to St.
  Paul's center.
  - Cascading contamination — Source A (tagged Minneapolis) links to Source B
  (tagged Minneapolis), but Source B is actually an Atlanta org. When Atlanta scout
  runs, it never finds Source B because the coords are wrong.

  These tests won't involve the full graph — they'll test the promote_links function
   signature and the SourceNode it produces, asserting that coords should NOT
  blindly inherit from region center.

  Want me to write these tests into the link_promoter.rs test module? The location
  bug tests would be written as #[test] functions that assert the desired behavior
  (and would fail today, giving you the TDD red phase).

-- need to make sure that published_at is included in posts
-- published_at is ESENTIAL since it's used as signal and signal is treated as fresh
-- if no signal, then what? Don't show. put towards end. 
-- test date extraction logic aggressively

- need to test experiental vs political
- extensive tests testing what could go wrong - messy content
- ensure that locations are ACTUALLY correct. Make sure that mentions of locations are specific to post