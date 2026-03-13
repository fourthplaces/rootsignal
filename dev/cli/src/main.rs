//! Project-specific development CLI
//!
//! Customize this file to add your own commands and workflows.

use anyhow::Result;
use clap::{Parser, Subcommand};
use devkit_core::AppContext;
use std::process::ExitCode;

mod cmd;
use dialoguer::FuzzySelect;
use dialoguer::Select;

#[derive(Parser)]
#[command(name = "dev")]
#[command(about = "Development environment CLI")]
#[command(version)]
struct Cli {
    /// Run in quiet mode (non-interactive)
    #[arg(short, long, global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start development environment (Docker services)
    Up {
        /// Start all services
        #[arg(short, long)]
        all: bool,

        /// Include optional services (web)
        #[arg(long)]
        full: bool,
    },

    /// Stop all services
    Down {
        /// Remove volumes (WARNING: deletes data)
        #[arg(short, long)]
        volumes: bool,
    },

    /// Show environment status
    Status,

    /// Check system prerequisites
    Doctor,

    /// Run tests and analysis loops
    #[command(subcommand)]
    Test(cmd::test::TestCommand),

    /// Docker service management
    #[command(subcommand)]
    Docker(cmd::docker::DockerCommand),

    /// Run the API server locally (outside Docker)
    #[command(subcommand)]
    Server(cmd::server::ServerCommand),

    /// Run database migrations
    #[command(subcommand)]
    Migrate(cmd::migrate::MigrateCommand),

    /// Run frontend Node.js apps
    #[command(subcommand)]
    Node(cmd::node::NodeCommand),
}

