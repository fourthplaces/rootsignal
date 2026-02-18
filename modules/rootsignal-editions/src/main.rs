use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate, Utc};
use tracing::info;
use tracing_subscriber::EnvFilter;

use rootsignal_common::Config;
use rootsignal_graph::{edition::EditionGenerator, migrate::migrate, GraphClient};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("rootsignal=info".parse()?))
        .init();

    info!("Root Signal Editions starting...");

    let config = Config::editions_from_env();
    config.log_redacted();

    let client =
        GraphClient::connect(&config.neo4j_uri, &config.neo4j_user, &config.neo4j_password)
            .await?;

    migrate(&client).await?;

    // Parse optional --week arg (e.g. "2026-W07"), default to current week
    let (period_start, period_end) = match std::env::args().nth(1).as_deref() {
        Some("--week") => {
            let week_str = std::env::args()
                .nth(2)
                .expect("--week requires a value like 2026-W07");
            parse_iso_week(&week_str)?
        }
        _ => current_week(),
    };

    info!(
        city = config.city,
        period_start = %period_start,
        period_end = %period_end,
        "Generating edition"
    );

    let generator = EditionGenerator::new(client, &config.anthropic_api_key);
    let edition = generator
        .generate_edition(&config.city, period_start, period_end)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    info!(
        edition_id = %edition.id,
        stories = edition.story_count,
        signals = edition.new_signal_count,
        "Edition published"
    );
    println!("\n=== {} Edition: {} ===", edition.city, edition.period);
    println!("Stories: {}  |  New signals: {}", edition.story_count, edition.new_signal_count);
    println!("\n{}", edition.editorial_summary);

    Ok(())
}

/// Return (Monday 00:00 UTC, Sunday 23:59:59 UTC) for the current ISO week.
fn current_week() -> (chrono::DateTime<Utc>, chrono::DateTime<Utc>) {
    let now = Utc::now();
    let weekday = now.weekday().num_days_from_monday(); // Mon=0, Sun=6
    let monday = (now - Duration::days(weekday as i64))
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc();
    let sunday = monday + Duration::days(7) - Duration::seconds(1);
    (monday, sunday)
}

/// Parse "2026-W07" into (Monday 00:00 UTC, Sunday 23:59:59 UTC).
fn parse_iso_week(s: &str) -> Result<(chrono::DateTime<Utc>, chrono::DateTime<Utc>)> {
    let parts: Vec<&str> = s.split("-W").collect();
    anyhow::ensure!(parts.len() == 2, "Expected format YYYY-Www, got {s}");
    let year: i32 = parts[0].parse()?;
    let week: u32 = parts[1].parse()?;
    anyhow::ensure!((1..=53).contains(&week), "Week must be 1-53, got {week}");

    // ISO week 1 contains the first Thursday of the year.
    // Jan 4 is always in week 1. Find that Monday.
    let jan4 = NaiveDate::from_ymd_opt(year, 1, 4).unwrap();
    let jan4_weekday = jan4.weekday().num_days_from_monday();
    let week1_monday = jan4 - Duration::days(jan4_weekday as i64);
    let target_monday = week1_monday + Duration::weeks((week - 1) as i64);

    let start = target_monday.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end = start + Duration::days(7) - Duration::seconds(1);
    Ok((start, end))
}
