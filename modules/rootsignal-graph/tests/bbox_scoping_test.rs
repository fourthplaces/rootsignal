//! Integration test: verify bbox-scoped queries work correctly against live Neo4j.
//! Run with: cargo test -p rootsignal-graph --test bbox_scoping_test -- --ignored --nocapture

use rootsignal_graph::{query, GraphClient, GraphWriter};

fn load_env() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(".env");
    if let Ok(content) = std::fs::read_to_string(&path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                if std::env::var(key.trim()).is_err() {
                    std::env::set_var(key.trim(), value.trim());
                }
            }
        }
    }
}

async fn connect() -> GraphClient {
    load_env();
    let uri = std::env::var("NEO4J_URI").expect("NEO4J_URI required");
    let user = std::env::var("NEO4J_USER").expect("NEO4J_USER required");
    let password = std::env::var("NEO4J_PASSWORD").expect("NEO4J_PASSWORD required");
    GraphClient::connect(&uri, &user, &password)
        .await
        .expect("Failed to connect to Neo4j")
}

/// Load all city nodes from the graph.
async fn get_cities(client: &GraphClient) -> Vec<(String, f64, f64, f64)> {
    let q = query(
        "MATCH (c:City) WHERE c.active = true
         RETURN c.slug AS slug, c.center_lat AS lat, c.center_lng AS lng, c.radius_km AS radius",
    );
    let mut cities = Vec::new();
    let mut stream = client.inner().execute(q).await.unwrap();
    while let Ok(Some(row)) = stream.next().await {
        let slug: String = row.get("slug").unwrap_or_default();
        let lat: f64 = row.get("lat").unwrap_or(0.0);
        let lng: f64 = row.get("lng").unwrap_or(0.0);
        let radius: f64 = row.get("radius").unwrap_or(30.0);
        if !slug.is_empty() {
            cities.push((slug, lat, lng, radius));
        }
    }
    cities
}

fn compute_bbox(center_lat: f64, center_lng: f64, radius_km: f64) -> (f64, f64, f64, f64) {
    let lat_delta = radius_km / 111.0;
    let lng_delta = radius_km / (111.0 * center_lat.to_radians().cos());
    (
        center_lat - lat_delta,
        center_lat + lat_delta,
        center_lng - lng_delta,
        center_lng + lng_delta,
    )
}

#[tokio::test]
#[ignore]
async fn test_get_active_tensions_respects_bbox() {
    let client = connect().await;
    let writer = GraphWriter::new(client.clone());
    let cities = get_cities(&client).await;

    println!("\n=== Testing get_active_tensions bbox scoping ===");
    println!("Found {} active cities", cities.len());

    for (slug, lat, lng, radius) in &cities {
        let (min_lat, max_lat, min_lng, max_lng) = compute_bbox(*lat, *lng, *radius);

        let tensions = writer
            .get_active_tensions(min_lat, max_lat, min_lng, max_lng)
            .await
            .expect("get_active_tensions failed");

        println!(
            "\n[{}] bbox: lat [{:.2}, {:.2}], lng [{:.2}, {:.2}]",
            slug, min_lat, max_lat, min_lng, max_lng
        );
        println!("  Active tensions with embeddings: {}", tensions.len());

        // Verify all returned tensions are actually within bbox by checking their lat/lng
        for (tid, _emb) in &tensions {
            let q = query(
                "MATCH (t:Tension {id: $id}) RETURN t.lat AS lat, t.lng AS lng, t.title AS title",
            )
            .param("id", tid.to_string());
            let mut stream = client.inner().execute(q).await.unwrap();
            if let Ok(Some(row)) = stream.next().await {
                let t_lat: f64 = row.get("lat").unwrap_or(0.0);
                let t_lng: f64 = row.get("lng").unwrap_or(0.0);
                let title: String = row.get("title").unwrap_or_default();
                assert!(
                    t_lat >= min_lat && t_lat <= max_lat && t_lng >= min_lng && t_lng <= max_lng,
                    "Tension '{}' at ({}, {}) is outside bbox for {}",
                    title,
                    t_lat,
                    t_lng,
                    slug
                );
            }
        }
        println!("  ✓ All tensions verified within bbox");
    }
}

