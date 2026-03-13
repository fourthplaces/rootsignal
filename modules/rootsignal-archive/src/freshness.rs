use chrono::{DateTime, Duration, Utc};

pub(crate) fn ttl_for(content_type: &str) -> Duration {
    match content_type {
        "posts" | "stories" | "short_videos" => Duration::hours(6),
        "pages" | "feeds" => Duration::hours(1),
        _ => Duration::zero(),
    }
}

pub(crate) fn is_fresh(last_scraped: DateTime<Utc>, content_type: &str) -> bool {
    let ttl = ttl_for(content_type);
    if ttl.is_zero() {
        return false;
    }
    Utc::now() - last_scraped < ttl
}
