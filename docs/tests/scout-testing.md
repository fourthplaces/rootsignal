# Scout Testing Playbook

Instructions for testing the scout pipeline. Follow these in order.

## Prerequisites

```bash
# Services must be running
docker compose up -d memgraph web

# Verify
docker compose ps  # memgraph: healthy, web: running
```

Required env vars in `.env` or shell: `ANTHROPIC_API_KEY`, `VOYAGE_API_KEY`, `TAVILY_API_KEY`, `APIFY_API_KEY`.

## 1. Build and Run Scout

```bash
docker compose build scout
docker compose run --rm scout 2>&1 | tee /tmp/scout-run.log
```

The run takes 10-20 minutes. Most time is spent on Apify social media scraping.

Expected final output format:
```
=== Scout Run Complete ===
URLs scraped:       70+
URLs unchanged:     0-120 (depends on prior runs)
URLs failed:        <15
Social media posts: 300+
Geo stripped:       0+ (fake city-center coords removed)
Signals extracted:  200+
Signals deduped:    varies
Signals stored:     varies (high on first run, low on re-runs)
```

**Red flags in the output:**
- `URLs failed` > 20% of total — scraping infrastructure problem
- `Signals extracted: 0` — LLM extraction is broken
- `Signals stored: 0` on a first run — dedup or graph write is broken
- Any panic or stack trace — code bug

## 2. Run Validation Script

```bash
./scripts/validate-city-run.sh twincities 44.9778 -93.2650
```

**All 8 checks must pass:**

| Check | Gate | What it catches |
|-------|------|-----------------|
| Signal count >= 50 | Hard fail | Scraping or extraction totally broken |
| Signal types >= 3 | Hard fail | LLM classifying everything as one type |
| Audience roles >= 3 | Hard fail | LLM not assigning roles |
| Exact duplicates = 0 | Hard fail | Dedup layers broken |
| Geo accuracy >= 80% | Hard fail | Coordinates outside metro area |
| Email in titles = 0 | Hard fail | Private PII leaking through |
| SSN patterns = 0 | Hard fail | Private PII leaking through |
| Evidence trail = 0 missing | Hard fail | Evidence linking broken |

If any check fails, investigate before proceeding. Do not just re-run and hope.

## 3. Spot-Check Signal Quality

Pull 5 random signals of each type and read them. Are they real signals?

```bash
MG="docker exec -i rootsignal-memgraph-1 mgconsole --username memgraph --password rootsignal"

# Events — should be real upcoming gatherings with dates
echo "MATCH (n:Event) RETURN n.title, n.summary, n.starts_at, n.action_url, n.source_url ORDER BY rand() LIMIT 5;" | $MG

# Gives — should be available resources (services, programs, free things)
echo "MATCH (n:Give) RETURN n.title, n.summary, n.action_url, n.source_url ORDER BY rand() LIMIT 5;" | $MG

# Asks — should be real requests for help (volunteers, donations, advocacy)
echo "MATCH (n:Ask) RETURN n.title, n.summary, n.action_url, n.source_url ORDER BY rand() LIMIT 5;" | $MG

# Notices — should be official advisories/policy changes, NOT general news
echo "MATCH (n:Notice) RETURN n.title, n.summary, n.source_url ORDER BY rand() LIMIT 5;" | $MG
```

**What to look for:**
- Titles should be specific and actionable, not vague ("Community Resources" is bad, "Free Tax Prep at East Side Library — Feb 22" is good)
- Summaries should add context beyond the title
- `action_url` should be a real URL (not empty, not the source_url repeated) for Events/Gives/Asks
- Notices should reference official sources, not just news articles
- No junk signals (ads, navigation text, boilerplate)

**What is NOT a problem:**
- Organization phone numbers and emails in summaries — these are public broadcast info
- Signals mentioning sensitive topics (immigration, housing) — the system should surface community responses to crises
- `lat`/`lng` being Null — honest about unknown location is correct

## 4. Geo Honesty Check

The system must not fake location precision. This is a regression test for the city-center echo bug.

```bash
# Check for clustering: no single 0.01-degree bucket should have >10% of geolocated signals
echo "MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice) AND n.lat IS NOT NULL
WITH count(n) AS total
MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice) AND n.lat IS NOT NULL
WITH total, round(n.lat * 100) / 100 AS lat_b, round(n.lng * 100) / 100 AS lng_b, count(n) AS cnt
RETURN lat_b, lng_b, cnt, round(cnt * 100.0 / total) AS pct ORDER BY cnt DESC LIMIT 5;" | $MG
```

**Pass criteria:**
- No single coordinate bucket has more than 10% of signals
- Signals with coordinates should be at real places (buildings, parks, venues)
- It is fine for most signals to have `lat: Null` — that's honest

