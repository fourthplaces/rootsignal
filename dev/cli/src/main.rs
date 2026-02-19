//! Project-specific development CLI
//!
//! Customize this file to add your own commands and workflows.

use anyhow::Result;
use clap::{Parser, Subcommand};
use devkit_core::AppContext;
use std::process::ExitCode;

mod cmd;

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

    /// Run scout to discover civic signals
    Scout,

    /// Run scout integration tests (fixture-driven, ~$1-2)
    TestScout {
        /// Run a specific test by name (substring match)
        #[arg(short, long)]
        filter: Option<String>,
    },

    /// Run sim integration tests (LLM-generated worlds, ~$5-10)
    TestSim {
        /// Run a specific scenario by name
        #[arg(short, long)]
        filter: Option<String>,
    },

    /// Run improvement loop (blind spot analysis, ~$5-10)
    Improve,

    /// Run prompt evolution loop (autonomous overnight improvement)
    Evolve {
        /// Max generations to run
        #[arg(short, long, default_value = "3")]
        generations: u32,

        /// Mutations per generation
        #[arg(short, long, default_value = "2")]
        mutations: u32,
    },

    /// Docker service management
    #[command(subcommand)]
    Docker(cmd::docker::DockerCommand),
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
        Some(Commands::Scout) => cmd_scout(&ctx),
        Some(Commands::TestScout { filter }) => cmd_test_scout(&ctx, filter.as_deref()),
        Some(Commands::TestSim { filter }) => cmd_test_sim(&ctx, filter.as_deref()),
        Some(Commands::Improve) => cmd_improve(&ctx),
        Some(Commands::Evolve { generations, mutations }) => cmd_evolve(&ctx, generations, mutations),
        Some(Commands::Status) => cmd_status(&ctx),
        Some(Commands::Doctor) => cmd_doctor(&ctx),
        Some(Commands::Docker(cmd)) => cmd::docker::run(&ctx, cmd),
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

fn cmd_scout(ctx: &AppContext) -> Result<()> {
    cmd::docker::run(ctx, cmd::docker::DockerCommand::Scout)
}

fn cmd_test_scout(ctx: &AppContext, filter: Option<&str>) -> Result<()> {
    ctx.print_header("Scout Integration Tests (fixture-driven)");
    println!();
    ctx.print_info("Runs 20 scenarios with fixture content + real LLM + real Neo4j (~$1-2)");
    println!();

    let mut args = vec![
        "test", "-p", "rootsignal-scout",
        "--test", "scout_integration",
        "--", "--nocapture",
    ];
    let filter_owned;
    if let Some(f) = filter {
        filter_owned = f.to_string();
        args.push(&filter_owned);
    }

    let status = std::process::Command::new("cargo")
        .args(&args)
        .current_dir(&ctx.repo)
        .status()?;

    println!();
    if status.success() {
        ctx.print_success("All scout integration tests passed!");
    } else {
        ctx.print_warning("Some tests failed — check output above");
    }
    Ok(())
}

fn cmd_test_sim(ctx: &AppContext, filter: Option<&str>) -> Result<()> {
    ctx.print_header("Sim Integration Tests (LLM-generated worlds)");
    println!();
    ctx.print_info("Runs 8 world scenarios with simulated web + judge + audit (~$5-10)");
    println!();

    let mut args = vec![
        "test", "-p", "rootsignal-scout",
        "--test", "sim_integration",
        "--", "--nocapture",
    ];
    let filter_owned;
    if let Some(f) = filter {
        filter_owned = f.to_string();
        args.push(&filter_owned);
    }

    let status = std::process::Command::new("cargo")
        .args(&args)
        .current_dir(&ctx.repo)
        .status()?;

    println!();
    if status.success() {
        ctx.print_success("All sim integration tests passed!");
    } else {
        ctx.print_warning("Some tests failed — check output above");
    }
    Ok(())
}

fn cmd_improve(ctx: &AppContext) -> Result<()> {
    ctx.print_header("Improvement Loop (blind spot analysis)");
    println!();
    ctx.print_info("Runs all scenarios, collects failures, generates blind spot report (~$5-10)");
    println!();

    let status = std::process::Command::new("cargo")
        .args([
            "test", "-p", "rootsignal-scout",
            "--test", "sim_integration",
            "improvement_loop",
            "--", "--nocapture",
        ])
        .env("IMPROVEMENT_LOOP", "1")
        .current_dir(&ctx.repo)
        .status()?;

    println!();
    if status.success() {
        ctx.print_success("Improvement analysis complete!");
        ctx.print_info("Check tests/run_logs/improvement_report.json for results");
    } else {
        ctx.print_warning("Improvement loop failed — check output above");
    }
    Ok(())
}

