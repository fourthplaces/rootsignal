use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::sync::Arc;

use rootsignal_common::types::{ArchiveItem, Channels};
use tracing::warn;

use crate::error::Result;
use crate::router::Platform;
use crate::source_handle::ArchiveInner;
use rootsignal_common::types::Source;

/// A multi-channel fetch request. Built by `SourceHandle::fetch()`.
///
/// Implements `IntoFuture` so callers can `.await` directly:
/// ```ignore
/// let items = handle.fetch(Channels::everything()).await?;
/// ```
pub struct FetchRequest {
    pub(crate) inner: Arc<ArchiveInner>,
    pub(crate) source: Source,
    pub(crate) platform: Platform,
    pub(crate) identifier: String,
    pub(crate) channels: Channels,
    pub(crate) post_limit: u32,
    pub(crate) video_limit: u32,
}

impl FetchRequest {
    pub fn post_limit(mut self, n: u32) -> Self {
        self.post_limit = n;
        self
    }

    pub fn video_limit(mut self, n: u32) -> Self {
        self.video_limit = n;
        self
    }

    pub async fn send(self) -> Result<Vec<ArchiveItem>> {
        if self.channels.is_empty() {
            return Ok(Vec::new());
        }

        let mut futures: Vec<Pin<Box<dyn Future<Output = Option<ArchiveItem>> + Send>>> =
            Vec::new();

        // page channel: Web only
        if self.channels.page && self.platform == Platform::Web {
            let inner = self.inner.clone();
            let source = self.source.clone();
            futures.push(Box::pin(async move {
                let handle = crate::source_handle::SourceHandle {
                    inner,
                    source,
                    platform: Platform::Web,
                    identifier: String::new(),
                };
                match handle.page().send().await {
                    Ok(page) => Some(ArchiveItem::Page(page)),
                    Err(e) => {
                        warn!(error = %e, "fetch: page channel failed");
                        None
                    }
                }
            }));
        }

        // feed channel: Web → feed(), social → posts()
        if self.channels.feed {
            match self.platform {
                Platform::Web => {
                    let inner = self.inner.clone();
                    let source = self.source.clone();
                    futures.push(Box::pin(async move {
                        let handle = crate::source_handle::SourceHandle {
                            inner,
                            source,
                            platform: Platform::Web,
                            identifier: String::new(),
                        };
                        match handle.feed().send().await {
                            Ok(feed) => Some(ArchiveItem::Feed(feed)),
                            Err(e) => {
                                warn!(error = %e, "fetch: feed channel failed");
                                None
                            }
                        }
                    }));
                }
                Platform::Instagram
                | Platform::Twitter
                | Platform::Reddit
                | Platform::Facebook
                | Platform::TikTok
                | Platform::Bluesky => {
                    let inner = self.inner.clone();
                    let source = self.source.clone();
                    let platform = self.platform;
                    let identifier = self.identifier.clone();
                    let limit = self.post_limit;
                    futures.push(Box::pin(async move {
                        let handle = crate::source_handle::SourceHandle {
                            inner,
                            source,
                            platform,
                            identifier,
                        };
                        match handle.posts(limit).send().await {
                            Ok(posts) => Some(ArchiveItem::Posts(posts)),
                            Err(e) => {
                                warn!(error = %e, platform = ?platform, "fetch: feed channel (posts) failed");
                                None
                            }
                        }
                    }));
                }
            }
        }

        // media channel: Instagram → stories + short_videos, TikTok → short_videos
        if self.channels.media {
            match self.platform {
                Platform::Instagram => {
                    // stories
                    let inner = self.inner.clone();
                    let source = self.source.clone();
                    let identifier = self.identifier.clone();
                    futures.push(Box::pin(async move {
                        let handle = crate::source_handle::SourceHandle {
                            inner,
                            source,
                            platform: Platform::Instagram,
                            identifier,
                        };
                        match handle.stories().send().await {
                            Ok(stories) => Some(ArchiveItem::Stories(stories)),
                            Err(e) => {
                                warn!(error = %e, "fetch: media channel (stories) failed");
                                None
                            }
                        }
                    }));

                    // short_videos
                    let inner = self.inner.clone();
                    let source = self.source.clone();
                    let identifier = self.identifier.clone();
                    let limit = self.video_limit;
                    futures.push(Box::pin(async move {
                        let handle = crate::source_handle::SourceHandle {
                            inner,
                            source,
                            platform: Platform::Instagram,
                            identifier,
                        };
                        match handle.short_videos(limit).send().await {
                            Ok(videos) => Some(ArchiveItem::ShortVideos(videos)),
                            Err(e) => {
                                warn!(error = %e, "fetch: media channel (short_videos) failed");
                                None
                            }
                        }
                    }));
                }
                Platform::TikTok => {
                    let inner = self.inner.clone();
                    let source = self.source.clone();
                    let identifier = self.identifier.clone();
                    let limit = self.video_limit;
                    futures.push(Box::pin(async move {
                        let handle = crate::source_handle::SourceHandle {
                            inner,
                            source,
                            platform: Platform::TikTok,
                            identifier,
                        };
                        match handle.short_videos(limit).send().await {
                            Ok(videos) => Some(ArchiveItem::ShortVideos(videos)),
                            Err(e) => {
                                warn!(error = %e, "fetch: media channel (short_videos) failed");
                                None
                            }
                        }
                    }));
                }
                other => {
                    warn!(platform = ?other, "fetch: media channel not supported for platform");
                }
            }
        }

        let results = futures::future::join_all(futures).await;
        Ok(results.into_iter().flatten().collect())
    }
}

impl IntoFuture for FetchRequest {
    type Output = Result<Vec<ArchiveItem>>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.send())
    }
}
