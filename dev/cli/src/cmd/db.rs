use anyhow::{Context, Result};
use clap::Subcommand;
use std::path::Path;
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

/// Returns true if any file in `migrations/` is newer than the given binary.
fn migrations_newer_than_binary(root: &str, bin: &Path) -> bool {
    let bin_mtime = match bin.metadata().and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(_) => return true,
    };

    let migrations_dir = Path::new(root).join("migrations");
    let entries = match std::fs::read_dir(&migrations_dir) {
        Ok(e) => e,
        Err(_) => return true,
    };

    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata() {
            if let Ok(mtime) = meta.modified() {
                if mtime > bin_mtime {
                    return true;
                }
            }
        }
    }
    false
}

fn run_migrations_bin() -> Result<()> {
    let root = repo_root();
    let db_url = database_url()?;

    // Try the cargo-built binary first, fall back to sqlx-cli
    let bin_path = format!("{root}/target/release/run-migrations");
    let debug_bin_path = format!("{root}/target/debug/run-migrations");

    // Pick the best available binary
    let chosen_bin = if Path::new(&bin_path).exists() {
        Some(bin_path)
    } else if Path::new(&debug_bin_path).exists() {
        Some(debug_bin_path)
    } else {
        None
    };

    // If migration files are newer than the binary, rebuild so new
    // migrations get embedded via sqlx::migrate!().
    let needs_rebuild = match &chosen_bin {
        Some(bin) => migrations_newer_than_binary(&root, Path::new(bin)),
        None => true,
    };

    if needs_rebuild {
        println!("Migration files changed — rebuilding run-migrations binary...");
        let build_status = Command::new("cargo")
            .args(["build", "--release", "--bin", "run-migrations"])
            .current_dir(&root)
            .status()
            .context("Failed to build run-migrations")?;
        if !build_status.success() {
            anyhow::bail!("cargo build for run-migrations failed");
        }
    }

    // After a potential rebuild, re-resolve the binary path
    let final_bin = if Path::new(&format!("{root}/target/release/run-migrations")).exists() {
        format!("{root}/target/release/run-migrations")
    } else if Path::new(&format!("{root}/target/debug/run-migrations")).exists() {
        format!("{root}/target/debug/run-migrations")
    } else {
        anyhow::bail!("run-migrations binary not found after build");
    };

    let status = Command::new(&final_bin)
        .env("DATABASE_URL", &db_url)
        .status()
        .context("Failed to run run-migrations binary")?;

    if !status.success() {
        anyhow::bail!("run-migrations exited with {}", status);
    }
    Ok(())
}

pub fn exec(cmd: DbCmd) -> Result<()> {
    match cmd {
        DbCmd::Migrate => {
            println!("Running migrations...");
            run_migrations_bin()
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
            run_migrations_bin()
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