fn cmd_evolve(ctx: &AppContext, generations: u32, mutations: u32) -> Result<()> {
    ctx.print_header("Prompt Evolution Loop");
    println!();
    ctx.print_info(&format!(
        "Running {} generations × {} mutations per generation",
        generations, mutations
    ));
    ctx.print_info("This will cost ~$3-5 in API calls and take 30-60 minutes.");
    println!();

    // Ensure Docker is running (need Neo4j for test harness)
    ctx.print_info("Checking Docker services...");
    let docker_status = std::process::Command::new("docker")
        .args(["compose", "ps", "--format", "{{.Name}}"])
        .current_dir(&ctx.repo)
        .output();

    match docker_status {
        Ok(output) if output.status.success() => {
            let running = String::from_utf8_lossy(&output.stdout);
            if !running.contains("neo4j") {
                ctx.print_warning("Neo4j doesn't appear to be running. Start it with 'dev up' first.");
                return Ok(());
            }
        }
        _ => {
            ctx.print_warning("Could not check Docker status. Make sure services are running.");
        }
    }

    let status = std::process::Command::new("cargo")
        .args([
            "test", "-p", "rootsignal-scout",
            "evolution_loop",
            "--", "--nocapture",
        ])
        .env("EVOLUTION_LOOP", "1")
        .env("EVOLUTION_GENERATIONS", generations.to_string())
        .env("EVOLUTION_MUTATIONS", mutations.to_string())
        .current_dir(&ctx.repo)
        .status()?;

    println!();
    if status.success() {
        ctx.print_success("Evolution complete!");
        ctx.print_info("Check tests/evolution/champion.json for results");
        ctx.print_info("Diff the champion prompt vs baseline to see mutations");
    } else {
        ctx.print_warning("Evolution run failed — check output above");
    }

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
    use dialoguer::FuzzySelect;

    let items = vec![
        "🚀 Start environment (up)",
        "🛑 Stop environment (down)",
        "🔍 Run scout (investigate)",
        "🧪 Test scout →",
        "🐳 Docker services →",
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
            0 => cmd_up(ctx, false, false)?,
            1 => cmd_down(ctx, false)?,
            2 => cmd_scout(ctx)?,
            3 => test_submenu(ctx)?,
            4 => docker_submenu(ctx)?,
            5 => cmd_status(ctx)?,
            6 => cmd_doctor(ctx)?,
            _ => break,
        }
    }

    Ok(())
}

fn test_submenu(ctx: &AppContext) -> Result<()> {
    use dialoguer::Select;

    let items = vec![
        "🔬 Scout integration (fixtures, ~$1-2)",
        "🌐 Sim integration (8 worlds, ~$5-10)",
        "🔎 Improvement loop (blind spots, ~$5-10)",
        "🧬 Evolution loop (prompt mutation, ~$3-12)",
        "🎲 Random discovery (1 random world, informational)",
        "← Back",
    ];

    let choice = Select::with_theme(&ctx.theme())
        .with_prompt("Test scout")
        .items(&items)
        .default(0)
        .interact()?;

    match choice {
        0 => test_scout_interactive(ctx),
        1 => test_sim_interactive(ctx),
        2 => cmd_improve(ctx),
        3 => evolve_interactive(ctx),
        4 => cmd_random_discovery(ctx),
        _ => Ok(()),
    }
}

