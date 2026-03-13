use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Row types
// ---------------------------------------------------------------------------

pub struct ArchiveCounts {
    pub posts: i64,
    pub short_videos: i64,
    pub stories: i64,
    pub long_videos: i64,
    pub pages: i64,
    pub feeds: i64,
    pub search_results: i64,
    pub files: i64,
}

pub struct ArchiveVolumeDay {
    pub day: String,
    pub posts: i64,
    pub short_videos: i64,
    pub stories: i64,
    pub long_videos: i64,
    pub pages: i64,
    pub feeds: i64,
    pub search_results: i64,
    pub files: i64,
}

pub struct ArchivePostRow {
    pub id: Uuid,
    pub source_url: String,
    pub permalink: Option<String>,
    pub author: Option<String>,
    pub text: Option<String>,
    pub hashtags: Vec<String>,
    pub engagement: Option<serde_json::Value>,
    pub published_at: Option<DateTime<Utc>>,
    pub fetch_count: i64,
}

pub struct ArchiveShortVideoRow {
    pub id: Uuid,
    pub source_url: String,
    pub permalink: Option<String>,
    pub text: Option<String>,
    pub engagement: Option<serde_json::Value>,
    pub published_at: Option<DateTime<Utc>>,
    pub fetch_count: i64,
}

pub struct ArchiveStoryRow {
    pub id: Uuid,
    pub source_url: String,
    pub permalink: Option<String>,
    pub text: Option<String>,
    pub location: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub fetched_at: DateTime<Utc>,
    pub fetch_count: i64,
}

pub struct ArchiveLongVideoRow {
    pub id: Uuid,
    pub source_url: String,
    pub permalink: Option<String>,
    pub text: Option<String>,
    pub engagement: Option<serde_json::Value>,
    pub published_at: Option<DateTime<Utc>>,
    pub fetch_count: i64,
}

pub struct ArchivePageRow {
    pub id: Uuid,
    pub source_url: String,
    pub title: Option<String>,
    pub fetched_at: DateTime<Utc>,
    pub fetch_count: i64,
}

pub struct ArchiveFeedRow {
    pub id: Uuid,
    pub source_url: String,
    pub title: Option<String>,
    pub item_count: i64,
    pub fetched_at: DateTime<Utc>,
    pub fetch_count: i64,
}

pub struct ArchiveSearchResultRow {
    pub id: Uuid,
    pub query: String,
    pub result_count: i64,
    pub fetched_at: DateTime<Utc>,
}