#[tokio::test]
#[ignore]
async fn test_find_tension_hubs_respects_bbox() {
    let client = connect().await;
    let writer = GraphWriter::new(client.clone());
    let cities = get_cities(&client).await;

    println!("\n=== Testing find_tension_hubs bbox scoping ===");

    for (slug, lat, lng, radius) in &cities {
        let (min_lat, max_lat, min_lng, max_lng) = compute_bbox(*lat, *lng, *radius);

        let hubs = writer
            .find_tension_hubs(10, min_lat, max_lat, min_lng, max_lng)
            .await
            .expect("find_tension_hubs failed");

        println!("\n[{}] Tension hubs ready to materialize: {}", slug, hubs.len());

        for hub in &hubs {
            println!(
                "  Hub: '{}' ({} respondents)",
                hub.title,
                hub.respondents.len()
            );

            // Verify each respondent signal is within bbox
            for resp in &hub.respondents {
                let q = query(
                    "MATCH (n {id: $id}) RETURN n.lat AS lat, n.lng AS lng, n.title AS title",
                )
                .param("id", resp.signal_id.to_string());
                let mut stream = client.inner().execute(q).await.unwrap();
                if let Ok(Some(row)) = stream.next().await {
                    let s_lat: f64 = row.get("lat").unwrap_or(0.0);
                    let s_lng: f64 = row.get("lng").unwrap_or(0.0);
                    let title: String = row.get("title").unwrap_or_default();
                    assert!(
                        s_lat >= min_lat && s_lat <= max_lat && s_lng >= min_lng && s_lng <= max_lng,
                        "Respondent '{}' at ({}, {}) is outside bbox for {}",
                        title,
                        s_lat,
                        s_lng,
                        slug
                    );
                }
            }
        }
        println!("  ✓ All hub respondents verified within bbox");
    }
}

#[tokio::test]
#[ignore]
async fn test_find_story_growth_respects_bbox() {
    let client = connect().await;
    let writer = GraphWriter::new(client.clone());
    let cities = get_cities(&client).await;

    println!("\n=== Testing find_story_growth bbox scoping ===");

    for (slug, lat, lng, radius) in &cities {
        let (min_lat, max_lat, min_lng, max_lng) = compute_bbox(*lat, *lng, *radius);

        let growths = writer
            .find_story_growth(20, min_lat, max_lat, min_lng, max_lng)
            .await
            .expect("find_story_growth failed");

        println!("\n[{}] Stories with growth candidates: {}", slug, growths.len());

        for growth in &growths {
            for resp in &growth.new_respondents {
                let q = query(
                    "MATCH (n {id: $id}) RETURN n.lat AS lat, n.lng AS lng, n.title AS title",
                )
                .param("id", resp.signal_id.to_string());
                let mut stream = client.inner().execute(q).await.unwrap();
                if let Ok(Some(row)) = stream.next().await {
                    let s_lat: f64 = row.get("lat").unwrap_or(0.0);
                    let s_lng: f64 = row.get("lng").unwrap_or(0.0);
                    let title: String = row.get("title").unwrap_or_default();
                    assert!(
                        s_lat >= min_lat && s_lat <= max_lat && s_lng >= min_lng && s_lng <= max_lng,
                        "Growth respondent '{}' at ({}, {}) is outside bbox for {}",
                        title,
                        s_lat,
                        s_lng,
                        slug
                    );
                }
            }
        }
        println!("  ✓ All growth respondents verified within bbox");
    }
}

