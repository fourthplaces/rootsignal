//! Docker commands for managing development services

use anyhow::{Context, Result};
use clap::Subcommand;
use console::style;
use devkit_core::AppContext;
use dialoguer::{FuzzySelect, MultiSelect};
use std::process::Command;

#[derive(Debug, Clone)]
struct ServiceInfo {
    name: String,
    description: String,
    buildable: bool,
    shell: String,
}

/// Parse docker-compose.yml via `docker compose config --format json`
fn discover_services(ctx: &AppContext) -> Result<Vec<ServiceInfo>> {
    let output = Command::new("docker")
        .args(["compose", "-f"])
        .arg(ctx.repo.join("docker-compose.yml"))
        .args(["config", "--format", "json"])
        .output()
        .context("Failed to run docker compose config")?;

    if !output.status.success() {
        anyhow::bail!(
            "docker compose config failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let config: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let services_map = config["services"]
        .as_object()
        .context("No services in docker-compose.yml")?;

    let mut services: Vec<ServiceInfo> = services_map
        .iter()
        .map(|(name, svc)| {
            let labels = &svc["labels"];
            let description = labels["dev.description"]
                .as_str()
                .unwrap_or_else(|| svc["image"].as_str().unwrap_or(""))
                .to_string();
            let buildable = svc.get("build").is_some();
            let shell = labels["dev.shell"]
                .as_str()
                .unwrap_or("sh")
                .to_string();
            ServiceInfo {
                name: name.clone(),
                description,
                buildable,
                shell,
            }
        })
        .collect();

    services.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(services)
}

#[derive(Subcommand)]
pub enum DockerCommand {
    /// Start services
    Up {
        /// Services to start (omit for interactive selection)
        #[arg(value_name = "SERVICE")]
        services: Vec<String>,

        /// Start all services
        #[arg(short, long)]
        all: bool,

        /// Include optional services (web)
        #[arg(long)]
        full: bool,

        /// Run in detached mode
        #[arg(short, long, default_value = "true")]
        detach: bool,
    },

    /// Stop services
    Down {
        /// Services to stop (omit for all)
        #[arg(value_name = "SERVICE")]
        services: Vec<String>,

        /// Remove volumes (WARNING: deletes data)
        #[arg(short, long)]
        volumes: bool,
    },

    /// Restart services
    Restart {
        /// Services to restart (omit for interactive selection)
        #[arg(value_name = "SERVICE")]
        services: Vec<String>,

        /// Restart all services
        #[arg(short, long)]
        all: bool,
    },

    /// Rebuild service images
    Build {
        /// Services to build (omit for interactive selection)
        #[arg(value_name = "SERVICE")]
        services: Vec<String>,

        /// Build all services
        #[arg(short, long)]
        all: bool,

        /// Don't use cache
        #[arg(long)]
        no_cache: bool,
    },

    /// Follow logs from services
    Logs {
        /// Services to follow (omit for interactive selection)
        #[arg(value_name = "SERVICE")]
        services: Vec<String>,

        /// Follow all services
        #[arg(short, long)]
        all: bool,

        /// Number of lines to show initially
        #[arg(short = 'n', long, default_value = "100")]
        tail: String,

        /// Don't follow, just show recent logs
        #[arg(long)]
        no_follow: bool,
    },

    /// Show status of all services
    Status,

    /// Open a shell in a service container
    Shell {
        /// Service to open shell in
        service: Option<String>,
    },

    /// Run cypher-shell in the neo4j container
    Cypher,

    /// Run scout to discover civic signals
    Scout,
}

pub fn run(ctx: &AppContext, cmd: DockerCommand) -> Result<()> {
    match cmd {
        DockerCommand::Up {
            services,
            all,
            full,
            detach,
        } => run_up(ctx, services, all, full, detach),
        DockerCommand::Down { services, volumes } => run_down(ctx, services, volumes),
        DockerCommand::Restart { services, all } => run_restart(ctx, services, all),
        DockerCommand::Build {
            services,
            all,
            no_cache,
        } => run_build(ctx, services, all, no_cache),
        DockerCommand::Logs {
            services,
            all,
            tail,
            no_follow,
        } => run_logs(ctx, services, all, &tail, no_follow),
        DockerCommand::Status => run_status(ctx),
        DockerCommand::Shell { service } => run_shell(ctx, service),
        DockerCommand::Cypher => run_cypher(ctx),
        DockerCommand::Scout => run_scout(ctx),
    }
}

fn select_services(
    ctx: &AppContext,
    all_services: &[ServiceInfo],
    prompt: &str,
    allow_all: bool,
) -> Result<Vec<String>> {
    if ctx.quiet {
        return Ok(all_services.iter().map(|s| s.name.clone()).collect());
    }

    let items: Vec<String> = all_services
        .iter()
        .map(|s| format!("{} - {}", s.name, s.description))
        .collect();

    let mut items_with_all = items.clone();
    if allow_all {
        items_with_all.insert(0, "All services".to_string());
    }

    let selections = MultiSelect::with_theme(&ctx.theme())
        .with_prompt(prompt)
        .items(&items_with_all)
        .interact()?;

    if selections.is_empty() {
        anyhow::bail!("No services selected");
    }

    if allow_all && selections.contains(&0) {
        return Ok(all_services.iter().map(|s| s.name.clone()).collect());
    }

    let offset = if allow_all { 1 } else { 0 };
    Ok(selections
        .into_iter()
        .filter(|&i| i >= offset)
        .map(|i| all_services[i - offset].name.clone())
        .collect())
}

fn select_single_service(
    ctx: &AppContext,
    all_services: &[ServiceInfo],
    prompt: &str,
) -> Result<String> {
    if ctx.quiet {
        anyhow::bail!("Service selection requires interactive mode");
    }

    let items: Vec<String> = all_services
        .iter()
        .map(|s| format!("{} - {}", s.name, s.description))
        .collect();

    let selection = FuzzySelect::with_theme(&ctx.theme())
        .with_prompt(prompt)
        .items(&items)
        .default(0)
        .interact()?;

    Ok(all_services[selection].name.clone())
}

fn docker_compose(ctx: &AppContext) -> Command {
    let mut cmd = Command::new("docker");
    cmd.args(["compose", "-f"]);
    cmd.arg(ctx.repo.join("docker-compose.yml"));
    cmd
}

fn run_up(
    ctx: &AppContext,
    services: Vec<String>,
    all: bool,
    full: bool,
    detach: bool,
) -> Result<()> {
    let all_services = discover_services(ctx)?;

    let services = if all {
        all_services
            .iter()
            .filter(|s| s.name != "web" || full)
            .map(|s| s.name.clone())
            .collect()
    } else if services.is_empty() {
        select_services(ctx, &all_services, "Select services to start", true)?
    } else {
        services
    };

    ctx.print_header("Starting services");
    for svc in &services {
        println!("  • {}", style(svc).cyan());
    }
    println!();

    let mut cmd = docker_compose(ctx);
    if full {
        cmd.args(["--profile", "full"]);
    }
    cmd.arg("up");
    if detach {
        cmd.arg("-d");
    }
    cmd.args(&services);

    let status = cmd.status().context("Failed to run docker compose")?;

    if status.success() {
        ctx.print_success("Services started");
    } else {
        anyhow::bail!("Failed to start services");
    }

    Ok(())
}

fn run_down(ctx: &AppContext, services: Vec<String>, volumes: bool) -> Result<()> {
    ctx.print_header("Stopping services");

    if volumes {
        ctx.print_warning("WARNING: This will delete all data volumes!");
    }

    let mut cmd = docker_compose(ctx);
    cmd.arg("down");
    if volumes {
        cmd.arg("-v");
    }
    if !services.is_empty() {
        cmd.args(&services);
    }

    let status = cmd.status().context("Failed to run docker compose")?;

    if status.success() {
        ctx.print_success("Services stopped");
    } else {
        anyhow::bail!("Failed to stop services");
    }

    Ok(())
}

fn run_restart(ctx: &AppContext, services: Vec<String>, all: bool) -> Result<()> {
    let all_services = discover_services(ctx)?;

    let services = if all {
        all_services.iter().map(|s| s.name.clone()).collect()
    } else if services.is_empty() {
        select_services(ctx, &all_services, "Select services to restart", true)?
    } else {
        services
    };

    ctx.print_header("Restarting services (down + up)");
    for svc in &services {
        println!("  • {}", style(svc).cyan());
    }
    println!();

    // Use rm -sf + up -d instead of restart so config changes (volumes, env, etc.) are picked up
    let mut rm_cmd = docker_compose(ctx);
    rm_cmd.args(["rm", "-sf"]);
    rm_cmd.args(&services);

    let status = rm_cmd
        .status()
        .context("Failed to stop services")?;

    if !status.success() {
        anyhow::bail!("Failed to stop services");
    }

    let mut up_cmd = docker_compose(ctx);
    up_cmd.args(["up", "-d"]);
    up_cmd.args(&services);

    let status = up_cmd
        .status()
        .context("Failed to start services")?;

    if status.success() {
        ctx.print_success("Services restarted");
    } else {
        anyhow::bail!("Failed to restart services");
    }

    Ok(())
}

fn run_build(ctx: &AppContext, services: Vec<String>, all: bool, no_cache: bool) -> Result<()> {
    let all_services = discover_services(ctx)?;
    let buildable: Vec<&ServiceInfo> = all_services.iter().filter(|s| s.buildable).collect();

    let services = if all {
        buildable.iter().map(|s| s.name.clone()).collect()
    } else if services.is_empty() {
        let items: Vec<String> = buildable
            .iter()
            .map(|s| format!("{} - {}", s.name, s.description))
            .collect();

        if ctx.quiet {
            buildable.iter().map(|s| s.name.clone()).collect()
        } else {
            let selections = MultiSelect::with_theme(&ctx.theme())
                .with_prompt("Select services to build")
                .items(&items)
                .interact()?;

            selections
                .into_iter()
                .map(|i| buildable[i].name.clone())
                .collect()
        }
    } else {
        services
    };

    if services.is_empty() {
        ctx.print_info("No services selected to build");
        return Ok(());
    }

    ctx.print_header("Building services");
    for svc in &services {
        println!("  • {}", style(svc).cyan());
    }
    println!();

    let mut cmd = docker_compose(ctx);
    cmd.arg("build");
    if no_cache {
        cmd.arg("--no-cache");
    }
    cmd.args(&services);

    let status = cmd.status().context("Failed to run docker compose")?;

    if status.success() {
        ctx.print_success("Build complete");
    } else {
        anyhow::bail!("Build failed");
    }

    Ok(())
}

fn run_logs(
    ctx: &AppContext,
    services: Vec<String>,
    all: bool,
    tail: &str,
    no_follow: bool,
) -> Result<()> {
    let services = if all {
        vec![] // Empty means all services for logs
    } else if services.is_empty() {
        let all_services = discover_services(ctx)?;
        select_services(ctx, &all_services, "Select services to follow logs", true)?
    } else {
        services
    };

    ctx.print_header("Following logs");

    let mut cmd = docker_compose(ctx);
    cmd.arg("logs");
    cmd.args(["--tail", tail]);
    if !no_follow {
        cmd.arg("-f");
    }
    if !services.is_empty() {
        cmd.args(&services);
    }

    let status = cmd.status().context("Failed to run docker compose")?;

    if !status.success() && !no_follow {
        // Ctrl+C exits with non-zero, which is fine for logs
    }

    Ok(())
}

fn run_status(ctx: &AppContext) -> Result<()> {
    ctx.print_header("Docker service status");
    println!();

    let mut cmd = docker_compose(ctx);
    cmd.args(["ps", "--format", "table {{.Name}}\t{{.Status}}\t{{.Ports}}"]);

    let status = cmd.status().context("Failed to run docker compose")?;

    if !status.success() {
        anyhow::bail!("Failed to get status");
    }

    Ok(())
}

fn run_shell(ctx: &AppContext, service: Option<String>) -> Result<()> {
    let all_services = discover_services(ctx)?;

    let service_name = match service {
        Some(s) => s,
        None => select_single_service(ctx, &all_services, "Select service to open shell")?,
    };

    ctx.print_header(&format!("Opening shell in {}", service_name));

    let shell = all_services
        .iter()
        .find(|s| s.name == service_name)
        .map(|s| s.shell.as_str())
        .unwrap_or("sh");

    let mut cmd = docker_compose(ctx);
    cmd.args(["exec", &service_name, shell]);

    let status = cmd.status().context("Failed to open shell")?;

    if !status.success() {
        anyhow::bail!("Shell exited with error");
    }

    Ok(())
}

fn run_cypher(ctx: &AppContext) -> Result<()> {
    ctx.print_header("Connecting to Neo4j");

    let mut cmd = docker_compose(ctx);
    cmd.args([
        "exec",
        "neo4j",
        "cypher-shell",
        "-u", "neo4j",
        "-p", "rootsignal",
    ]);

    let status = cmd.status().context("Failed to connect to Neo4j")?;

    if !status.success() {
        anyhow::bail!("cypher-shell exited with error");
    }

    Ok(())
}

fn run_scout(ctx: &AppContext) -> Result<()> {
    ctx.print_header("Running scout investigation");
    ctx.print_info("Scraping sources, extracting signals, populating graph...");
    println!();

    let mut cmd = docker_compose(ctx);
    cmd.args(["run", "--rm", "scout"]);

    let status = cmd.status().context("Failed to run scout")?;

    if status.success() {
        ctx.print_success("Scout run complete");
    } else {
        anyhow::bail!("Scout run failed");
    }

    Ok(())
}