pub struct ArchiveFileRow {
    pub id: Uuid,
    pub url: String,
    pub title: Option<String>,
    pub mime_type: String,
    pub duration: Option<f64>,
    pub page_count: Option<i32>,
    pub fetched_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// Get total row counts for all 8 content types in parallel.
pub async fn count_all(pool: &PgPool) -> Result<ArchiveCounts> {
    let (posts, short_videos, stories, long_videos, pages, feeds, search_results, files) = tokio::try_join!(
        count_table(pool, "posts"),
        count_table(pool, "short_videos"),
        count_table(pool, "stories"),
        count_table(pool, "long_videos"),
        count_table(pool, "pages"),
        count_table(pool, "feeds"),
        count_table(pool, "search_results"),
        count_table(pool, "files"),
    )?;

    Ok(ArchiveCounts {
        posts,
        short_videos,
        stories,
        long_videos,
        pages,
        feeds,
        search_results,
        files,
    })
}

/// Get daily ingestion volume for the last N days, broken down by content type.
pub async fn volume_by_day(pool: &PgPool, days: u32) -> Result<Vec<ArchiveVolumeDay>> {
    let days = days.min(30) as i64;

    let rows = sqlx::query_as::<_, (String, i64, i64, i64, i64, i64, i64, i64, i64)>(
        r#"
        WITH date_series AS (
            SELECT to_char(d::date, 'Mon DD') AS day, d::date AS dt
            FROM generate_series(
                current_date - ($1 - 1) * interval '1 day',
                current_date,
                '1 day'
            ) AS d
        ),
        counts AS (
            SELECT date_trunc('day', fetched_at)::date AS dt, 'posts' AS content_type, COUNT(*) AS cnt
            FROM posts WHERE fetched_at >= current_date - $1 * interval '1 day' GROUP BY 1
            UNION ALL
            SELECT date_trunc('day', fetched_at)::date, 'short_videos', COUNT(*)
            FROM short_videos WHERE fetched_at >= current_date - $1 * interval '1 day' GROUP BY 1
            UNION ALL
            SELECT date_trunc('day', fetched_at)::date, 'stories', COUNT(*)
            FROM stories WHERE fetched_at >= current_date - $1 * interval '1 day' GROUP BY 1
            UNION ALL
            SELECT date_trunc('day', fetched_at)::date, 'long_videos', COUNT(*)
            FROM long_videos WHERE fetched_at >= current_date - $1 * interval '1 day' GROUP BY 1
            UNION ALL
            SELECT date_trunc('day', fetched_at)::date, 'pages', COUNT(*)
            FROM pages WHERE fetched_at >= current_date - $1 * interval '1 day' GROUP BY 1
            UNION ALL
            SELECT date_trunc('day', fetched_at)::date, 'feeds', COUNT(*)
            FROM feeds WHERE fetched_at >= current_date - $1 * interval '1 day' GROUP BY 1
            UNION ALL
            SELECT date_trunc('day', fetched_at)::date, 'search_results', COUNT(*)
            FROM search_results WHERE fetched_at >= current_date - $1 * interval '1 day' GROUP BY 1
            UNION ALL
            SELECT date_trunc('day', fetched_at)::date, 'files', COUNT(*)
            FROM files WHERE fetched_at >= current_date - $1 * interval '1 day' GROUP BY 1
        )
        SELECT
            ds.day,
            COALESCE(SUM(c.cnt) FILTER (WHERE c.content_type = 'posts'), 0),
            COALESCE(SUM(c.cnt) FILTER (WHERE c.content_type = 'short_videos'), 0),
            COALESCE(SUM(c.cnt) FILTER (WHERE c.content_type = 'stories'), 0),
            COALESCE(SUM(c.cnt) FILTER (WHERE c.content_type = 'long_videos'), 0),
            COALESCE(SUM(c.cnt) FILTER (WHERE c.content_type = 'pages'), 0),
            COALESCE(SUM(c.cnt) FILTER (WHERE c.content_type = 'feeds'), 0),
            COALESCE(SUM(c.cnt) FILTER (WHERE c.content_type = 'search_results'), 0),
            COALESCE(SUM(c.cnt) FILTER (WHERE c.content_type = 'files'), 0)
        FROM date_series ds
        LEFT JOIN counts c ON c.dt = ds.dt
        GROUP BY ds.day, ds.dt
        ORDER BY ds.dt
        "#,
    )
    .bind(days)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| ArchiveVolumeDay {
            day: r.0,
            posts: r.1,
            short_videos: r.2,
            stories: r.3,
            long_videos: r.4,
            pages: r.5,
            feeds: r.6,
            search_results: r.7,
            files: r.8,
        })
        .collect())
}

pub async fn recent_posts(pool: &PgPool, limit: u32) -> Result<Vec<ArchivePostRow>> {
    let limit = limit.min(100) as i64;

    let rows = sqlx::query_as::<_, (Uuid, String, Option<String>, Option<String>, Option<String>, Vec<String>, Option<serde_json::Value>, Option<DateTime<Utc>>, i64)>(
        r#"
        SELECT p.id, s.url, p.permalink, p.author, p.text, p.hashtags, p.engagement, p.published_at, s.fetch_count
        FROM posts p
        JOIN sources s ON s.id = p.source_id
        ORDER BY p.fetched_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| ArchivePostRow {
            id: r.0,
            source_url: r.1,
            permalink: r.2,
            author: r.3,
            text: r.4,
            hashtags: r.5,
            engagement: r.6,
            published_at: r.7,
            fetch_count: r.8,
        })
        .collect())
}

pub async fn recent_short_videos(pool: &PgPool, limit: u32) -> Result<Vec<ArchiveShortVideoRow>> {
    let limit = limit.min(100) as i64;

    let rows = sqlx::query_as::<
        _,
        (
            Uuid,
            String,
            Option<String>,
            Option<String>,
            Option<serde_json::Value>,
            Option<DateTime<Utc>>,
            i64,
        ),
    >(
        r#"
        SELECT sv.id, s.url, sv.permalink, sv.text, sv.engagement, sv.published_at, s.fetch_count
        FROM short_videos sv
        JOIN sources s ON s.id = sv.source_id
        ORDER BY sv.fetched_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| ArchiveShortVideoRow {
            id: r.0,
            source_url: r.1,
            permalink: r.2,
            text: r.3,
            engagement: r.4,
            published_at: r.5,
            fetch_count: r.6,
        })
        .collect())
}

