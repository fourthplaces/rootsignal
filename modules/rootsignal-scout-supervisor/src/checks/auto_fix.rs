use neo4rs::query;
use tracing::{info, warn};

use rootsignal_graph::GraphClient;

use crate::types::AutoFixStats;

/// Run all deterministic auto-fix checks against the graph.
/// These are idempotent and safe to run concurrently with the scout.
pub async fn run_auto_fixes(
    client: &GraphClient,
    center_lat: f64,
    center_lng: f64,
) -> Result<AutoFixStats, neo4rs::Error> {
    let mut stats = AutoFixStats::default();

    stats.orphaned_citations_deleted = fix_orphaned_citations(client).await?;
    stats.orphaned_edges_deleted = fix_orphaned_acted_in_edges(client).await?;
    stats.actors_merged = fix_duplicate_actors(client).await?;
    stats.empty_signals_deleted = fix_empty_signals(client).await?;
    stats.fake_coords_nulled =
        fix_fake_center_coords(client, center_lat, center_lng).await?;

    info!("{stats}");
    Ok(stats)
}

/// Delete Citation nodes that have no SOURCED_FROM edge pointing to them.
async fn fix_orphaned_citations(client: &GraphClient) -> Result<u64, neo4rs::Error> {
    let q = query(
        "MATCH (ev:Citation)
         WHERE NOT ()-[:SOURCED_FROM]->(ev)
         DETACH DELETE ev
         RETURN count(ev) AS deleted",
    );

    let mut stream = client.inner().execute(q).await?;
    if let Some(row) = stream.next().await? {
        let deleted: i64 = row.get("deleted").unwrap_or(0);
        if deleted > 0 {
            info!(deleted, "Deleted orphaned Citation nodes");
        }
        return Ok(deleted as u64);
    }
    Ok(0)
}

/// Delete ACTED_IN edges where either the Actor or Signal node is missing.
async fn fix_orphaned_acted_in_edges(client: &GraphClient) -> Result<u64, neo4rs::Error> {
    // DETACH DELETE removes edges with the node, so truly orphaned
    // edges shouldn't exist. But partial transaction failures could leave them.
    // Check for Actor nodes with no remaining signal connections and clean up.
    let q = query(
        "MATCH (a:Actor)
         WHERE NOT (a)<-[:ACTED_IN]-()
         DETACH DELETE a
         RETURN count(a) AS deleted",
    );

    let mut stream = client.inner().execute(q).await?;
    if let Some(row) = stream.next().await? {
        let deleted: i64 = row.get("deleted").unwrap_or(0);
        if deleted > 0 {
            info!(deleted, "Deleted orphaned Actor nodes (no ACTED_IN edges)");
        }
        return Ok(deleted as u64);
    }
    Ok(0)
}

