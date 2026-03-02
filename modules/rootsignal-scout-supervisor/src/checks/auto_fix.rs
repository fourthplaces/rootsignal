use neo4rs::query;
use tracing::info;
use uuid::Uuid;

use rootsignal_common::events::SystemEvent;
use rootsignal_graph::GraphClient;

use crate::types::AutoFixStats;

/// Run all deterministic auto-fix checks against the graph.
/// Returns stats and a vec of events describing what should be fixed.
/// The caller is responsible for persisting the events; the GraphProjector handles the writes.
pub async fn run_auto_fixes(
    client: &GraphClient,
    center_lat: f64,
    center_lng: f64,
) -> Result<(AutoFixStats, Vec<SystemEvent>), neo4rs::Error> {
    let mut stats = AutoFixStats::default();
    let mut events = Vec::new();

    if let Some(ev) = fix_orphaned_citations(client).await? {
        stats.orphaned_citations_deleted = match &ev {
            SystemEvent::OrphanedCitationsCleaned { citation_ids } => citation_ids.len() as u64,
            _ => 0,
        };
        events.push(ev);
    }

    if let Some(ev) = fix_orphaned_actors(client).await? {
        stats.orphaned_edges_deleted = match &ev {
            SystemEvent::OrphanedActorsCleaned { actor_ids } => actor_ids.len() as u64,
            _ => 0,
        };
        events.push(ev);
    }

    let merge_events = fix_duplicate_actors(client).await?;
    stats.actors_merged = merge_events.len() as u64;
    events.extend(merge_events);

    if let Some(ev) = fix_empty_signals(client).await? {
        stats.empty_signals_deleted = match &ev {
            SystemEvent::EmptyEntitiesCleaned { signal_ids } => signal_ids.len() as u64,
            _ => 0,
        };
        events.push(ev);
    }

    if let Some(ev) = fix_fake_center_coords(client, center_lat, center_lng).await? {
        stats.fake_coords_nulled = match &ev {
            SystemEvent::FakeCoordinatesNulled { signal_ids, .. } => signal_ids.len() as u64,
            _ => 0,
        };
        events.push(ev);
    }

    info!("{stats}");
    Ok((stats, events))
}

/// Find Citation nodes that have no SOURCED_FROM edge pointing to them.
async fn fix_orphaned_citations(
    client: &GraphClient,
) -> Result<Option<SystemEvent>, neo4rs::Error> {
    let q = query(
        "MATCH (ev:Citation)
         WHERE NOT ()-[:SOURCED_FROM]->(ev)
         RETURN ev.id AS id",
    );

    let mut ids = Vec::new();
    let mut stream = client.execute(q).await?;
    while let Some(row) = stream.next().await? {
        let id_str: String = row.get("id").unwrap_or_default();
        if let Ok(id) = Uuid::parse_str(&id_str) {
            ids.push(id);
        }
    }

    if ids.is_empty() {
        return Ok(None);
    }

    info!(deleted = ids.len(), "Found orphaned Citation nodes");
    Ok(Some(SystemEvent::OrphanedCitationsCleaned {
        citation_ids: ids,
    }))
}

/// Find Actor nodes with no remaining signal connections.
async fn fix_orphaned_actors(client: &GraphClient) -> Result<Option<SystemEvent>, neo4rs::Error> {
    let q = query(
        "MATCH (a:Actor)
         WHERE NOT (a)<-[:ACTED_IN]-()
         RETURN a.id AS id",
    );

    let mut ids = Vec::new();
    let mut stream = client.execute(q).await?;
    while let Some(row) = stream.next().await? {
        let id_str: String = row.get("id").unwrap_or_default();
        if let Ok(id) = Uuid::parse_str(&id_str) {
            ids.push(id);
        }
    }

    if ids.is_empty() {
        return Ok(None);
    }

    info!(deleted = ids.len(), "Found orphaned Actor nodes (no ACTED_IN edges)");
    Ok(Some(SystemEvent::OrphanedActorsCleaned { actor_ids: ids }))
}

/// Find duplicate Actors with identical normalized names.
/// Returns one DuplicateActorsMerged event per pair.
async fn fix_duplicate_actors(client: &GraphClient) -> Result<Vec<SystemEvent>, neo4rs::Error> {
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
         RETURN CASE WHEN a1_count >= a2_count THEN a1.id ELSE a2.id END AS winner_id,
                CASE WHEN a1_count >= a2_count THEN a2.id ELSE a1.id END AS loser_id,
                a1.name AS keep_name, a2.name AS drop_name",
    );

    let mut stream = client.execute(q).await?;
    let mut events = Vec::new();

    while let Some(row) = stream.next().await? {
        let winner_str: String = row.get("winner_id").unwrap_or_default();
        let loser_str: String = row.get("loser_id").unwrap_or_default();
        let keep_name: String = row.get("keep_name").unwrap_or_default();
        let drop_name: String = row.get("drop_name").unwrap_or_default();

        let (Ok(kept_id), Ok(merged_id)) =
            (Uuid::parse_str(&winner_str), Uuid::parse_str(&loser_str))
        else {
            continue;
        };

        info!(keep = %keep_name, drop = %drop_name, "Found duplicate Actors to merge");
        events.push(SystemEvent::DuplicateActorsMerged {
            kept_id,
            merged_ids: vec![merged_id],
        });
    }

    if !events.is_empty() {
        info!(merged = events.len(), "Found duplicate Actor pairs");
    }
    Ok(events)
}

/// Find signal nodes with empty or null titles.
async fn fix_empty_signals(client: &GraphClient) -> Result<Option<SystemEvent>, neo4rs::Error> {
    let mut ids = Vec::new();

    for label in &["Gathering", "Resource", "HelpRequest", "Announcement", "Concern"] {
        let q = query(&format!(
            "MATCH (n:{label})
             WHERE n.title IS NULL OR n.title = ''
             RETURN n.id AS id"
        ));

        let mut stream = client.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                info!(label, "Found signal with empty title");
                ids.push(id);
            }
        }
    }

    if ids.is_empty() {
        return Ok(None);
    }

    Ok(Some(SystemEvent::EmptyEntitiesCleaned { signal_ids: ids }))
}

/// Find coordinates that are suspiciously close to the scope center.
async fn fix_fake_center_coords(
    client: &GraphClient,
    center_lat: f64,
    center_lng: f64,
) -> Result<Option<SystemEvent>, neo4rs::Error> {
    let epsilon = 0.02;
    let mut signal_ids = Vec::new();
    let mut old_coords = Vec::new();

    for label in &["Gathering", "Resource", "HelpRequest", "Announcement", "Concern"] {
        let q = query(&format!(
            "MATCH (n:{label})
             WHERE n.lat IS NOT NULL AND n.lng IS NOT NULL
               AND abs(n.lat - $center_lat) < $epsilon
               AND abs(n.lng - $center_lng) < $epsilon
             RETURN n.id AS id, n.lat AS lat, n.lng AS lng"
        ))
        .param("center_lat", center_lat)
        .param("center_lng", center_lng)
        .param("epsilon", epsilon);

        let mut stream = client.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let lat: f64 = row.get("lat").unwrap_or(0.0);
            let lng: f64 = row.get("lng").unwrap_or(0.0);
            if let Ok(id) = Uuid::parse_str(&id_str) {
                info!(label, "Found fake center coordinates");
                signal_ids.push(id);
                old_coords.push((lat, lng));
            }
        }
    }

    if signal_ids.is_empty() {
        return Ok(None);
    }

    Ok(Some(SystemEvent::FakeCoordinatesNulled {
        signal_ids,
        old_coords,
    }))
}