pub async fn recent_stories(pool: &PgPool, limit: u32) -> Result<Vec<ArchiveStoryRow>> {
    let limit = limit.min(100) as i64;

    let rows = sqlx::query_as::<_, (Uuid, String, Option<String>, Option<String>, Option<String>, Option<DateTime<Utc>>, DateTime<Utc>, i64)>(
        r#"
        SELECT st.id, s.url, st.permalink, st.text, st.location, st.expires_at, st.fetched_at, s.fetch_count
        FROM stories st
        JOIN sources s ON s.id = st.source_id
        ORDER BY st.fetched_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| ArchiveStoryRow {
            id: r.0,
            source_url: r.1,
            permalink: r.2,
            text: r.3,
            location: r.4,
            expires_at: r.5,
            fetched_at: r.6,
            fetch_count: r.7,
        })
        .collect())
}

pub async fn recent_long_videos(pool: &PgPool, limit: u32) -> Result<Vec<ArchiveLongVideoRow>> {
    let limit = limit.min(100) as i64;

    let rows = sqlx::query_as::<
        _,
        (
            Uuid,
            String,
            Option<String>,
            Option<String>,
            Option<serde_json::Value>,
            Option<DateTime<Utc>>,
            i64,
        ),
    >(
        r#"
        SELECT lv.id, s.url, lv.permalink, lv.text, lv.engagement, lv.published_at, s.fetch_count
        FROM long_videos lv
        JOIN sources s ON s.id = lv.source_id
        ORDER BY lv.fetched_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| ArchiveLongVideoRow {
            id: r.0,
            source_url: r.1,
            permalink: r.2,
            text: r.3,
            engagement: r.4,
            published_at: r.5,
            fetch_count: r.6,
        })
        .collect())
}

pub async fn recent_pages(pool: &PgPool, limit: u32) -> Result<Vec<ArchivePageRow>> {
    let limit = limit.min(100) as i64;

    let rows = sqlx::query_as::<_, (Uuid, String, Option<String>, DateTime<Utc>, i64)>(
        r#"
        SELECT pg.id, s.url, pg.title, pg.fetched_at, s.fetch_count
        FROM pages pg
        JOIN sources s ON s.id = pg.source_id
        ORDER BY pg.fetched_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| ArchivePageRow {
            id: r.0,
            source_url: r.1,
            title: r.2,
            fetched_at: r.3,
            fetch_count: r.4,
        })
        .collect())
}

pub async fn recent_feeds(pool: &PgPool, limit: u32) -> Result<Vec<ArchiveFeedRow>> {
    let limit = limit.min(100) as i64;

    let rows = sqlx::query_as::<_, (Uuid, String, Option<String>, i64, DateTime<Utc>, i64)>(
        r#"
        SELECT f.id, s.url, f.title, COALESCE(jsonb_array_length(f.items), 0), f.fetched_at, s.fetch_count
        FROM feeds f
        JOIN sources s ON s.id = f.source_id
        ORDER BY f.fetched_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| ArchiveFeedRow {
            id: r.0,
            source_url: r.1,
            title: r.2,
            item_count: r.3,
            fetched_at: r.4,
            fetch_count: r.5,
        })
        .collect())
}

pub async fn recent_search_results(
    pool: &PgPool,
    limit: u32,
) -> Result<Vec<ArchiveSearchResultRow>> {
    let limit = limit.min(100) as i64;

    let rows = sqlx::query_as::<_, (Uuid, String, i64, DateTime<Utc>)>(
        r#"
        SELECT sr.id, sr.query, COALESCE(jsonb_array_length(sr.results), 0)::BIGINT, sr.fetched_at
        FROM search_results sr
        ORDER BY sr.fetched_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| ArchiveSearchResultRow {
            id: r.0,
            query: r.1,
            result_count: r.2,
            fetched_at: r.3,
        })
        .collect())
}