fn main() -> ExitCode {
    // Load environment variables
    let _ = dotenvy::dotenv();

    if let Err(e) = run() {
        eprintln!("Error: {:#}", e);
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let ctx = AppContext::new(cli.quiet)?;

    match cli.command {
        Some(Commands::Up { all, full }) => cmd_up(&ctx, all, full),
        Some(Commands::Down { volumes }) => cmd_down(&ctx, volumes),
        Some(Commands::Status) => cmd_status(&ctx),
        Some(Commands::Doctor) => cmd_doctor(&ctx),
        Some(Commands::Test(cmd)) => cmd::test::run(&ctx, cmd),
        Some(Commands::Docker(cmd)) => cmd::docker::run(&ctx, cmd),
        Some(Commands::Server(cmd)) => cmd::server::run(&ctx, cmd),
        Some(Commands::Migrate(cmd)) => cmd::migrate::run(&ctx, cmd),
        Some(Commands::Node(cmd)) => cmd::node::run(&ctx, cmd),
        None => interactive_menu(&ctx),
    }
}

fn cmd_up(ctx: &AppContext, all: bool, full: bool) -> Result<()> {
    ctx.print_header("Starting development environment");

    ctx.print_info("Starting services...");
    cmd::docker::run(
        ctx,
        cmd::docker::DockerCommand::Up {
            services: vec![],
            all,
            full,
            detach: true,
        },
    )?;

    ctx.print_success("Development environment is ready!");
    println!();
    ctx.print_info("Run 'dev docker status' to see service URLs");

    Ok(())
}

fn cmd_down(ctx: &AppContext, volumes: bool) -> Result<()> {
    ctx.print_header("Stopping development environment");

    cmd::docker::run(
        ctx,
        cmd::docker::DockerCommand::Down {
            services: vec![],
            volumes,
        },
    )
}

fn cmd_status(ctx: &AppContext) -> Result<()> {
    ctx.print_header("Development Environment Status");
    println!();
    println!("Repository: {}", ctx.repo.display());
    println!("Project: {}", ctx.config.global.project.name);
    println!();
    cmd::docker::run(ctx, cmd::docker::DockerCommand::Status)?;
    Ok(())
}

fn cmd_doctor(ctx: &AppContext) -> Result<()> {
    ctx.print_header("System Health Check");
    println!();

    let tools = vec![
        ("git", devkit_core::utils::cmd_exists("git")),
        ("cargo", devkit_core::utils::cmd_exists("cargo")),
        ("docker", devkit_core::utils::docker_available()),
    ];

    for (tool, available) in tools {
        if available {
            ctx.print_success(&format!("✓ {}", tool));
        } else {
            ctx.print_warning(&format!("✗ {} (not found)", tool));
        }
    }

    println!();
    ctx.print_success("Health check complete");
    Ok(())
}

fn interactive_menu(ctx: &AppContext) -> Result<()> {

    let items = vec![
        "🚀 Server (run API locally)",
        "🗃️  Migrate (database migrations)",
        "📦 Node (frontend apps) →",
        "🐳 Docker →",
        "🧪 Test →",
        "📊 Status",
        "🩺 Doctor",
        "❌ Exit",
    ];

    loop {
        println!();
        let choice = FuzzySelect::with_theme(&ctx.theme())
            .with_prompt("What would you like to do?")
            .items(&items)
            .default(0)
            .interact()?;

        match choice {
            0 => server_submenu(ctx)?,
            1 => migrate_submenu(ctx)?,
            2 => cmd::node::interactive_menu(ctx)?,
            3 => docker_submenu(ctx)?,
            4 => cmd::test::interactive_menu(ctx)?,
            5 => cmd_status(ctx)?,
            6 => cmd_doctor(ctx)?,
            _ => break,
        }
    }

    Ok(())
}

fn server_submenu(ctx: &AppContext) -> Result<()> {
    let items = vec![
        "Run API server",
        "Run API server (REPLAY mode)",
        "← Back",
    ];

    let choice = Select::with_theme(&ctx.theme())
        .with_prompt("Server")
        .items(&items)
        .default(0)
        .interact()?;

    match choice {
        0 => cmd::server::run(ctx, cmd::server::ServerCommand::Run { replay: false }),
        1 => cmd::server::run(ctx, cmd::server::ServerCommand::Run { replay: true }),
        _ => Ok(()),
    }
}

fn migrate_submenu(ctx: &AppContext) -> Result<()> {
    let items = vec![
        "Status (dry run — show pending)",
        "Commit (apply pending migrations)",
        "Check (lint SQL for risky patterns)",
        "Mark completed (skip specific migrations)",
        "← Back",
    ];

    let choice = Select::with_theme(&ctx.theme())
        .with_prompt("Migrate")
        .items(&items)
        .default(0)
        .interact()?;

    match choice {
        0 => cmd::migrate::run(ctx, cmd::migrate::MigrateCommand::Status),
        1 => cmd::migrate::run(ctx, cmd::migrate::MigrateCommand::Commit),
        2 => cmd::migrate::run(ctx, cmd::migrate::MigrateCommand::Check),
        3 => {
            let name: String = dialoguer::Input::with_theme(&ctx.theme())
                .with_prompt("Migration names (comma-separated)")
                .interact_text()?;
            cmd::migrate::run(ctx, cmd::migrate::MigrateCommand::MarkCompleted { names: name })
        }
        _ => Ok(()),
    }
}

fn docker_submenu(ctx: &AppContext) -> Result<()> {

    let items = vec![
        "Start services",
        "Stop services",
        "Restart services",
        "Rebuild images",
        "Rebuild images (clean, from scratch)",
        "Follow logs",
        "Status",
        "Shell into container",
        "Neo4j console",
        "← Back",
    ];

    let choice = Select::with_theme(&ctx.theme())
        .with_prompt("Docker")
        .items(&items)
        .default(0)
        .interact()?;

    match choice {
        0 => cmd::docker::run(
            ctx,
            cmd::docker::DockerCommand::Up {
                services: vec![],
                all: false,
                full: false,
                detach: true,
            },
        ),
        1 => cmd::docker::run(
            ctx,
            cmd::docker::DockerCommand::Down {
                services: vec![],
                volumes: false,
            },
        ),
        2 => cmd::docker::run(
            ctx,
            cmd::docker::DockerCommand::Restart {
                services: vec![],
                all: false,
            },
        ),
        3 => cmd::docker::run(
            ctx,
            cmd::docker::DockerCommand::Build {
                services: vec![],
                all: false,
                no_cache: false,
                clean: false,
                pull: false,
            },
        ),
        4 => cmd::docker::run(
            ctx,
            cmd::docker::DockerCommand::Build {
                services: vec![],
                all: false,
                no_cache: true,
                clean: true,
                pull: true,
            },
        ),
        5 => cmd::docker::run(
            ctx,
            cmd::docker::DockerCommand::Logs {
                services: vec![],
                all: false,
                tail: "100".to_string(),
                no_follow: false,
            },
        ),
        6 => cmd::docker::run(ctx, cmd::docker::DockerCommand::Status),
        7 => cmd::docker::run(ctx, cmd::docker::DockerCommand::Shell { service: None }),
        8 => cmd::docker::run(ctx, cmd::docker::DockerCommand::Cypher),
        _ => Ok(()),
    }
}

