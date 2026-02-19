//! Graph structural audits extracted from docs/tests/*.md playbooks.
//!
//! Each check runs a Cypher query and returns a `CheckResult`.
//! `run_audit()` runs all applicable checks for a given configuration.

use regex::Regex;
use rootsignal_graph::{query, GraphClient};

/// Result of a single structural check.
#[derive(Debug)]
pub struct CheckResult {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
    pub value: Option<f64>,
}

/// Aggregated audit report.
#[derive(Debug)]
pub struct AuditReport {
    pub checks: Vec<CheckResult>,
    pub passed: usize,
    pub failed: usize,
}

/// City-specific parameters for audit checks.
pub struct AuditConfig {
    pub min_signals: usize,
    pub min_types: usize,
    pub center_lat: f64,
    pub center_lng: f64,
    pub radius_km: f64,
    pub geo_accuracy_pct: f64,
    pub max_cluster_size: usize,
    pub max_geo_bucket_pct: f64,
}

impl AuditConfig {
    /// Relaxed config suitable for sim tests (small worlds with few signals).
    pub fn for_sim(world: &simweb::World) -> Self {
        Self {
            min_signals: 1,
            min_types: 1,
            center_lat: world.geography.center_lat,
            center_lng: world.geography.center_lng,
            radius_km: 50.0,
            geo_accuracy_pct: 0.5,
            max_cluster_size: 30,
            max_geo_bucket_pct: 0.5,
        }
    }
}

// ---------------------------------------------------------------------------
// Individual checks
// ---------------------------------------------------------------------------

/// At least `min` signals exist in the graph.
pub async fn check_signal_count(client: &GraphClient, min: usize) -> CheckResult {
    let cypher = "MATCH (n) WHERE n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension \
                  RETURN count(n) AS cnt";
    let q = query(cypher);
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let count: i64 = stream
        .next()
        .await
        .expect("row failed")
        .map(|r| r.get::<i64>("cnt").unwrap_or(0))
        .unwrap_or(0);

    CheckResult {
        name: "signal_count",
        passed: count as usize >= min,
        detail: format!("{count} signals (min: {min})"),
        value: Some(count as f64),
    }
}

/// Distinct signal types (labels) >= min_types.
pub async fn check_type_diversity(client: &GraphClient, min_types: usize) -> CheckResult {
    let cypher = "MATCH (n) WHERE n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension \
                  WITH labels(n)[0] AS lbl \
                  RETURN collect(DISTINCT lbl) AS types";
    let q = query(cypher);
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let types: Vec<String> = stream
        .next()
        .await
        .expect("row failed")
        .map(|r| r.get::<Vec<String>>("types").unwrap_or_default())
        .unwrap_or_default();

    let count = types.len();
    CheckResult {
        name: "type_diversity",
        passed: count >= min_types,
        detail: format!("{count} types: {} (min: {min_types})", types.join(", ")),
        value: Some(count as f64),
    }
}

/// No email or SSN patterns in signal titles.
pub async fn check_no_pii(client: &GraphClient) -> CheckResult {
    let cypher = "MATCH (n) WHERE n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension \
                  RETURN n.title AS title";
    let q = query(cypher);
    let mut stream = client.inner().execute(q).await.expect("query failed");

    let email_re = Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap();
    let ssn_re = Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap();

    let mut violations = vec![];
    while let Some(row) = stream.next().await.expect("row failed") {
        let title: String = row.get("title").unwrap_or_default();
        if email_re.is_match(&title) || ssn_re.is_match(&title) {
            violations.push(title);
        }
    }

    CheckResult {
        name: "no_pii",
        passed: violations.is_empty(),
        detail: if violations.is_empty() {
            "No PII patterns found".into()
        } else {
            format!(
                "{} titles with PII: {}",
                violations.len(),
                violations.join("; ")
            )
        },
        value: Some(violations.len() as f64),
    }
}

/// No signals with identical toLower(title) + type.
pub async fn check_no_exact_duplicates(client: &GraphClient) -> CheckResult {
    let cypher = "MATCH (n) WHERE n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension \
                  WITH toLower(n.title) AS title, labels(n)[0] AS type, count(*) AS c \
                  WHERE c > 1 RETURN title, type, c";
    let q = query(cypher);
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut dupes = vec![];
    while let Some(row) = stream.next().await.expect("row failed") {
        dupes.push(format!(
            "{}: {} x{}",
            row.get::<String>("type").unwrap_or_default(),
            row.get::<String>("title").unwrap_or_default(),
            row.get::<i64>("c").unwrap_or(0),
        ));
    }

    CheckResult {
        name: "no_exact_duplicates",
        passed: dupes.is_empty(),
        detail: if dupes.is_empty() {
            "No duplicates".into()
        } else {
            format!("{} dupes: {}", dupes.len(), dupes.join(", "))
        },
        value: Some(dupes.len() as f64),
    }
}