fn test_scout_interactive(ctx: &AppContext) -> Result<()> {
    use dialoguer::Select;

    let items = vec![
        "Run all 20 scenarios",
        "Extraction (event, give, ask, notice, tension)",
        "Dedup & corroboration",
        "Adversarial (spam, astroturf, coordinated)",
        "Multi-city (Portland, NYC, Berlin)",
        "← Back",
    ];

    let choice = Select::with_theme(&ctx.theme())
        .with_prompt("Scout integration tests")
        .items(&items)
        .default(0)
        .interact()?;

    let filter = match choice {
        0 => None,
        1 => Some("event_page\x00resource_page\x00gofundme\x00government_advisory\x00tension_extraction"),
        2 => Some("duplicate_content\x00overlapping_content\x00corroborated\x00conflicting"),
        3 => Some("spam\x00astroturf\x00coordinated"),
        4 => Some("portland\x00nyc\x00berlin"),
        _ => return Ok(()),
    };

    // cargo test doesn't support OR filters, so for subsets we run each individually
    if let Some(filter_str) = filter {
        if filter_str.contains('\x00') {
            // Multiple filters — run each
            for f in filter_str.split('\x00') {
                ctx.print_info(&format!("Running: {f}"));
                cmd_test_scout(ctx, Some(f))?;
            }
            Ok(())
        } else {
            cmd_test_scout(ctx, Some(filter_str))
        }
    } else {
        cmd_test_scout(ctx, None)
    }
}

fn test_sim_interactive(ctx: &AppContext) -> Result<()> {
    use dialoguer::Select;

    let items = vec![
        "Run all 8 scenarios",
        "stale_minneapolis",
        "organizing_portland",
        "simmering_cedar_riverside",
        "rural_minnesota",
        "hidden_civic_minneapolis",
        "shifting_ground",
        "tension_response_cycle",
        "tension_discovery_bridge",
        "← Back",
    ];

    let choice = Select::with_theme(&ctx.theme())
        .with_prompt("Sim integration tests")
        .items(&items)
        .default(0)
        .interact()?;

    let filter = match choice {
        0 => None,
        1 => Some("sim_stale_minneapolis"),
        2 => Some("sim_organizing_portland"),
        3 => Some("sim_simmering_cedar_riverside"),
        4 => Some("sim_rural_minnesota"),
        5 => Some("sim_hidden_civic_minneapolis"),
        6 => Some("sim_shifting_ground"),
        7 => Some("sim_tension_response_cycle"),
        8 => Some("sim_tension_discovery_bridge"),
        _ => return Ok(()),
    };

    cmd_test_sim(ctx, filter)
}

fn cmd_random_discovery(ctx: &AppContext) -> Result<()> {
    ctx.print_header("Random World Discovery (informational)");
    println!();
    ctx.print_info("Generates a random world and runs scout against it. Never fails CI.");
    println!();

    let status = std::process::Command::new("cargo")
        .args([
            "test", "-p", "rootsignal-scout",
            "--test", "sim_integration",
            "discovery_random_world",
            "--", "--nocapture",
        ])
        .current_dir(&ctx.repo)
        .status()?;

    println!();
    if status.success() {
        ctx.print_success("Random discovery complete!");
    } else {
        ctx.print_warning("Random discovery had issues — check output above");
    }
    Ok(())
}

fn evolve_interactive(ctx: &AppContext) -> Result<()> {
    use dialoguer::{Input, Select};

    let presets = vec![
        "Quick (1 gen × 1 mut, ~$1)",
        "Standard (3 gen × 2 mut, ~$3-5)",
        "Deep (5 gen × 3 mut, ~$8-12)",
        "Custom",
    ];

    let choice = Select::with_theme(&ctx.theme())
        .with_prompt("Evolution intensity")
        .items(&presets)
        .default(1)
        .interact()?;

    let (generations, mutations) = match choice {
        0 => (1, 1),
        1 => (3, 2),
        2 => (5, 3),
        _ => {
            let g: u32 = Input::with_theme(&ctx.theme())
                .with_prompt("Generations")
                .default(3)
                .interact_text()?;
            let m: u32 = Input::with_theme(&ctx.theme())
                .with_prompt("Mutations per generation")
                .default(2)
                .interact_text()?;
            (g, m)
        }
    };

    cmd_evolve(ctx, generations, mutations)
}

fn docker_submenu(ctx: &AppContext) -> Result<()> {
    use dialoguer::Select;

    let items = vec![
        "Start services",
        "Stop services",
        "Restart services",
        "Rebuild images",
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
            },
        ),
        4 => cmd::docker::run(
            ctx,
            cmd::docker::DockerCommand::Logs {
                services: vec![],
                all: false,
                tail: "100".to_string(),
                no_follow: false,
            },
        ),
        5 => cmd::docker::run(ctx, cmd::docker::DockerCommand::Status),
        6 => cmd::docker::run(ctx, cmd::docker::DockerCommand::Shell { service: None }),
        7 => cmd::docker::run(ctx, cmd::docker::DockerCommand::Cypher),
        _ => Ok(()),
    }
}
