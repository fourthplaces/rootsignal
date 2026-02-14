use anyhow::{Context, Result};
use clap::Subcommand;
use std::process::Command;

use crate::repo_root;

#[derive(Subcommand)]
pub enum DbCmd {
    /// Run pending migrations
    Migrate,
    /// Drop, create, and re-run all migrations
    Reset,
    /// Show migration status
    Status,
    /// Open psql using DATABASE_URL
    Psql,
}

fn database_url() -> Result<String> {
    std::env::var("DATABASE_URL").context(
        "DATABASE_URL not set. Create a .env file or export DATABASE_URL.",
    )
}

fn sqlx(args: &[&str]) -> Result<()> {
    let root = repo_root();
    let db_url = database_url()?;
    let status = Command::new("sqlx")
        .args(args)
        .arg("--source")
        .arg(format!("{root}/migrations"))
        .arg("--database-url")
        .arg(&db_url)
        .status()
        .context("Failed to run sqlx. Is sqlx-cli installed? (cargo install sqlx-cli)")?;
    if !status.success() {
        anyhow::bail!("sqlx exited with {}", status);
    }
    Ok(())
}

pub fn exec(cmd: DbCmd) -> Result<()> {
    match cmd {
        DbCmd::Migrate => {
            println!("Running migrations...");
            sqlx(&["migrate", "run"])
        }
        DbCmd::Reset => {
            println!("Resetting database...");
            let db_url = database_url()?;
            // Drop
            let _ = Command::new("sqlx")
                .args(["database", "drop", "-y", "--database-url"])
                .arg(&db_url)
                .status();
            // Create
            let status = Command::new("sqlx")
                .args(["database", "create", "--database-url"])
                .arg(&db_url)
                .status()
                .context("Failed to create database")?;
            if !status.success() {
                anyhow::bail!("sqlx database create failed");
            }
            // Migrate
            sqlx(&["migrate", "run"])
        }
        DbCmd::Status => sqlx(&["migrate", "info"]),
        DbCmd::Psql => {
            let db_url = database_url()?;
            let status = Command::new("psql")
                .arg(&db_url)
                .status()
                .context("Failed to run psql")?;
            if !status.success() {
                anyhow::bail!("psql exited with {}", status);
            }
            Ok(())
        }
    }
}
