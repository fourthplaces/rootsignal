//! Local server commands (cargo run, not Docker)

use anyhow::{Context, Result};
use clap::Subcommand;
use devkit_core::AppContext;

#[derive(Subcommand)]
pub enum ServerCommand {
    /// Run the API server locally
    Run {
        /// Run in replay mode (REPLAY=1): replay all events to fresh Neo4j DB, then exit
        #[arg(long)]
        replay: bool,
    },
}

pub fn run(ctx: &AppContext, cmd: ServerCommand) -> Result<()> {
    match cmd {
        ServerCommand::Run { replay } => run_server(ctx, replay),
    }
}

fn run_server(ctx: &AppContext, replay: bool) -> Result<()> {
    if replay {
        ctx.print_header("Running API server in REPLAY mode");
        ctx.print_info("Replays all events → fresh Neo4j DB → health check → promote → exit");
    } else {
        ctx.print_header("Running API server");
    }
    println!();

    let mut cmd = std::process::Command::new("cargo");
    cmd.args(["run", "--bin", "rootsignal-api"]);
    cmd.current_dir(&ctx.repo);

    if replay {
        cmd.env("REPLAY", "1");
    }

    let status = cmd.status().context("Failed to run API server")?;

    if !status.success() {
        anyhow::bail!("API server exited with error");
    }

    Ok(())
}