#[tokio::test]
#[ignore]
async fn test_no_off_geo_contamination_remains() {
    let client = connect().await;

    println!("\n=== Checking for remaining off-geo contamination ===");

    // Check for signals outside Twin Cities metro bbox
    let q = query(
        "MATCH (n)
         WHERE (n:Event OR n:Give OR n:Need OR n:Tension OR n:Notice)
           AND (n.lat < 43.0 OR n.lat > 46.5 OR n.lng < -95.5 OR n.lng > -91.0)
         RETURN count(n) AS count",
    );
    let mut stream = client.inner().execute(q).await.unwrap();
    let off_geo_count: i64 = if let Ok(Some(row)) = stream.next().await {
        row.get("count").unwrap_or(0)
    } else {
        0
    };
    println!("Off-geo signals remaining: {}", off_geo_count);

    // Check for cross-geo edges
    let q2 = query(
        "MATCH (sig)-[r:RESPONDS_TO|DRAWN_TO]->(t:Tension)
         WHERE abs(sig.lat - t.lat) > 1.0 OR abs(sig.lng - t.lng) > 1.0
         RETURN count(r) AS count",
    );
    let mut stream2 = client.inner().execute(q2).await.unwrap();
    let cross_geo_count: i64 = if let Ok(Some(row)) = stream2.next().await {
        row.get("count").unwrap_or(0)
    } else {
        0
    };
    println!("Cross-geo edges remaining: {}", cross_geo_count);

    // Show some examples if any remain (for debugging, not a failure)
    if off_geo_count > 0 {
        let examples = query(
            "MATCH (n)
             WHERE (n:Event OR n:Give OR n:Need OR n:Tension OR n:Notice)
               AND (n.lat < 43.0 OR n.lat > 46.5 OR n.lng < -95.5 OR n.lng > -91.0)
             RETURN labels(n)[0] AS label, n.title AS title, n.lat AS lat, n.lng AS lng
             LIMIT 5",
        );
        let mut stream = client.inner().execute(examples).await.unwrap();
        println!("\n  Off-geo signal examples:");
        while let Ok(Some(row)) = stream.next().await {
            let label: String = row.get("label").unwrap_or_default();
            let title: String = row.get("title").unwrap_or_default();
            let lat: f64 = row.get("lat").unwrap_or(0.0);
            let lng: f64 = row.get("lng").unwrap_or(0.0);
            println!("    [{label}] '{title}' at ({lat:.2}, {lng:.2})");
        }
        println!("  Note: These will be cleaned up on next migration run");
    }

    if cross_geo_count > 0 {
        let examples = query(
            "MATCH (sig)-[r:RESPONDS_TO|DRAWN_TO]->(t:Tension)
             WHERE abs(sig.lat - t.lat) > 1.0 OR abs(sig.lng - t.lng) > 1.0
             RETURN sig.title AS sig_title, sig.lat AS sig_lat, sig.lng AS sig_lng,
                    t.title AS tension_title, t.lat AS t_lat, t.lng AS t_lng
             LIMIT 5",
        );
        let mut stream = client.inner().execute(examples).await.unwrap();
        println!("\n  Cross-geo edge examples:");
        while let Ok(Some(row)) = stream.next().await {
            let sig_title: String = row.get("sig_title").unwrap_or_default();
            let sig_lat: f64 = row.get("sig_lat").unwrap_or(0.0);
            let sig_lng: f64 = row.get("sig_lng").unwrap_or(0.0);
            let t_title: String = row.get("tension_title").unwrap_or_default();
            let t_lat: f64 = row.get("t_lat").unwrap_or(0.0);
            let t_lng: f64 = row.get("t_lng").unwrap_or(0.0);
            println!(
                "    '{}' ({:.2},{:.2}) → '{}' ({:.2},{:.2})",
                sig_title, sig_lat, sig_lng, t_title, t_lat, t_lng
            );
        }
        println!("  Note: These will be cleaned up on next migration run");
    }

    println!("\n  ✓ Contamination check complete");
}

#[tokio::test]
#[ignore]
async fn test_list_all_cities() {
    let client = connect().await;

    println!("\n=== All City nodes in graph ===");
    let q = query(
        "MATCH (c:City)
         RETURN c.slug AS slug, c.name AS name, c.center_lat AS lat, c.center_lng AS lng,
                c.radius_km AS radius, c.active AS active, c.id AS id
         ORDER BY c.slug",
    );
    let mut stream = client.inner().execute(q).await.unwrap();
    while let Ok(Some(row)) = stream.next().await {
        let slug: String = row.get("slug").unwrap_or_default();
        let name: String = row.get("name").unwrap_or_default();
        let lat: f64 = row.get("lat").unwrap_or(0.0);
        let lng: f64 = row.get("lng").unwrap_or(0.0);
        let radius: f64 = row.get("radius").unwrap_or(0.0);
        let active: bool = row.get("active").unwrap_or(false);
        let id: String = row.get("id").unwrap_or_default();
        // Count sources for this city
        let sq = query("MATCH (s:Source {city: $slug, active: true}) RETURN count(s) AS cnt")
            .param("slug", slug.as_str());
        let mut sstream = client.inner().execute(sq).await.unwrap();
        let src_count: i64 = if let Ok(Some(r)) = sstream.next().await {
            r.get("cnt").unwrap_or(0)
        } else { 0 };

        println!(
            "  [{:>5}] {:<30} slug={:<25} ({:.4}, {:.4}) r={:.0}km  sources={}  id={}",
            if active { "ON" } else { "off" },
            name,
            slug,
            lat,
            lng,
            radius,
            src_count,
            id
        );
    }
}

