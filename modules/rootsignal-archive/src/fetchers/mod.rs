/// Internal fetcher implementations. Not exposed outside the archive crate.
/// Each fetcher handles one type of web content source.

pub(crate) mod page;
pub(crate) mod search;
pub(crate) mod social;
pub(crate) mod feed;
