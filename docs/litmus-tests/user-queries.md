# User Query Litmus Tests

Real questions a person might ask Root Signal, tested against live Memgraph data on 2026-02-17.

## How to use this file

Every query below represents something a real user would type into a search bar. If Root Signal can't answer it, that's a gap. Run these periodically after changes to the scout, extractor, or query layer to make sure nothing regresses.

---

## Immigration & ICE

| Status | Question |
|--------|----------|
| WORKS | What's happening with ICE in my area? |
| WORKS | Where can I find know-your-rights resources? |
| WORKS | Who needs volunteer witnesses? |
| WORKS | Are there any legal clinics coming up? |
| WORKS | What are people doing to protect immigrants? |

**Test query:**
```cypher
MATCH (n)
WHERE n.title CONTAINS 'ICE'
   OR n.title CONTAINS 'immigration'
   OR n.title CONTAINS 'immigrant'
   OR n.summary CONTAINS 'immigration'
RETURN labels(n)[0] AS type, n.title AS title
LIMIT 15;
```
**Result:** 15 results across Events, Asks, Gives, Notices. Strong coverage.

---

## Food & Hunger

| Status | Question |
|--------|----------|
| WORKS | Where can I get free food? |
| WORKS | Who's running a food drive? |
| WORKS | What pantries are near me? |
| WORKS | Who needs food donations? |
| WORKS | Are kids covered for meals over the break? |

**Test query:**
```cypher
MATCH (n)
WHERE toLower(n.title) CONTAINS 'food'
   OR toLower(n.title) CONTAINS 'meal'
   OR toLower(n.title) CONTAINS 'pantry'
   OR toLower(n.title) CONTAINS 'hunger'
   OR toLower(n.title) CONTAINS 'grocery'
RETURN labels(n)[0] AS type, n.title AS title
LIMIT 15;
```
**Result:** 15 results, mostly Gives (food shelves, co-ops, free meals, radiothon). Good coverage.

---

## Housing & Shelter

| Status | Question |
|--------|----------|
| WORKS | Who's facing eviction right now? |
| WORKS | Where can I get help with rent? |
| WORKS | Who's fighting against displacement in my neighborhood? |
| WORKS | Are there any affordable housing openings? |
| WORKS | What shelters are available? |

**Test query:**
```cypher
MATCH (n)
WHERE toLower(n.title) CONTAINS 'housing'
   OR toLower(n.title) CONTAINS 'rent'
   OR toLower(n.title) CONTAINS 'evict'
   OR toLower(n.title) CONTAINS 'shelter'
   OR toLower(n.title) CONTAINS 'homeless'
   OR toLower(n.title) CONTAINS 'unhoused'
RETURN labels(n)[0] AS type, n.title AS title
LIMIT 15;
```
**Result:** 15 results (rent assistance, shelters, eviction prevention, housing justice, affordable housing). Strong coverage.

---

## Volunteering

| Status | Question |
|--------|----------|
| WORKS | Where can I volunteer this week? |
| WORKS | Who needs volunteers? |
| WORKS | What volunteer opportunities are near me? |

**Test query:**
```cypher
MATCH (n)
WHERE toLower(n.title) CONTAINS 'volunteer'
RETURN labels(n)[0] AS type, n.title AS title
LIMIT 15;
```
**Result:** 15 results, mix of Gives (opportunities) and Asks (needs). Good coverage.

---

## Youth & Education

| Status | Question |
|--------|----------|
| WORKS | What after-school programs are available? |
| WORKS | Who needs mentors or tutors? |
| WORKS | What are schools asking for? |
| WORKS | Are there summer programs signing up? |

**Test query:**
```cypher
MATCH (n)
WHERE toLower(n.title) CONTAINS 'youth'
   OR toLower(n.title) CONTAINS 'kids'
   OR toLower(n.title) CONTAINS 'children'
   OR toLower(n.title) CONTAINS 'school'
   OR toLower(n.title) CONTAINS 'tutor'
   OR toLower(n.title) CONTAINS 'mentor'
RETURN labels(n)[0] AS type, n.title AS title
LIMIT 15;
```
**Result:** 15 results (mentoring, employment training, summer care, youth shelters, school events). Good.