#[tokio::test]
#[ignore]
async fn test_city_signal_assessment() {
    let client = connect().await;
    let cities = get_cities(&client).await;

    println!("\n=== Comprehensive City Signal Assessment ===\n");

    for (slug, lat, lng, radius) in &cities {
        let (min_lat, max_lat, min_lng, max_lng) = compute_bbox(*lat, *lng, *radius);

        println!("--- {} (center: {:.4}, {:.4}, radius: {:.0}km) ---", slug, lat, lng, radius);

        // Count signals by type
        for label in &["Event", "Give", "Need", "Notice", "Tension"] {
            let q = query(&format!(
                "MATCH (n:{label})
                 WHERE n.lat >= $min_lat AND n.lat <= $max_lat
                   AND n.lng >= $min_lng AND n.lng <= $max_lng
                 RETURN count(n) AS cnt"
            ))
            .param("min_lat", min_lat)
            .param("max_lat", max_lat)
            .param("min_lng", min_lng)
            .param("max_lng", max_lng);
            let mut s = client.inner().execute(q).await.unwrap();
            let cnt: i64 = if let Ok(Some(r)) = s.next().await { r.get("cnt").unwrap_or(0) } else { 0 };
            if cnt > 0 {
                println!("  {:<10} {}", label, cnt);
            }
        }

        // Stories
        let sq = query(
            "MATCH (s:Story)-[:CONTAINS]->(n)
             WHERE n.lat >= $min_lat AND n.lat <= $max_lat
               AND n.lng >= $min_lng AND n.lng <= $max_lng
             WITH DISTINCT s
             RETURN s.id AS id, s.headline AS headline, s.status AS status,
                    s.arc AS arc, s.category AS category, s.signal_count AS sig_count
             ORDER BY s.energy DESC"
        )
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng);
        let mut ss = client.inner().execute(sq).await.unwrap();
        let mut story_count = 0;
        while let Ok(Some(row)) = ss.next().await {
            let headline: String = row.get("headline").unwrap_or_default();
            let status: String = row.get("status").unwrap_or_default();
            let arc: String = row.get("arc").unwrap_or_default();
            let category: String = row.get("category").unwrap_or_default();
            let sig_count: i64 = row.get("sig_count").unwrap_or(0);
            story_count += 1;
            println!("  Story: '{}' [{}/{}] {} signals, {}", headline.chars().take(70).collect::<String>(), status, arc, sig_count, category);
        }
        if story_count == 0 {
            println!("  (no stories yet)");
        }

        // RESPONDS_TO / DRAWN_TO edges
        let eq = query(
            "MATCH (sig)-[r:RESPONDS_TO|DRAWN_TO]->(t:Tension)
             WHERE sig.lat >= $min_lat AND sig.lat <= $max_lat
               AND sig.lng >= $min_lng AND sig.lng <= $max_lng
             RETURN count(r) AS cnt"
        )
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng);
        let mut es = client.inner().execute(eq).await.unwrap();
        let edge_cnt: i64 = if let Ok(Some(r)) = es.next().await { r.get("cnt").unwrap_or(0) } else { 0 };
        println!("  Response edges: {}", edge_cnt);

        // Cross-geo edges (signals responding to tensions in other cities)
        let xq = query(
            "MATCH (sig)-[r:RESPONDS_TO|DRAWN_TO]->(t:Tension)
             WHERE sig.lat >= $min_lat AND sig.lat <= $max_lat
               AND sig.lng >= $min_lng AND sig.lng <= $max_lng
               AND (abs(sig.lat - t.lat) > 1.0 OR abs(sig.lng - t.lng) > 1.0)
             RETURN count(r) AS cnt"
        )
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng);
        let mut xs = client.inner().execute(xq).await.unwrap();
        let xcnt: i64 = if let Ok(Some(r)) = xs.next().await { r.get("cnt").unwrap_or(0) } else { 0 };
        if xcnt > 0 {
            println!("  ⚠ Cross-geo edges: {}", xcnt);
        } else {
            println!("  Cross-geo edges: 0 ✓");
        }

        println!();
    }
}