pub async fn recent_files(pool: &PgPool, limit: u32) -> Result<Vec<ArchiveFileRow>> {
    let limit = limit.min(100) as i64;

    let rows = sqlx::query_as::<
        _,
        (
            Uuid,
            String,
            Option<String>,
            String,
            Option<f64>,
            Option<i32>,
            DateTime<Utc>,
        ),
    >(
        r#"
        SELECT id, url, title, mime_type, duration, page_count, fetched_at
        FROM files
        ORDER BY fetched_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| ArchiveFileRow {
            id: r.0,
            url: r.1,
            title: r.2,
            mime_type: r.3,
            duration: r.4,
            page_count: r.5,
            fetched_at: r.6,
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

async fn count_table(pool: &PgPool, table: &str) -> Result<i64> {
    // Table names are hardcoded constants, not user input -- safe to interpolate.
    let query = format!("SELECT COUNT(*) FROM {table}");
    let (count,): (i64,) = sqlx::query_as(&query).fetch_one(pool).await?;
    Ok(count)
}

/// Format a JSONB engagement object into a human-readable summary string.
/// e.g. {"likes": 1500, "comments": 42} → "1.5k likes, 42 comments"
pub fn format_engagement(engagement: &Option<serde_json::Value>) -> String {
    let Some(serde_json::Value::Object(map)) = engagement else {
        return String::new();
    };
    if map.is_empty() {
        return String::new();
    }

    let parts: Vec<String> = map
        .iter()
        .filter_map(|(key, val)| {
            let n = val.as_f64()?;
            if n == 0.0 {
                return None;
            }
            Some(format!("{} {key}", format_number(n)))
        })
        .collect();

    parts.join(", ")
}

fn format_number(n: f64) -> String {
    if n >= 1_000_000.0 {
        format!("{:.1}M", n / 1_000_000.0)
    } else if n >= 1_000.0 {
        format!("{:.1}k", n / 1_000.0)
    } else {
        format!("{}", n as i64)
    }
}

/// Truncate text to a maximum number of characters, appending "..." if truncated.
pub fn truncate_text(text: &Option<String>, max_chars: usize) -> Option<String> {
    text.as_ref().map(|t| {
        if t.len() <= max_chars {
            t.clone()
        } else {
            let truncated: String = t.chars().take(max_chars).collect();
            format!("{truncated}...")
        }
    })
}

/// Derive a platform name from a source URL domain.
/// e.g. "https://www.instagram.com/user" → "Instagram"
pub fn platform_from_url(url: &str) -> String {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let domain = without_scheme
        .split('/')
        .next()
        .unwrap_or(url)
        .strip_prefix("www.")
        .unwrap_or(without_scheme.split('/').next().unwrap_or(url));

    match domain {
        d if d.contains("instagram.com") => "Instagram".to_string(),
        d if d.contains("facebook.com") || d.contains("fb.com") => "Facebook".to_string(),
        d if d.contains("twitter.com") || d.contains("x.com") => "Twitter/X".to_string(),
        d if d.contains("reddit.com") => "Reddit".to_string(),
        d if d.contains("tiktok.com") => "TikTok".to_string(),
        d if d.contains("bsky.app") || d.contains("bluesky") => "Bluesky".to_string(),
        d if d.contains("youtube.com") || d.contains("youtu.be") => "YouTube".to_string(),
        _ => domain.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Source-scoped archive summary
// ---------------------------------------------------------------------------

pub struct ArchiveSourceSummary {
    pub posts: i64,
    pub pages: i64,
    pub feeds: i64,
    pub short_videos: i64,
    pub long_videos: i64,
    pub stories: i64,
    pub search_results: i64,
    pub files: i64,
    pub last_fetched_at: Option<DateTime<Utc>>,
}

/// Count archive rows per content type for a given source URL.
pub async fn archive_summary_for_source(
    pool: &PgPool,
    source_url: &str,
) -> Result<ArchiveSourceSummary> {
    let row = sqlx::query_as::<_, (i64, i64, i64, i64, i64, i64, i64, i64, Option<DateTime<Utc>>)>(
        r#"
        SELECT
            (SELECT count(*) FROM posts p JOIN sources s ON s.id = p.source_id WHERE s.url = $1),
            (SELECT count(*) FROM pages p JOIN sources s ON s.id = p.source_id WHERE s.url = $1),
            (SELECT count(*) FROM feeds f JOIN sources s ON s.id = f.source_id WHERE s.url = $1),
            (SELECT count(*) FROM short_videos sv JOIN sources s ON s.id = sv.source_id WHERE s.url = $1),
            (SELECT count(*) FROM long_videos lv JOIN sources s ON s.id = lv.source_id WHERE s.url = $1),
            (SELECT count(*) FROM stories st JOIN sources s ON s.id = st.source_id WHERE s.url = $1),
            (SELECT count(*) FROM search_results sr JOIN sources s ON s.id = sr.source_id WHERE s.url = $1),
            (SELECT count(*) FROM files f2 JOIN sources s ON s.id = f2.source_id WHERE s.url = $1),
            (SELECT max(s.last_scraped_at) FROM source_content_types sct JOIN sources s ON s.id = sct.source_id WHERE s.url = $1)
        "#,
    )
    .bind(source_url)
    .fetch_one(pool)
    .await?;

    Ok(ArchiveSourceSummary {
        posts: row.0,
        pages: row.1,
        feeds: row.2,
        short_videos: row.3,
        long_videos: row.4,
        stories: row.5,
        search_results: row.6,
        files: row.7,
        last_fetched_at: row.8,
    })
}