/// Merge duplicate Actors with identical normalized names.
/// Keeps the Actor with more signal connections, re-points edges from the duplicate.
async fn fix_duplicate_actors(client: &GraphClient) -> Result<u64, neo4rs::Error> {
    let mut merged = 0u64;

    // Find Actor pairs with the same lowercased name
    let q = query(
        "MATCH (a1:Actor), (a2:Actor)
         WHERE a1.id < a2.id
           AND toLower(replace(replace(a1.name, '-', ' '), '.', '')) =
               toLower(replace(replace(a2.name, '-', ' '), '.', ''))
         WITH a1, a2
         OPTIONAL MATCH (a1)<-[r1:ACTED_IN]-()
         WITH a1, a2, count(r1) AS a1_count
         OPTIONAL MATCH (a2)<-[r2:ACTED_IN]-()
         WITH a1, a2, a1_count, count(r2) AS a2_count
         RETURN a1.id AS keep_id, a2.id AS drop_id,
                a1.name AS keep_name, a2.name AS drop_name,
                CASE WHEN a1_count >= a2_count THEN a1.id ELSE a2.id END AS winner_id,
                CASE WHEN a1_count >= a2_count THEN a2.id ELSE a1.id END AS loser_id",
    );

    let mut stream = client.inner().execute(q).await?;
    let mut pairs: Vec<(String, String)> = Vec::new();

    while let Some(row) = stream.next().await? {
        let winner: String = row.get("winner_id").unwrap_or_default();
        let loser: String = row.get("loser_id").unwrap_or_default();
        let keep_name: String = row.get("keep_name").unwrap_or_default();
        let drop_name: String = row.get("drop_name").unwrap_or_default();
        if !winner.is_empty() && !loser.is_empty() {
            info!(keep = %keep_name, drop = %drop_name, "Merging duplicate Actors");
            pairs.push((winner, loser));
        }
    }

    for (winner_id, loser_id) in &pairs {
        // Re-point ACTED_IN edges from loser to winner
        let repoint = query(
            "MATCH (loser:Actor {id: $loser_id})<-[r:ACTED_IN]-(sig)
             MATCH (winner:Actor {id: $winner_id})
             CREATE (sig)-[:ACTED_IN {role: r.role}]->(winner)
             DELETE r",
        )
        .param("loser_id", loser_id.clone())
        .param("winner_id", winner_id.clone());

        match client.inner().run(repoint).await {
            Ok(_) => {}
            Err(e) => {
                warn!(loser = %loser_id, winner = %winner_id, error = %e, "Failed to re-point ACTED_IN edges");
                continue;
            }
        }

        // Delete the loser Actor
        let delete =
            query("MATCH (a:Actor {id: $id}) DETACH DELETE a").param("id", loser_id.clone());

        match client.inner().run(delete).await {
            Ok(_) => merged += 1,
            Err(e) => warn!(id = %loser_id, error = %e, "Failed to delete duplicate Actor"),
        }
    }

    if merged > 0 {
        info!(merged, "Merged duplicate Actor nodes");
    }
    Ok(merged)
}

/// Delete signal nodes with empty or null titles.
async fn fix_empty_signals(client: &GraphClient) -> Result<u64, neo4rs::Error> {
    let mut deleted = 0u64;

    for label in &["Gathering", "Aid", "Need", "Notice", "Tension"] {
        let q = query(&format!(
            "MATCH (n:{label})
             WHERE n.title IS NULL OR n.title = ''
             DETACH DELETE n
             RETURN count(n) AS deleted"
        ));

        let mut stream = client.inner().execute(q).await?;
        if let Some(row) = stream.next().await? {
            let d: i64 = row.get("deleted").unwrap_or(0);
            if d > 0 {
                info!(label, deleted = d, "Deleted signals with empty titles");
            }
            deleted += d as u64;
        }
    }

    Ok(deleted)
}

/// Null out coordinates that are suspiciously close to the scope center.
/// The scout strips coords within 0.01 degrees; we catch anything within 0.02
/// that slipped through (e.g., LLM echoed a slightly offset default).
async fn fix_fake_center_coords(
    client: &GraphClient,
    center_lat: f64,
    center_lng: f64,
) -> Result<u64, neo4rs::Error> {
    let mut nulled = 0u64;
    let epsilon = 0.02;

    for label in &["Gathering", "Aid", "Need", "Notice", "Tension"] {
        let q = query(&format!(
            "MATCH (n:{label})
             WHERE n.lat IS NOT NULL AND n.lng IS NOT NULL
               AND abs(n.lat - $center_lat) < $epsilon
               AND abs(n.lng - $center_lng) < $epsilon
             SET n.lat = null, n.lng = null
             RETURN count(n) AS nulled"
        ))
        .param("center_lat", center_lat)
        .param("center_lng", center_lng)
        .param("epsilon", epsilon);

        let mut stream = client.inner().execute(q).await?;
        if let Some(row) = stream.next().await? {
            let n: i64 = row.get("nulled").unwrap_or(0);
            if n > 0 {
                info!(label, nulled = n, "Nulled fake center coordinates");
            }
            nulled += n as u64;
        }
    }

    Ok(nulled)
}