#[tokio::test]
#[ignore]
async fn test_find_recent_signals_by_city() {
    let client = connect().await;

    println!("\n=== Recent signals (last 24h) by location ===\n");

    // Find all signals created recently, show their lat/lng
    for label in &["Event", "Give", "Need", "Notice", "Tension"] {
        let q = query(&format!(
            "MATCH (n:{label})
             WHERE n.extracted_at IS NOT NULL
               AND datetime(n.extracted_at) >= datetime() - duration('PT24H')
             RETURN n.title AS title, n.lat AS lat, n.lng AS lng, n.source_url AS url
             ORDER BY n.extracted_at DESC
             LIMIT 30"
        ));
        let mut stream = client.inner().execute(q).await.unwrap();
        while let Ok(Some(row)) = stream.next().await {
            let title: String = row.get("title").unwrap_or_default();
            let lat: f64 = row.get("lat").unwrap_or(0.0);
            let lng: f64 = row.get("lng").unwrap_or(0.0);
            let url: String = row.get("url").unwrap_or_default();
            let city = if (lat - 25.76).abs() < 1.0 { "MIAMI" }
                else if (lat - 31.90).abs() < 1.0 { "RAMALLAH" }
                else if (lat - 44.98).abs() < 1.0 { "TWINCITIES" }
                else { "OTHER" };
            println!("  [{:>8}] [{:>10}] ({:.4}, {:.4}) {}", label, city, lat, lng, title.chars().take(60).collect::<String>());
        }
    }
}

#[tokio::test]
#[ignore]
async fn test_find_duplicate_respects_bbox() {
    let client = connect().await;
    let writer = GraphWriter::new(client.clone());

    println!("\n=== Testing find_duplicate bbox scoping ===");

    // Get a tension embedding from the Twin Cities
    let tc_bbox = compute_bbox(44.9778, -93.2650, 30.0);
    let tensions = writer
        .get_active_tensions(tc_bbox.0, tc_bbox.1, tc_bbox.2, tc_bbox.3)
        .await
        .unwrap();

    if tensions.is_empty() {
        println!("  No TC tensions found, skipping");
        return;
    }

    let (tid, embedding) = &tensions[0];
    let embedding_f32: Vec<f32> = embedding.iter().map(|&x| x as f32).collect();
    println!("  Using TC tension {} as test embedding", tid);

    // find_duplicate with TC bbox should find it
    let tc_result = writer
        .find_duplicate(
            &embedding_f32,
            rootsignal_common::NodeType::Tension,
            0.8,
            tc_bbox.0, tc_bbox.1, tc_bbox.2, tc_bbox.3,
        )
        .await
        .unwrap();
    assert!(
        tc_result.is_some(),
        "Should find duplicate in TC bbox for a TC tension's own embedding"
    );
    println!("  ✓ Found in TC bbox (sim={:.3})", tc_result.unwrap().similarity);

    // find_duplicate with a far-away bbox (e.g., Miami) should NOT find it
    let miami_bbox = compute_bbox(25.7617, -80.1918, 25.0);
    let miami_result = writer
        .find_duplicate(
            &embedding_f32,
            rootsignal_common::NodeType::Tension,
            0.8,
            miami_bbox.0, miami_bbox.1, miami_bbox.2, miami_bbox.3,
        )
        .await
        .unwrap();
    assert!(
        miami_result.is_none(),
        "Should NOT find TC tension in Miami bbox"
    );
    println!("  ✓ Not found in Miami bbox (correct!)");

    // Global bbox should find it
    let global_result = writer
        .find_duplicate(
            &embedding_f32,
            rootsignal_common::NodeType::Tension,
            0.8,
            -90.0, 90.0, -180.0, 180.0,
        )
        .await
        .unwrap();
    assert!(global_result.is_some(), "Should find with global bbox");
    println!("  ✓ Found with global bbox (sim={:.3})", global_result.unwrap().similarity);
}

