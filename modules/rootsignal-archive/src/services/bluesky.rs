// Bluesky service: stub. Not yet supported by Apify.
// When a Bluesky actor becomes available, implement HasPosts + HasTopicSearch here.

use anyhow::Result;
use tracing::info;
use uuid::Uuid;

use crate::store::InsertPost;

pub(crate) struct FetchedPost {
    pub post: InsertPost,
}

pub(crate) struct BlueskyService;

impl BlueskyService {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) async fn fetch_posts(
        &self,
        _identifier: &str,
        _source_id: Uuid,
        _limit: u32,
    ) -> Result<Vec<FetchedPost>> {
        info!("bluesky: not yet supported");
        anyhow::bail!("Bluesky is not yet supported")
    }

    pub(crate) async fn search_topics(
        &self,
        _topics: &[&str],
        _source_id: Uuid,
        _limit: u32,
    ) -> Result<Vec<FetchedPost>> {
        info!("bluesky: not yet supported");
        anyhow::bail!("Bluesky is not yet supported")
    }
}
