---
date: 2026-02-17
topic: radio-as-civic-signal-source
---

# Radio as a Civic Signal Source

## What We're Building

Adding community and public radio stations as curated web sources for civic signal extraction. Many local radio stations publish articles, show notes, and news coverage on their websites — this is crawlable text content that fits directly into the existing web scraping pipeline with no new infrastructure.

This is not about capturing live audio streams or transcribing broadcasts. It's about recognizing that community radio stations are among the best civic journalism outlets in a city, and their websites are an untapped source of hyperlocal signal.

## Why Radio Stations Matter

Community radio stations occupy a unique position in the civic information ecosystem:

- **Hyperlocal focus**: Community stations like KFAI (Twin Cities) or KBOO (Portland) cover neighborhood-level civic issues that mainstream media ignores.
- **Grassroots voice**: They amplify mutual aid networks, immigrant communities, tenant organizing, and environmental stewardship — exactly the signal types Root Signal tracks.
- **Multilingual coverage**: Stations like KRSM (Twin Cities) broadcast in 6 languages, reaching communities invisible to English-only web sources.
- **Civic depth**: Public radio newsrooms (MPR, OPB, WNYC) produce detailed city council coverage, housing policy analysis, and investigative civic reporting with article archives.

## Approaches Considered

### Approach A: Add radio station websites as curated sources (chosen)

Radio station websites added to `curated_sources` in each city's `CityProfile`. These are plain `Web` source type — no new `SourceType` variant needed. Optionally add `site:` Tavily queries to surface their content via search.

**Pros:** Zero infrastructure change. Fits existing pipeline perfectly. Immediate value.
**Cons:** Only captures what stations publish as text on their websites, not audio content.

### Approach B: Podcast/RSS transcription pipeline

Fetch RSS feeds, download audio, transcribe via Whisper API, then feed transcripts through LLM extraction. New pipeline step and transcription dependency.

**Best when:** Text-based content proves insufficient and audio-only shows contain unique civic signal not published anywhere as text.

### Approach C: Live radio stream capture

Real-time audio stream monitoring with chunked transcription. Significant infrastructure.

**Best when:** Never, probably. Civic signals aren't time-critical enough to justify real-time radio monitoring.

**Decision:** Start with Approach A. Graduate to B only if text content proves insufficient.

## Proposed Sources

### Twin Cities (5 additions)

| Station | URL | Type | Why |
|---|---|---|---|
| KFAI 90.3 FM | `https://kfai.org/` | Community radio | Volunteer-driven, MinneCulture documentaries, underrepresented voices |
| KRSM 98.9 FM | `https://krsmradio.org/` | Community radio | 6 languages (English, Spanish, Somali, Ojibwe, Hmong, Haitian Creole), Phillips neighborhood |
| KMOJ 89.9 FM | `https://kmojfm.com/wp/` | Community radio | "The People's Station", African-American community, North Minneapolis civic engagement |
| North News | `https://mynorthnews.org/` | Community journalism | Immigration/ICE coverage, public safety, North Minneapolis hyperlocal, youth voices |
| WFNU | `https://wfnu.org/` | Community radio | Frogtown/St. Paul, immigrant communities |

### NYC (4 additions)

| Station | URL | Type | Why |
|---|---|---|---|
| WNYC | `https://www.wnyc.org/` | Public radio | Flagship NYC public radio, city council, housing, immigration coverage |
| Gothamist | `https://gothamist.com/news` | WNYC news arm | Deep civic reporting, housing crisis, social services, structured articles |
| WBAI 99.5 FM | `https://www.wbai.org/` | Community radio (Pacifica) | Labor rights, immigration enforcement, City Hall reporting |
| WHCR 90.3 FM | `https://whcr.org/` | Community radio | "Voice of Harlem", community affairs, housing, health |

### Portland (3 additions)

| Station | URL | Type | Why |
|---|---|---|---|
| OPB | `https://www.opb.org/` | Public radio (NPR) | Oregon's flagship, city council, housing, environment, transcripts available |
| KBOO 90.7 FM | `https://kboo.fm/` | Community radio | Social justice, immigration, housing/homelessness, mutual aid since 1968 |
| XRAY.fm | `https://xray.fm/` | Community radio | East Portland community news, local events |

### Berlin (4 additions)

| Station | URL | Type | Why |
|---|---|---|---|
| rbb24 | `https://www.rbb24.de/` | Public broadcaster | Major Berlin/Brandenburg news, neighborhood-level civic coverage |
| Radio Spaetkauf | `https://www.radiospaetkauf.com/` | Community (English) | Housing policy, civic infrastructure, detailed show notes/archives |
| THF Radio | `https://www.thfradio.de/en` | Community radio | Housing conflicts, refugee/migration issues, investigative podcasts |
| Common Ground Berlin | `https://commongroundberlin.com/` | Public radio (English) | English-language civic podcast, integration topics, town hall events |

## Entity Mappings to Add

For Twin Cities (where entity mappings are most developed), these stations should be mapped:

- `kfai.org` → entity_id `kfai.org`, type `org`
- `krsmradio.org` → entity_id `krsmradio.org`, type `org`
- `kmojfm.com` → entity_id `kmojfm.com`, type `org`
- `mynorthnews.org` → entity_id `mynorthnews.org`, type `org`

This prevents the same station's web content and social media from inflating `source_diversity`.

## Open Questions

- **Tavily queries**: Should we add `site:` queries for these stations (e.g., `"site:gothamist.com NYC volunteer community"`)? This would surface their content even when it's not on the homepage. Probably yes for the larger newsrooms (OPB, WNYC, Gothamist, rbb24).
- **Crawl depth**: Some stations have deep archives. Should we limit to recent content, or let the freshness scoring handle it naturally? Freshness scoring should be sufficient.
- **Berlin language**: rbb24 is German-only. The LLM extraction pipeline handles German fine, but worth confirming extraction quality on a sample.

## Next Steps

→ Add curated sources and entity mappings to `sources.rs`
→ Optionally add targeted Tavily queries for larger newsrooms
→ Run a scout cycle and check extraction quality from these new sources