/// At least `min_pct` of geolocated signals fall within `radius_km` of center.
pub async fn check_geo_accuracy(
    client: &GraphClient,
    center_lat: f64,
    center_lng: f64,
    radius_km: f64,
    min_pct: f64,
) -> CheckResult {
    let cypher = "MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension) \
                  AND n.lat IS NOT NULL AND n.lng IS NOT NULL \
                  RETURN n.lat AS lat, n.lng AS lng";
    let q = query(cypher);
    let mut stream = client.inner().execute(q).await.expect("query failed");

    let mut total = 0usize;
    let mut within = 0usize;
    while let Some(row) = stream.next().await.expect("row failed") {
        let lat = row.get::<f64>("lat").unwrap_or(0.0);
        let lng = row.get::<f64>("lng").unwrap_or(0.0);
        total += 1;
        if haversine_km(center_lat, center_lng, lat, lng) <= radius_km {
            within += 1;
        }
    }

    let pct = if total > 0 {
        within as f64 / total as f64
    } else {
        1.0 // no geolocated signals — vacuously true
    };

    CheckResult {
        name: "geo_accuracy",
        passed: pct >= min_pct,
        detail: format!(
            "{within}/{total} within {radius_km}km ({:.0}%, min: {:.0}%)",
            pct * 100.0,
            min_pct * 100.0
        ),
        value: Some(pct),
    }
}

/// No signals without SOURCED_FROM→Evidence.
pub async fn check_evidence_trails(client: &GraphClient) -> CheckResult {
    let cypher = "MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension) \
                  AND NOT (n)-[:SOURCED_FROM]->(:Evidence) \
                  RETURN n.title AS title, labels(n)[0] AS type";
    let q = query(cypher);
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut orphans = vec![];
    while let Some(row) = stream.next().await.expect("row failed") {
        orphans.push(format!(
            "{}: {}",
            row.get::<String>("type").unwrap_or_default(),
            row.get::<String>("title").unwrap_or_default(),
        ));
    }

    CheckResult {
        name: "evidence_trails",
        passed: orphans.is_empty(),
        detail: if orphans.is_empty() {
            "All signals have evidence".into()
        } else {
            format!(
                "{} signals without evidence: {}",
                orphans.len(),
                orphans.join("; ")
            )
        },
        value: Some(orphans.len() as f64),
    }
}

/// No single 0.01° lat/lng bucket has > max_bucket_pct of geolocated signals.
pub async fn check_no_geo_clustering(client: &GraphClient, max_bucket_pct: f64) -> CheckResult {
    let cypher = "MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension) \
                  AND n.lat IS NOT NULL AND n.lng IS NOT NULL \
                  WITH round(n.lat * 100) / 100 AS blat, round(n.lng * 100) / 100 AS blng, \
                  count(*) AS c \
                  RETURN blat, blng, c ORDER BY c DESC LIMIT 1";
    let q = query(cypher);
    let mut stream = client.inner().execute(q).await.expect("query failed");

    // Also need the total count
    let total_cypher = "MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension) \
                        AND n.lat IS NOT NULL AND n.lng IS NOT NULL \
                        RETURN count(n) AS cnt";
    let tq = query(total_cypher);
    let mut tstream = client.inner().execute(tq).await.expect("query failed");
    let total: i64 = tstream
        .next()
        .await
        .expect("row failed")
        .map(|r| r.get::<i64>("cnt").unwrap_or(0))
        .unwrap_or(0);

    let biggest: i64 = stream
        .next()
        .await
        .expect("row failed")
        .map(|r| r.get::<i64>("c").unwrap_or(0))
        .unwrap_or(0);

    let pct = if total > 0 {
        biggest as f64 / total as f64
    } else {
        0.0
    };

    CheckResult {
        name: "no_geo_clustering",
        passed: pct <= max_bucket_pct,
        detail: format!(
            "biggest bucket: {biggest}/{total} ({:.0}%, max: {:.0}%)",
            pct * 100.0,
            max_bucket_pct * 100.0
        ),
        value: Some(pct),
    }
}

/// No story has more than `max_size` signals.
pub async fn check_no_mega_clusters(client: &GraphClient, max_size: usize) -> CheckResult {
    let cypher = "MATCH (s:Story)<-[:PART_OF]-(sig) \
                  WITH s.headline AS headline, count(sig) AS c \
                  WHERE c > $max \
                  RETURN headline, c";
    let q = query(cypher).param("max", max_size as i64);
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut megas = vec![];
    while let Some(row) = stream.next().await.expect("row failed") {
        megas.push(format!(
            "{}: {} signals",
            row.get::<String>("headline").unwrap_or_default(),
            row.get::<i64>("c").unwrap_or(0),
        ));
    }

    CheckResult {
        name: "no_mega_clusters",
        passed: megas.is_empty(),
        detail: if megas.is_empty() {
            format!("No stories exceed {max_size} signals")
        } else {
            format!("{} mega-clusters: {}", megas.len(), megas.join("; "))
        },
        value: Some(megas.len() as f64),
    }
}