---

## Health & Mental Health

| Status | Question |
|--------|----------|
| WORKS | Where can I find free or low-cost healthcare? |
| WORKS | Who's offering mental health support? |
| WORKS | Any health clinics or screening events coming up? |
| WORKS | What's available for people in crisis? |

**Test query:**
```cypher
MATCH (n)
WHERE toLower(n.title) CONTAINS 'health'
   OR toLower(n.title) CONTAINS 'mental'
   OR toLower(n.title) CONTAINS 'clinic'
   OR toLower(n.title) CONTAINS 'therapy'
RETURN labels(n)[0] AS type, n.title AS title
LIMIT 15;
```
**Result:** 14 results (clinics, mental health conversations, health fairs, sliding-scale clinics). Good.

---

## Mutual Aid

| Status | Question |
|--------|----------|
| WORKS | What mutual aid networks are active near me? |
| WORKS | How can I plug in and help? |
| WORKS | What's my neighborhood organizing around? |

**Test query:**
```cypher
MATCH (n)
WHERE toLower(n.title) CONTAINS 'mutual aid'
RETURN labels(n)[0] AS type, n.title AS title
LIMIT 15;
```
**Result:** 15 results, both Asks and Gives. Cross-city (Twin Cities, NYC, Berlin). Strong.

---

## Environment & Parks

| Status | Question |
|--------|----------|
| WORKS | Who's organizing cleanups? |
| WORKS | What's happening with the river/lakes/parks? |
| WORKS | Are there environmental justice issues people are raising? |

**Test query:**
```cypher
MATCH (n)
WHERE toLower(n.title) CONTAINS 'environment'
   OR toLower(n.title) CONTAINS 'cleanup'
   OR toLower(n.title) CONTAINS 'river'
   OR toLower(n.title) CONTAINS 'park'
   OR toLower(n.title) CONTAINS 'garden'
RETURN labels(n)[0] AS type, n.title AS title
LIMIT 15;
```
**Result:** 15 results (river cleanups, Earth Day, gardens, watershed committees, park programs). Good.

---

## Public Safety & Policing

| Status | Question |
|--------|----------|
| THIN | What are people saying about policing in my neighborhood? |
| THIN | Are there any community safety meetings? |
| THIN | Who's organizing around police accountability? |

**Test query:**
```cypher
MATCH (n)
WHERE toLower(n.title) CONTAINS 'police'
   OR toLower(n.title) CONTAINS 'safety'
   OR toLower(n.title) CONTAINS 'policing'
   OR toLower(n.title) CONTAINS 'crime'
RETURN labels(n)[0] AS type, n.title AS title
LIMIT 15;
```
**Result:** Only 2 results. Gap in source coverage — need to add sources that cover this topic.

---

## Geographic ("what's near me")

| Status | Question |
|--------|----------|
| WORKS | What's happening near downtown Minneapolis? |
| WORKS | What resources are in my neighborhood? |

**Test query (downtown Minneapolis bounding box):**
```cypher
MATCH (n)
WHERE n.lat IS NOT NULL
  AND n.lat > 44.9 AND n.lat < 45.0
  AND n.lng > -93.3 AND n.lng < -93.2
RETURN labels(n)[0] AS type, n.title AS title, n.lat AS lat, n.lng AS lng
LIMIT 15;
```
**Result:** 15 results with real coordinates. Works.

---

## Corroboration ("is this real?")

| Status | Question |
|--------|----------|
| WORKS | Is this just one organization promoting themselves, or are multiple people talking about it? |
| WORKS | What needs are backed by multiple independent sources? |
| WORKS | What's getting the most cross-source attention? |

