use anyhow::Result;

use rootsignal_common::canonical_value;
use rootsignal_common::events::WorldEvent;
use rootsignal_common::types::{ActorNode, DiscoveryMethod, SourceNode, SourceRole};

use crate::traits::SignalReader;

pub struct ClaimResult {
    pub link_events: Vec<WorldEvent>,
    pub new_sources: Vec<SourceNode>,
}

/// For each actor with an external_url, either link to an existing source
/// or promote the URL as a new source.
pub async fn claim_profile_sources(
    reader: &dyn SignalReader,
    actors: &[(ActorNode, Vec<SourceNode>)],
) -> Result<ClaimResult> {
    let mut link_events = Vec::new();
    let mut new_sources = Vec::new();

    for (actor, linked_sources) in actors {
        let external_url = match &actor.external_url {
            Some(url) => url,
            None => continue,
        };

        let ext_ck = canonical_value(external_url);

        if linked_sources.iter().any(|s| s.canonical_key == ext_ck) {
            continue;
        }

        if let Some(source_id) = reader.find_source_by_canonical_key(&ext_ck).await? {
            link_events.push(WorldEvent::ActorLinkedToSource {
                actor_id: actor.id,
                source_id,
            });
        } else {
            let source = SourceNode::new(
                ext_ck.clone(),
                ext_ck,
                Some(external_url.clone()),
                DiscoveryMethod::LinkedFrom,
                0.5,
                SourceRole::Mixed,
                None,
            );
            link_events.push(WorldEvent::ActorLinkedToSource {
                actor_id: actor.id,
                source_id: source.id,
            });
            new_sources.push(source);
        }
    }

    Ok(ClaimResult {
        link_events,
        new_sources,
    })
}
