//! Database migration commands

use anyhow::{Context, Result};
use clap::Subcommand;
use devkit_core::AppContext;

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
}

pub fn run(ctx: &AppContext, cmd: MigrateCommand) -> Result<()> {
    match cmd {
        MigrateCommand::Status => run_migrate(ctx, &[]),
        MigrateCommand::Commit => run_migrate(ctx, &["--commit"]),
        MigrateCommand::Check => run_migrate(ctx, &["--check"]),
        MigrateCommand::Baseline { ref target } => run_migrate(ctx, &["--baseline", target]),
    }
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