**Test query:**
```cypher
MATCH (n)
WHERE n.source_diversity > 1
RETURN labels(n)[0] AS type, n.title AS title,
       n.source_diversity AS sources, n.corroboration_count AS corr
ORDER BY n.source_diversity DESC
LIMIT 15;
```
**Result:** Top signal has source_diversity of 10. Can clearly distinguish single-org promotion from genuinely cross-sourced needs.

---

## Matching Needs to Resources

| Status | Question |
|--------|----------|
| PARTIAL | I need food — is anyone offering it? |
| PARTIAL | Someone's asking for winter coats — is anyone giving them away? |

**Test query (food example):**
```cypher
MATCH (a:Ask)
WHERE toLower(a.title) CONTAINS 'food' OR toLower(a.summary) CONTAINS 'food'
WITH a
MATCH (g:Give)
WHERE toLower(g.title) CONTAINS 'food' OR toLower(g.summary) CONTAINS 'food'
RETURN a.title AS need, g.title AS resource
LIMIT 10;
```
**Result:** Returns results but as a cartesian product (every need paired with every resource). No smart matching — needs semantic similarity between Ask and Give embeddings.

---

## Time-Based ("this week", "upcoming")

| Status | Question |
|--------|----------|
| FIXED | What events are happening this week? |
| FIXED | What's coming up this weekend? |
| FIXED | Anything happening Saturday? |

**Test query:**
```cypher
MATCH (n:Event)
WHERE n.starts_at IS NOT NULL
RETURN n.title AS title, n.starts_at AS date
ORDER BY n.starts_at
LIMIT 15;
```
**Result:** After backfill migration: empty strings → null, scrape-timestamp dates → null, remaining string dates → proper ZONED_DATE_TIME. ORDER BY no longer crashes on mixed types. Events with real dates are now sortable. Events without extracted dates have starts_at = null (correctly excluded from time-based queries).

---

## Actor Queries ("who's doing what")

| Status | Question |
|--------|----------|
| WORKS | What organizations are active in my neighborhood? |
| WORKS | Who's organizing around housing? |
| WORKS | Who's doing the most in my community right now? |

**Test query:**
```cypher
MATCH (a:Actor)-[:ACTED_IN]->(n)
RETURN a.name AS who, labels(n)[0] AS type, n.title AS what
LIMIT 15;
```
**Result:** 78 edges. Actors are linked to signals via ACTED_IN with role properties. Works. (Previous test used wrong edge name `PARTICIPATED_IN`.)

---

## Stories / Narratives

| Status | Question |
|--------|----------|
| PARTIAL | What are the big stories right now? |
| PARTIAL | What issues keep coming up? |

**Test query:**
```cypher
MATCH (s:Story)-[:CONTAINS]->(n)
RETURN s.id AS story, labels(n)[0] AS type, n.title AS title
LIMIT 20;
```
**Result:** Only 2 stories exist, and one is a grab-bag of unrelated signals. Clustering needs more data or tuning.

---

## Summary

| Category | Status | Notes |
|----------|--------|-------|
| Immigration/ICE | WORKS | Strong coverage, all signal types |
| Food/hunger | WORKS | Mostly Gives |
| Housing/shelter | WORKS | Strong |
| Volunteering | WORKS | Strong |
| Youth/education | WORKS | Good |
| Health/mental health | WORKS | Good |
| Mutual aid | WORKS | Cross-city |
| Environment/parks | WORKS | Good |
| Public safety/policing | THIN | Only 2 results — source gap |
| Geographic | WORKS | Bounding box queries work |
| Corroboration | WORKS | source_diversity ranking works |
| Need-to-resource matching | PARTIAL | Cartesian product, no semantic match |
| Time-based | BROKEN | starts_at is scrape time, not event time |
| Actor queries | BROKEN | PARTICIPATED_IN edges missing |
| Stories/narratives | PARTIAL | Only 2 stories, clustering needs tuning |
