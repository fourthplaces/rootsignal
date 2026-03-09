use std::collections::HashSet;
use std::time::Instant;

use anyhow::{Context, Result};
use sqlx::PgPool;
use tracing::{info, warn};

use crate::{MigrateContext, Migration, MigrationBody};

/// Advisory lock key — "rootsig" as bytes → i64.
const ADVISORY_LOCK_KEY: i64 = 0x726F6F74_7369676E;

async fn ensure_table(pool: &PgPool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _migrations (
            name TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            completed_at TIMESTAMPTZ,
            duration_ms BIGINT,
            checksum TEXT
        )",
    )
    .execute(pool)
    .await
    .context("failed to create _migrations table")?;
    Ok(())
}

async fn completed_migrations(pool: &PgPool) -> Result<HashSet<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT name FROM _migrations WHERE completed_at IS NOT NULL")
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|(name,)| name).collect())
}

async fn acquire_lock(pool: &PgPool) -> Result<()> {
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(ADVISORY_LOCK_KEY)
        .execute(pool)
        .await
        .context("failed to acquire advisory lock")?;
    Ok(())
}

async fn release_lock(pool: &PgPool) -> Result<()> {
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(ADVISORY_LOCK_KEY)
        .execute(pool)
        .await
        .context("failed to release advisory lock")?;
    Ok(())
}

fn checksum(sql: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(sql.as_bytes());
    format!("{:x}", hash)
}

/// Default mode: show what would run without touching anything.
pub async fn dry_run(ctx: &MigrateContext, migrations: &[Migration]) -> Result<()> {
    let pg = ctx.pg();
    ensure_table(pg).await?;
    let done = completed_migrations(pg).await?;

    let pending: Vec<&Migration> = migrations
        .iter()
        .filter(|m| !done.contains(m.name))
        .collect();

    if pending.is_empty() {
        info!("All migrations already applied.");
        return Ok(());
    }

    println!();
    for m in &pending {
        match &m.body {
            MigrationBody::Sql(sql) => {
                let stmt_count = sql
                    .split(';')
                    .filter(|s| !s.trim().is_empty())
                    .count();
                println!(
                    "  \u{25cb} {} (sql, {} statement{})",
                    m.name,
                    stmt_count,
                    if stmt_count == 1 { "" } else { "s" }
                );
            }
            MigrationBody::DataMigration { plan, .. } => {
                println!("  \u{25cb} {} (data)", m.name);
                match plan(ctx).await {
                    Ok(description) => {
                        for line in description.lines() {
                            println!("    \u{2192} {line}");
                        }
                    }
                    Err(e) => {
                        println!("    \u{2192} plan failed: {e}");
                    }
                }
            }
        }
    }

    println!(
        "\n{} pending migration{}. Run with --commit to apply.\n",
        pending.len(),
        if pending.len() == 1 { "" } else { "s" }
    );

    Ok(())
}

/// Execute pending migrations.
pub async fn commit(ctx: &MigrateContext, migrations: &[Migration]) -> Result<()> {
    let pg = ctx.pg();
    ensure_table(pg).await?;
    acquire_lock(pg).await?;

    let result = commit_inner(ctx, migrations).await;

    release_lock(pg).await?;
    result
}

async fn commit_inner(ctx: &MigrateContext, migrations: &[Migration]) -> Result<()> {
    let pg = ctx.pg();
    let done = completed_migrations(pg).await?;

    let pending: Vec<&Migration> = migrations
        .iter()
        .filter(|m| !done.contains(m.name))
        .collect();

    if pending.is_empty() {
        info!("All migrations already applied.");
        return Ok(());
    }

    println!();
    let mut applied = 0;

    for m in &pending {
        let start = Instant::now();

        sqlx::query(
            "INSERT INTO _migrations (name, kind, started_at)
             VALUES ($1, $2, now())
             ON CONFLICT (name) DO UPDATE SET started_at = now(), completed_at = NULL",
        )
        .bind(m.name)
        .bind(m.kind())
        .execute(pg)
        .await?;

        match &m.body {
            MigrationBody::Sql(sql) => {
                let mut tx = pg.begin().await?;
                sqlx::query(sql)
                    .execute(&mut *tx)
                    .await
                    .with_context(|| format!("migration {} failed", m.name))?;
                tx.commit().await?;
            }
            MigrationBody::DataMigration { run, .. } => {
                run(ctx)
                    .await
                    .with_context(|| format!("data migration {} failed", m.name))?;
            }
        }

        let elapsed = start.elapsed();
        let duration_ms = elapsed.as_millis() as i64;

        let cs = m.sql_text().map(checksum);
        sqlx::query(
            "UPDATE _migrations
             SET completed_at = now(), duration_ms = $2, checksum = $3
             WHERE name = $1",
        )
        .bind(m.name)
        .bind(duration_ms)
        .bind(&cs)
        .execute(pg)
        .await?;

        println!("  \u{2713} {} ({:.1}s)", m.name, elapsed.as_secs_f64());
        applied += 1;
    }

    println!(
        "\n{} migration{} applied.\n",
        applied,
        if applied == 1 { "" } else { "s" }
    );

    Ok(())
}

/// Seed migrations 001..=N as already completed without running them.
pub async fn baseline(ctx: &MigrateContext, migrations: &[Migration], up_to: &str) -> Result<()> {
    let pg = ctx.pg();
    ensure_table(pg).await?;

    let mut seeded = 0;
    for m in migrations {
        let cs = m.sql_text().map(checksum);
        sqlx::query(
            "INSERT INTO _migrations (name, kind, started_at, completed_at, duration_ms, checksum)
             VALUES ($1, $2, now(), now(), 0, $3)
             ON CONFLICT (name) DO NOTHING",
        )
        .bind(m.name)
        .bind(m.kind())
        .bind(&cs)
        .execute(pg)
        .await?;

        seeded += 1;
        println!("  \u{2713} {} (baseline)", m.name);

        if m.name == up_to {
            break;
        }
    }

    if seeded == 0 {
        warn!("No migrations matched baseline target '{up_to}'");
    } else {
        println!(
            "\n{seeded} migration{} baselined.\n",
            if seeded == 1 { "" } else { "s" }
        );
    }

    Ok(())
}
