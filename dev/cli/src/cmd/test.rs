//! Test commands for running integration tests and analysis loops

use anyhow::Result;
use clap::Subcommand;
use devkit_core::AppContext;
use dialoguer::Select;
use dialoguer::{Input, Select};

#[derive(Subcommand)]
pub enum TestCommand {
    /// Run scout integration tests (fixture-driven, ~$1-2)
    Scout {
        /// Run a specific test by name (substring match)
        #[arg(short, long)]
        filter: Option<String>,
    },

    /// Run sim integration tests (LLM-generated worlds, ~$5-10)
    Sim {
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

    /// Run a single random world discovery (informational, never fails)
    Random,
}

pub fn run(ctx: &AppContext, cmd: TestCommand) -> Result<()> {
    match cmd {
        TestCommand::Scout { filter } => run_scout(ctx, filter.as_deref()),
        TestCommand::Sim { filter } => run_sim(ctx, filter.as_deref()),
        TestCommand::Improve => run_improve(ctx),
        TestCommand::Evolve {
            generations,
            mutations,
        } => run_evolve(ctx, generations, mutations),
        TestCommand::Random => run_random(ctx),
    }
}

fn run_scout(ctx: &AppContext, filter: Option<&str>) -> Result<()> {
    ctx.print_header("Scout Integration Tests (fixture-driven)");
    println!();
    ctx.print_info("Runs 20 scenarios with fixture content + real LLM + real Neo4j (~$1-2)");
    println!();

    let mut args = vec![
        "test",
        "-p",
        "rootsignal-scout",
        "--test",
        "scout_integration",
        "--",
        "--nocapture",
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

fn run_sim(ctx: &AppContext, filter: Option<&str>) -> Result<()> {
    ctx.print_header("Sim Integration Tests (LLM-generated worlds)");
    println!();
    ctx.print_info("Runs 8 world scenarios with simulated web + judge + audit (~$5-10)");
    println!();

    let mut args = vec![
        "test",
        "-p",
        "rootsignal-scout",
        "--test",
        "sim_integration",
        "--",
        "--nocapture",
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

fn run_improve(ctx: &AppContext) -> Result<()> {
    ctx.print_header("Improvement Loop (blind spot analysis)");
    println!();
    ctx.print_info("Runs all scenarios, collects failures, generates blind spot report (~$5-10)");
    println!();

    let status = std::process::Command::new("cargo")
        .args([
            "test",
            "-p",
            "rootsignal-scout",
            "--test",
            "sim_integration",
            "improvement_loop",
            "--",
            "--nocapture",
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

fn run_evolve(ctx: &AppContext, generations: u32, mutations: u32) -> Result<()> {
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
                ctx.print_warning(
                    "Neo4j doesn't appear to be running. Start it with 'dev up' first.",
                );
                return Ok(());
            }
        }
        _ => {
            ctx.print_warning("Could not check Docker status. Make sure services are running.");
        }
    }

    let status = std::process::Command::new("cargo")
        .args([
            "test",
            "-p",
            "rootsignal-scout",
            "evolution_loop",
            "--",
            "--nocapture",
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

fn run_random(ctx: &AppContext) -> Result<()> {
    ctx.print_header("Random World Discovery (informational)");
    println!();
    ctx.print_info("Generates a random world and runs scout against it. Never fails CI.");
    println!();

    let status = std::process::Command::new("cargo")
        .args([
            "test",
            "-p",
            "rootsignal-scout",
            "--test",
            "sim_integration",
            "discovery_random_world",
            "--",
            "--nocapture",
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

/// Interactive test menu (called from the main interactive menu)
pub fn interactive_menu(ctx: &AppContext) -> Result<()> {

    let items = vec![
        "Scout integration (fixtures, ~$1-2) →",
        "Sim integration (8 worlds, ~$5-10) →",
        "Improvement loop (blind spots, ~$5-10)",
        "Evolution loop (prompt mutation, ~$3-12) →",
        "Random discovery (1 random world)",
        "← Back",
    ];

    let choice = Select::with_theme(&ctx.theme())
        .with_prompt("Test")
        .items(&items)
        .default(0)
        .interact()?;

    match choice {
        0 => scout_interactive(ctx),
        1 => sim_interactive(ctx),
        2 => run_improve(ctx),
        3 => evolve_interactive(ctx),
        4 => run_random(ctx),
        _ => Ok(()),
    }
}

fn scout_interactive(ctx: &AppContext) -> Result<()> {

    let items = vec![
        "Run all 20 scenarios",
        "Extraction (gathering, aid, need, notice, tension)",
        "Dedup & corroboration",
        "Adversarial (spam, astroturf, coordinated)",
        "Multi-region (Portland, NYC, Berlin)",
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
            for f in filter_str.split('\x00') {
                ctx.print_info(&format!("Running: {f}"));
                run_scout(ctx, Some(f))?;
            }
            Ok(())
        } else {
            run_scout(ctx, Some(filter_str))
        }
    } else {
        run_scout(ctx, None)
    }
}

fn sim_interactive(ctx: &AppContext) -> Result<()> {

    let items = vec![
        "Run all 8 scenarios",
        "stale_minneapolis",
        "organizing_portland",
        "simmering_cedar_riverside",
        "rural_minnesota",
        "hidden_community_minneapolis",
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
        5 => Some("sim_hidden_community_minneapolis"),
        6 => Some("sim_shifting_ground"),
        7 => Some("sim_tension_response_cycle"),
        8 => Some("sim_tension_discovery_bridge"),
        _ => return Ok(()),
    };

    run_sim(ctx, filter)
}

fn evolve_interactive(ctx: &AppContext) -> Result<()> {

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

    run_evolve(ctx, generations, mutations)
}