**Fail criteria:**
- 50%+ of signals clustered at one point — the LLM is echoing default coords
- City center coordinates (44.9778, -93.265) appearing in bulk — fake-center stripping is broken

## 5. Dedup Verification

The system has 3 dedup layers. Test that all work.

```bash
# Layer 1: No exact title+type duplicates
echo "MATCH (n) WHERE n:Event OR n:Give OR n:Ask OR n:Notice
WITH toLower(n.title) AS t, labels(n)[0] AS type, count(*) AS c
WHERE c > 1 RETURN t, type, c ORDER BY c DESC LIMIT 10;" | $MG

# Should return empty. If not, dedup layer 1 or 2.5 is broken.

# Cross-source corroboration: signals seen from multiple sources should have corroboration_count > 0
echo "MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice) AND n.corroboration_count > 0
RETURN labels(n)[0] AS type, n.title, n.corroboration_count AS corroborations
ORDER BY corroborations DESC LIMIT 10;" | $MG

# Should show signals. If empty on a re-run, vector dedup corroboration is broken.
```

## 6. Sensitive Content Test (ICE/Immigration Scenarios)

The system should surface community responses to crises, not suppress them. Run these queries after a Twin Cities scout run.

```bash
# Immigration-related signals should exist
echo "MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice)
AND (toLower(n.title) CONTAINS 'immigration' OR toLower(n.title) CONTAINS 'ice'
     OR toLower(n.title) CONTAINS 'immigrant' OR toLower(n.title) CONTAINS 'know your rights')
RETURN labels(n)[0] AS type, n.title ORDER BY type, n.title;" | $MG
```

**Must find:**
- Know Your Rights resources (Give signals)
- Legal aid hotlines (Give signals)
- Community rallies/events (Event signals)
- Volunteer/donation asks from immigration orgs (Ask signals)

**Must NOT find:**
- News articles about ICE raids (that's narrative, not a signal)
- The crisis itself as a signal (the system extracts responses, not the crisis)

**If immigration signals are missing:**
- Check that the sensitive corroboration gate is NOT filtering them (it was removed — this is a regression test)
- Check that PII scrubbing is NOT stripping org hotline numbers
- Check source coverage: Sahan Journal, MIRAC, ILCM, Unidos MN should be in sources

## 7. Evidence Trail Integrity

Every signal must trace back to a source.

```bash
# Signals without evidence (should be 0)
echo "MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice)
AND NOT (n)-[:SOURCED_FROM]->(:Evidence)
RETURN count(n) AS orphaned_signals;" | $MG

# Evidence with content hashes (should all have them)
echo "MATCH (e:Evidence) WHERE e.content_hash IS NULL OR e.content_hash = ''
RETURN count(e) AS missing_hashes;" | $MG

# Both should return 0.
```

## 8. Web UI Smoke Test

Open `http://localhost:3001` and verify:

- Map loads with markers at real locations (not all clustered at one point)
- Signal cards show title, summary, type badge
- Type filter works (click Event/Give/Ask/Notice tabs)
- Clicking a signal shows detail with source URL
- Purple Notice badges appear alongside blue/green/orange

## After Code Changes

If you changed any of these files, run the full playbook:

| File changed | Why it matters |
|-------------|----------------|
| `extractor.rs` | LLM prompt or signal construction — affects what gets extracted |
| `scout.rs` | Pipeline logic — affects dedup, geo, stats |
| `quality.rs` (scout) | Quality scoring — affects confidence |
| `quality.rs` (common) | Quality constants — affects expiry, thresholds |
| `writer.rs` | Graph writes — affects what gets stored |
| `reader.rs` | Graph reads — affects what the web shows |
| `sources.rs` | Source list — affects coverage |

If you only changed `web/` or `templates.rs`, skip to step 8.

## Quick Re-run (After Prior Run Exists)

On re-runs, most content is unchanged (content hash match). The run is faster but stores fewer new signals. This is expected. Key things to check:

- `URLs unchanged` should be high (100+)
- `Signals deduped` should be high (most signals already exist)
- `Geo stripped` should be low (only new fake-center signals)
- Corroboration counts should increase (re-seen signals get corroborated)
- All 8 validation checks still pass

## Failure Diagnosis

| Symptom | Likely cause |
|---------|-------------|
| 0 signals extracted | Anthropic API key invalid or rate limited |
| All signals same type | LLM prompt regression — check `build_system_prompt()` |
| Massive duplicate count | Dedup threshold too high or content hash broken |
| All coords at one point | LLM echoing default coords — check prompt Location section |
| Immigration signals missing | Sensitive corroboration gate re-introduced or source removed |
| Evidence trail broken | `create_evidence()` not being called after `create_node()` |
| Scout lock error | Previous run killed without cleanup — run: `echo "MATCH (l:ScoutLock) DELETE l;" | docker exec -i rootsignal-memgraph-1 mgconsole --username memgraph --password rootsignal` |