/// All Evidence nodes are linked via SOURCED_FROM from at least one signal.
pub async fn check_no_orphaned_evidence(client: &GraphClient) -> CheckResult {
    let cypher = "MATCH (ev:Evidence) \
                  WHERE NOT ()-[:SOURCED_FROM]->(ev) \
                  RETURN ev.source_url AS url";
    let q = query(cypher);
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut orphans = vec![];
    while let Some(row) = stream.next().await.expect("row failed") {
        orphans.push(row.get::<String>("url").unwrap_or_default());
    }

    CheckResult {
        name: "no_orphaned_evidence",
        passed: orphans.is_empty(),
        detail: if orphans.is_empty() {
            "No orphaned evidence".into()
        } else {
            format!(
                "{} orphaned evidence nodes: {}",
                orphans.len(),
                orphans.join("; ")
            )
        },
        value: Some(orphans.len() as f64),
    }
}

/// No signals with empty or null titles.
pub async fn check_no_empty_signals(client: &GraphClient) -> CheckResult {
    let cypher = "MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension) \
                  AND (n.title IS NULL OR trim(n.title) = '') \
                  RETURN labels(n)[0] AS type, n.id AS id";
    let q = query(cypher);
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut empties = vec![];
    while let Some(row) = stream.next().await.expect("row failed") {
        empties.push(format!(
            "{}: {}",
            row.get::<String>("type").unwrap_or_default(),
            row.get::<String>("id").unwrap_or_default(),
        ));
    }

    CheckResult {
        name: "no_empty_signals",
        passed: empties.is_empty(),
        detail: if empties.is_empty() {
            "No empty signal titles".into()
        } else {
            format!(
                "{} signals with empty titles: {}",
                empties.len(),
                empties.join("; ")
            )
        },
        value: Some(empties.len() as f64),
    }
}

/// No signals at (0,0) or exact city-center coordinates (likely fake).
pub async fn check_no_fake_coordinates(
    client: &GraphClient,
    center_lat: f64,
    center_lng: f64,
) -> CheckResult {
    let cypher = "MATCH (n) WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension) \
                  AND n.lat IS NOT NULL AND n.lng IS NOT NULL \
                  RETURN n.title AS title, n.lat AS lat, n.lng AS lng";
    let q = query(cypher);
    let mut stream = client.inner().execute(q).await.expect("query failed");

    let mut fakes = vec![];
    while let Some(row) = stream.next().await.expect("row failed") {
        let lat = row.get::<f64>("lat").unwrap_or(0.0);
        let lng = row.get::<f64>("lng").unwrap_or(0.0);
        let title: String = row.get("title").unwrap_or_default();

        // (0,0) is almost certainly fake
        let is_null_island = lat.abs() < 0.001 && lng.abs() < 0.001;
        // Exact city center (4+ decimal places matching) is suspicious
        let is_exact_center =
            (lat - center_lat).abs() < 0.0001 && (lng - center_lng).abs() < 0.0001;

        if is_null_island || is_exact_center {
            fakes.push(format!("{title} ({lat:.4}, {lng:.4})"));
        }
    }

    CheckResult {
        name: "no_fake_coordinates",
        passed: fakes.is_empty(),
        detail: if fakes.is_empty() {
            "No fake coordinates detected".into()
        } else {
            format!("{} suspicious coords: {}", fakes.len(), fakes.join("; "))
        },
        value: Some(fakes.len() as f64),
    }
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

/// Run all applicable audit checks and return an aggregated report.
pub async fn run_audit(client: &GraphClient, config: &AuditConfig) -> AuditReport {
    let checks = vec![
        check_signal_count(client, config.min_signals).await,
        check_type_diversity(client, config.min_types).await,
        check_no_pii(client).await,
        check_no_exact_duplicates(client).await,
        check_geo_accuracy(
            client,
            config.center_lat,
            config.center_lng,
            config.radius_km,
            config.geo_accuracy_pct,
        )
        .await,
        check_evidence_trails(client).await,
        check_no_geo_clustering(client, config.max_geo_bucket_pct).await,
        check_no_mega_clusters(client, config.max_cluster_size).await,
        check_no_orphaned_evidence(client).await,
        check_no_empty_signals(client).await,
        check_no_fake_coordinates(client, config.center_lat, config.center_lng).await,
    ];

    let passed = checks.iter().filter(|c| c.passed).count();
    let failed = checks.len() - passed;

    AuditReport {
        checks,
        passed,
        failed,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Haversine distance in km between two lat/lng points.
fn haversine_km(lat1: f64, lng1: f64, lat2: f64, lng2: f64) -> f64 {
    let r = 6371.0; // Earth radius in km
    let dlat = (lat2 - lat1).to_radians();
    let dlng = (lng2 - lng1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlng / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    r * c
}