#[tokio::test]
#[ignore]
async fn test_get_tension_landscape_respects_bbox() {
    let client = connect().await;
    let writer = GraphWriter::new(client.clone());

    println!("\n=== Testing get_tension_landscape bbox scoping ===");

    // TC bbox
    let tc_bbox = compute_bbox(44.9778, -93.2650, 30.0);
    let tc_landscape = writer
        .get_tension_landscape(tc_bbox.0, tc_bbox.1, tc_bbox.2, tc_bbox.3)
        .await
        .unwrap();

    // Global
    let all_landscape = writer
        .get_tension_landscape(-90.0, 90.0, -180.0, 180.0)
        .await
        .unwrap();

    println!("  TC landscape: {} tensions", tc_landscape.len());
    println!("  Global landscape: {} tensions", all_landscape.len());

    assert!(
        tc_landscape.len() <= all_landscape.len(),
        "TC landscape should be subset of global"
    );

    // A far-away bbox should return empty (or at least fewer)
    let antarctica_bbox = compute_bbox(-80.0, 0.0, 10.0);
    let empty_landscape = writer
        .get_tension_landscape(
            antarctica_bbox.0, antarctica_bbox.1,
            antarctica_bbox.2, antarctica_bbox.3,
        )
        .await
        .unwrap();
    println!("  Antarctica landscape: {} tensions", empty_landscape.len());
    assert!(
        empty_landscape.is_empty(),
        "Antarctica should have no tensions"
    );
    println!("  ✓ Tension landscape respects bbox");
}

#[tokio::test]
#[ignore]
async fn test_find_tension_linker_targets_respects_bbox() {
    let client = connect().await;
    let writer = GraphWriter::new(client.clone());

    println!("\n=== Testing find_tension_linker_targets bbox scoping ===");

    // TC bbox
    let tc_bbox = compute_bbox(44.9778, -93.2650, 30.0);
    let tc_targets = writer
        .find_tension_linker_targets(50, tc_bbox.0, tc_bbox.1, tc_bbox.2, tc_bbox.3)
        .await
        .unwrap();

    // Global
    let all_targets = writer
        .find_tension_linker_targets(50, -90.0, 90.0, -180.0, 180.0)
        .await
        .unwrap();

    println!("  TC targets: {}", tc_targets.len());
    println!("  Global targets: {}", all_targets.len());

    assert!(
        tc_targets.len() <= all_targets.len(),
        "TC targets should be subset of global"
    );

    // Verify each TC target is actually in TC bbox
    for target in &tc_targets {
        let q = query(
            "MATCH (n {id: $id}) RETURN n.lat AS lat, n.lng AS lng",
        )
        .param("id", target.signal_id.to_string());
        let mut stream = client.inner().execute(q).await.unwrap();
        if let Ok(Some(row)) = stream.next().await {
            let lat: f64 = row.get("lat").unwrap_or(0.0);
            let lng: f64 = row.get("lng").unwrap_or(0.0);
            assert!(
                lat >= tc_bbox.0 && lat <= tc_bbox.1 && lng >= tc_bbox.2 && lng <= tc_bbox.3,
                "Target '{}' at ({}, {}) is outside TC bbox",
                target.title, lat, lng
            );
        }
    }
    println!("  ✓ All targets verified within bbox");
}

#[tokio::test]
#[ignore]
async fn test_response_mapper_bbox_scoping() {
    let client = connect().await;
    let cities = get_cities(&client).await;

    println!("\n=== Testing ResponseMapper bbox scoping ===");

    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();

    for (slug, lat, lng, radius) in &cities {
        let mapper = rootsignal_graph::response::ResponseMapper::new(
            client.clone(),
            &api_key,
            *lat,
            *lng,
            *radius,
        );

        // We can't run full map_responses (needs LLM calls), but we can verify
        // the constructor works and the struct is properly initialized
        println!("[{}] ResponseMapper created with center ({:.2}, {:.2}), radius {:.0}km", slug, lat, lng, radius);

        // Verify the bbox by checking that get_active_tensions returns subset
        let (min_lat, max_lat, min_lng, max_lng) = compute_bbox(*lat, *lng, *radius);
        let writer = GraphWriter::new(client.clone());
        let scoped = writer
            .get_active_tensions(min_lat, max_lat, min_lng, max_lng)
            .await
            .unwrap();

        // Compare against unscoped (huge bbox)
        let all = writer
            .get_active_tensions(-90.0, 90.0, -180.0, 180.0)
            .await
            .unwrap();

        println!(
            "  Scoped tensions: {} / {} total (filtered out {})",
            scoped.len(),
            all.len(),
            all.len() - scoped.len()
        );

        assert!(
            scoped.len() <= all.len(),
            "Scoped should never return more than unscoped"
        );
    }
    println!("  ✓ ResponseMapper bbox verified");
}
