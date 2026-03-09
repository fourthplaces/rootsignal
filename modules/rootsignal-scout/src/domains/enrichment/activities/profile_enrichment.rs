use rootsignal_common::events::SystemEvent;
use rootsignal_common::types::{ActorNode, SourceNode};
use rootsignal_common::scraping_strategy;
use rootsignal_common::ScrapingStrategy;

use crate::traits::ContentFetcher;

/// Fetch profiles for actors with social sources but no bio yet.
/// Returns `ActorProfileEnriched` events for each actor whose profile was fetched.
pub async fn enrich_actor_profiles(
    fetcher: &dyn ContentFetcher,
    actors: &[(ActorNode, Vec<SourceNode>)],
) -> Vec<SystemEvent> {
    let mut events = Vec::new();

    for (actor, sources) in actors {
        if actor.bio.is_some() {
            continue;
        }

        let social_source = sources.iter().find_map(|s| {
            match scraping_strategy(s.value()) {
                ScrapingStrategy::Social(platform) => Some((s, platform)),
                _ => None,
            }
        });

        let (source, platform) = match social_source {
            Some(pair) => pair,
            None => continue,
        };

        let identifier = source
            .url
            .as_deref()
            .filter(|u| !u.is_empty())
            .unwrap_or(&source.canonical_value);

        match fetcher.profile(identifier, platform).await {
            Ok(Some(snapshot)) if snapshot.bio.is_some() || snapshot.external_url.is_some() => {
                events.push(SystemEvent::ActorProfileEnriched {
                    actor_id: actor.id,
                    bio: snapshot.bio,
                    external_url: snapshot.external_url,
                });
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(
                    actor_id = %actor.id,
                    source = identifier,
                    error = %e,
                    "profile fetch failed"
                );
            }
        }
    }

    events
}
