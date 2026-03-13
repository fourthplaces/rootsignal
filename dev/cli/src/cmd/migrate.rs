//! Database migration commands

use anyhow::{Context, Result};
use clap::Subcommand;
use devkit_core::AppContext;
use dialoguer::{Confirm, Select};

#[derive(Subcommand)]
pub enum MigrateCommand {
    /// Show pending migrations (dry run, default)
    Status,

    /// Apply pending migrations
    Commit,

    /// Lint SQL migrations for risky patterns
    Check,

    /// Seed migrations up to a name as already applied
    Baseline {
        /// Migration name to baseline to
        target: String,
    },

    /// Mark specific migrations as completed without running them
    MarkCompleted {
        /// Migration names (comma-separated)
        names: String,
    },
}

pub fn run(ctx: &AppContext, cmd: MigrateCommand) -> Result<()> {
    match cmd {
        MigrateCommand::Status => run_migrate(ctx, &[]),
        MigrateCommand::Commit => run_commit_interactive(ctx),
        MigrateCommand::Check => run_migrate(ctx, &["--check"]),
        MigrateCommand::Baseline { ref target } => run_migrate(ctx, &["--baseline", target]),
        MigrateCommand::MarkCompleted { ref names } => {
            run_migrate(ctx, &["--mark-completed", names])
        }
    }
}

/// Run `--commit` with interactive recovery: if a migration fails, offer to
/// mark it as completed and retry.
fn run_commit_interactive(ctx: &AppContext) -> Result<()> {
    use std::process::Stdio;

    loop {
        ctx.print_header("Database Migrations");
        println!();

        // Inherit stdout for real-time progress, pipe stderr to parse errors
        let child = std::process::Command::new("cargo")
            .args(["run", "--bin", "rootsignal-migrate", "--", "--commit"])
            .current_dir(&ctx.repo)
            .stdout(Stdio::inherit())
            .stderr(Stdio::piped())
            .output()
            .context("Failed to run rootsignal-migrate")?;

        let stderr = String::from_utf8_lossy(&child.stderr);

        if child.status.success() {
            return Ok(());
        }

        // Print error lines, skip cargo build noise
        for line in stderr.lines() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with("Compiling")
                && !trimmed.starts_with("Finished")
                && !trimmed.starts_with("Running")
                && !trimmed.starts_with("warning:")
                && !trimmed.is_empty()
            {
                eprintln!("{line}");
            }
        }

        let failed_name = extract_failed_migration(&stderr);

        let Some(name) = failed_name else {
            anyhow::bail!("Migration command failed");
        };

        println!();
        let items = vec![
            format!("Mark '{name}' as completed and retry"),
            "Abort".to_string(),
        ];

        let choice = Select::with_theme(&ctx.theme())
            .with_prompt(format!("Migration '{name}' failed"))
            .items(&items)
            .default(0)
            .interact()?;

        if choice != 0 {
            anyhow::bail!("Migration aborted by user");
        }

        let confirmed = Confirm::with_theme(&ctx.theme())
            .with_prompt(format!(
                "Record '{name}' as completed without running it?"
            ))
            .default(false)
            .interact()?;

        if !confirmed {
            anyhow::bail!("Migration aborted by user");
        }

        let mark_status = std::process::Command::new("cargo")
            .args([
                "run",
                "--bin",
                "rootsignal-migrate",
                "--",
                "--mark-completed",
                &name,
            ])
            .current_dir(&ctx.repo)
            .status()
            .context("Failed to run rootsignal-migrate --mark-completed")?;

        if !mark_status.success() {
            anyhow::bail!("Failed to mark migration '{name}' as completed");
        }

        println!();
        ctx.print_info("Retrying remaining migrations...");
    }
}

/// Extract migration name from error output like "migration 004_content_type_tables failed"
fn extract_failed_migration(output: &str) -> Option<String> {
    let marker = "migration ";
    let suffix = " failed";
    for line in output.lines() {
        if let Some(start) = line.find(marker) {
            let after = &line[start + marker.len()..];
            if let Some(end) = after.find(suffix) {
                let name = after[..end].trim();
                if !name.is_empty() && !name.contains(' ') {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}

fn run_migrate(ctx: &AppContext, extra_args: &[&str]) -> Result<()> {
    ctx.print_header("Database Migrations");
    println!();

    let mut cmd = std::process::Command::new("cargo");
    cmd.args(["run", "--bin", "rootsignal-migrate", "--"]);
    cmd.args(extra_args);
    cmd.current_dir(&ctx.repo);

    let status = cmd.status().context("Failed to run rootsignal-migrate")?;

    if !status.success() {
        anyhow::bail!("Migration command failed");
    }

    Ok(())
}
