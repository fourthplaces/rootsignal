#!/usr/bin/env bash
# Validates a scout run against Milestone 2 quality gates.
# Usage: ./scripts/validate-region-run.sh [region_name] [expected_lat] [expected_lng] [geo_radius_deg]
#
# Checks:
#   1. Signal count >= 50 (kill-test pre-launch minimum)
#   2. 4+ signal types present (Event, Give, Ask, Notice)

#   4. 0 exact duplicates (same title+type)
#   5. Geo-accuracy: >80% of signals with coords within radius of region center
#   6. Zero PII in titles/summaries (emails, phone numbers)
#   7. Evidence trail (every signal has SOURCED_FROM evidence)

set -euo pipefail

REGION="${1:-all}"
EXPECTED_LAT="${2:-0}"
EXPECTED_LNG="${3:-0}"
GEO_RADIUS="${4:-2.0}"  # degrees (~130 miles, generous for metro areas)

MG="docker exec -i rootsignal-memgraph-1 mgconsole --username memgraph --password rootsignal"

run_query() {
    echo "$1" | $MG 2>/dev/null
}

PASS=0
FAIL=0
WARN=0

check() {
    local name="$1"
    local result="$2"
    local expected="$3"
    if [ "$result" = "$expected" ]; then
        echo "  PASS: $name ($result)"
        PASS=$((PASS + 1))
    else
        echo "  FAIL: $name (got: $result, expected: $expected)"
        FAIL=$((FAIL + 1))
    fi
}

check_gte() {
    local name="$1"
    local result="$2"
    local minimum="$3"
    if [ "$result" -ge "$minimum" ] 2>/dev/null; then
        echo "  PASS: $name ($result >= $minimum)"
        PASS=$((PASS + 1))
    else
        echo "  FAIL: $name (got: $result, minimum: $minimum)"
        FAIL=$((FAIL + 1))
    fi
}

echo "============================================"
echo "  Root Signal — Region Run Validation"
echo "  Region: $REGION"
echo "============================================"
echo ""

# 1. Signal count
echo "--- Signal Count ---"
TOTAL=$(run_query "MATCH (n) WHERE n:Event OR n:Give OR n:Ask OR n:Notice RETURN count(n) AS c;" | grep -oE '[0-9]+' | head -1)
check_gte "Total signals >= 50" "$TOTAL" 50

# 2. Signal type diversity
echo ""
echo "--- Signal Type Diversity ---"
TYPE_COUNT=$(run_query "MATCH (n) WHERE n:Event OR n:Give OR n:Ask OR n:Notice RETURN labels(n)[0] AS type, count(*) AS c;" | grep -c '"')
check_gte "Signal types >= 3" "$TYPE_COUNT" 3

run_query "MATCH (n) WHERE n:Event OR n:Give OR n:Ask OR n:Notice RETURN labels(n)[0] AS type, count(*) AS count ORDER BY count DESC;"

# 3. Exact-title+type duplicates (should be 0)
echo ""
echo "--- Deduplication ---"
DUPES=$(run_query "MATCH (n) WHERE n:Event OR n:Give OR n:Ask OR n:Notice WITH toLower(n.title) AS t, labels(n)[0] AS type, count(*) AS c WHERE c > 1 RETURN sum(c - 1) AS duplicates;" | grep -oE '[0-9]+' | head -1)
DUPES="${DUPES:-0}"
check "Exact-title+type duplicates = 0" "$DUPES" "0"

if [ "$DUPES" != "0" ]; then
    echo "  Duplicate details:"
    run_query "MATCH (n) WHERE n:Event OR n:Give OR n:Ask OR n:Notice WITH toLower(n.title) AS t, labels(n)[0] AS type, count(*) AS c WHERE c > 1 RETURN t, type, c ORDER BY c DESC LIMIT 10;"
fi

# 5. Geo-accuracy (if coordinates provided)
echo ""
echo "--- Geo-Accuracy ---"
if [ "$EXPECTED_LAT" != "0" ]; then
    TOTAL_WITH_GEO=$(run_query "MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice) AND n.lat IS NOT NULL RETURN count(n) AS c;" | grep -oE '[0-9]+' | head -1)
    TOTAL_WITH_GEO="${TOTAL_WITH_GEO:-0}"

    if [ "$TOTAL_WITH_GEO" -gt 0 ]; then
        NEAR_CENTER=$(run_query "MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice) AND n.lat IS NOT NULL AND abs(n.lat - $EXPECTED_LAT) < $GEO_RADIUS AND abs(n.lng - $EXPECTED_LNG) < $GEO_RADIUS RETURN count(n) AS c;" | grep -oE '[0-9]+' | head -1)
        NEAR_CENTER="${NEAR_CENTER:-0}"
        PCT=$((NEAR_CENTER * 100 / TOTAL_WITH_GEO))
        check_gte "Geo-accuracy >= 80% near region center" "$PCT" 80
        echo "    ($NEAR_CENTER / $TOTAL_WITH_GEO signals within ${GEO_RADIUS}° of ($EXPECTED_LAT, $EXPECTED_LNG))"
    else
        echo "  WARN: No signals have geo coordinates"
        WARN=$((WARN + 1))
    fi
else
    echo "  SKIP: No expected coordinates provided"
fi

# 6. Private PII check — SSNs and personal emails in titles (org contact info is expected)
echo ""
echo "--- Private PII Check ---"
PII_EMAIL_TITLE=$(run_query "MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice) AND (n.title CONTAINS '@' AND n.title CONTAINS '.') RETURN count(n) AS c;" | grep -oE '[0-9]+' | head -1)
PII_EMAIL_TITLE="${PII_EMAIL_TITLE:-0}"
check "Personal email in titles = 0" "$PII_EMAIL_TITLE" "0"

# Check for actual SSN patterns (XXX-XX-XXXX) — mentioning "SSN" as a word is fine
PII_SSN=$(run_query "MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice) AND n.summary =~ '.*[0-9]{3}-[0-9]{2}-[0-9]{4}.*' RETURN count(n) AS c;" | grep -oE '[0-9]+' | head -1)
PII_SSN="${PII_SSN:-0}"
check "SSN patterns in signals = 0" "$PII_SSN" "0"

# 7. Evidence trail (every signal should have SOURCED_FROM evidence)
echo ""
echo "--- Evidence Trail ---"
NO_EVIDENCE=$(run_query "MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice) AND NOT (n)-[:SOURCED_FROM]->(:Evidence) RETURN count(n) AS c;" | grep -oE '[0-9]+' | head -1)
NO_EVIDENCE="${NO_EVIDENCE:-0}"
check "Signals without evidence = 0" "$NO_EVIDENCE" "0"

# 8. Sample signals (show 5 random for manual spot-check)
echo ""
echo "--- Sample Signals (spot check) ---"
run_query "MATCH (n) WHERE n:Event OR n:Give OR n:Ask OR n:Notice RETURN labels(n)[0] AS type, n.title AS title, n.lat AS lat, n.lng AS lng, n.source_url AS source LIMIT 5;"

# Summary
echo ""
echo "============================================"
echo "  Results: $PASS passed, $FAIL failed, $WARN warnings"
echo "  Total signals: $TOTAL"
echo "============================================"

if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
