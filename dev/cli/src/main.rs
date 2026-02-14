mod cmd;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use console::style;
use std::process::Command;

/// Taproot developer CLI
#[derive(Parser)]
#[command(name = "dev", about = "Taproot developer tools")]
struct Cli {
    /// Suppress interactive prompts
    #[arg(long, global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Option<TopCmd>,
}

#[derive(Subcommand)]
enum TopCmd {
    /// Start all services (postgres -> migrations -> restate)
    Up,
    /// Stop all services
    Down {
        /// Remove volumes
        #[arg(short, long)]
        volumes: bool,
    },
    /// Docker service management
    Docker {
        #[command(subcommand)]
        cmd: cmd::docker::DockerCmd,
    },
    /// Database management
    Db {
        #[command(subcommand)]
        cmd: cmd::db::DbCmd,
    },
    /// Show environment status
    Status,
    /// Check system prerequisites
    Doctor,
}

pub fn repo_root() -> String {
    std::env::var("REPO_ROOT").unwrap_or_else(|_| ".".to_string())
}

fn main() -> Result<()> {
    // Load .env from repo root
    let root = repo_root();
    let _ = dotenvy::from_path(format!("{root}/.env"));

    let cli = Cli::parse();

    match cli.command {
        Some(cmd) => run(cmd),
        None => {
            if cli.quiet {
                Cli::parse_from(["dev", "--help"]);
                Ok(())
            } else {
                interactive_menu()
            }
        }
    }
}

fn run(cmd: TopCmd) -> Result<()> {
    match cmd {
        TopCmd::Up => cmd_up(),
        TopCmd::Down { volumes } => {
            cmd::docker::exec(cmd::docker::DockerCmd::Down {
                services: vec![],
                volumes,
            })
        }
        TopCmd::Docker { cmd } => cmd::docker::exec(cmd),
        TopCmd::Db { cmd } => cmd::db::exec(cmd),
        TopCmd::Status => cmd_status(),
        TopCmd::Doctor => cmd_doctor(),
    }
}

fn cmd_up() -> Result<()> {
    let root = repo_root();

    // 1. Start postgres
    println!("{}", style("Starting postgres...").cyan());
    cmd::docker::exec(cmd::docker::DockerCmd::Up {
        services: vec!["postgres".to_string()],
    })?;

    // 2. Wait for postgres to accept connections
    println!("{}", style("Waiting for postgres...").dim());
    let mut attempts = 0;
    loop {
        let status = Command::new("docker")
            .args([
                "compose",
                "-f",
                &format!("{root}/docker-compose.yml"),
                "exec",
                "postgres",
                "pg_isready",
                "-U",
                "postgres",
            ])
            .output();
        match status {
            Ok(out) if out.status.success() => break,
            _ if attempts >= 30 => anyhow::bail!("Postgres did not become ready in time"),
            _ => {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    }
    println!("{}", style("Postgres is ready.").green());

    // 3. Run migrations
    println!("{}", style("Running migrations...").cyan());
    cmd::db::exec(cmd::db::DbCmd::Migrate)?;

    // 4. Start restate
    println!("{}", style("Starting restate...").cyan());
    cmd::docker::exec(cmd::docker::DockerCmd::Up {
        services: vec!["restate".to_string()],
    })?;

    println!("{}", style("All services are up!").green().bold());
    Ok(())
}

fn cmd_status() -> Result<()> {
    println!("{}", style("Docker services:").bold());
    cmd::docker::exec(cmd::docker::DockerCmd::Status)?;

    println!();
    println!("{}", style("Environment:").bold());
    let vars = ["DATABASE_URL", "RESTATE_ADMIN_URL", "PORT"];
    for var in vars {
        let val = std::env::var(var).unwrap_or_else(|_| "(not set)".to_string());
        // Mask secrets
        let display = if var.contains("KEY") || var.contains("SECRET") {
            if val.len() > 8 {
                format!("{}...{}", &val[..4], &val[val.len() - 4..])
            } else {
                "(set)".to_string()
            }
        } else {
            val
        };
        println!("  {}: {}", style(var).dim(), display);
    }
    Ok(())
}

fn cmd_doctor() -> Result<()> {
    let checks: &[(&str, &[&str])] = &[
        ("docker", &["--version"]),
        ("docker compose", &["compose", "version"]),
        ("cargo", &["--version"]),
        ("sqlx", &["--version"]),
        ("psql", &["--version"]),
        ("git", &["--version"]),
    ];

    let mut all_ok = true;
    for (name, args) in checks {
        let bin = if *name == "docker compose" {
            "docker"
        } else {
            name
        };
        let result = Command::new(bin).args(*args).output();
        match result {
            Ok(out) if out.status.success() => {
                let ver = String::from_utf8_lossy(&out.stdout);
                let ver = ver.lines().next().unwrap_or("").trim();
                println!("  {} {}", style("✓").green(), format!("{name}: {ver}"));
            }
            _ => {
                println!("  {} {}", style("✗").red(), format!("{name}: not found"));
                all_ok = false;
            }
        }
    }

    // Check .env
    let root = repo_root();
    let env_path = format!("{root}/.env");
    if std::path::Path::new(&env_path).exists() {
        println!("  {} .env file exists", style("✓").green());
    } else {
        println!(
            "  {} .env file missing (copy from .env.example)",
            style("!").yellow()
        );
    }

    // Check docker-compose.yml
    let dc_path = format!("{root}/docker-compose.yml");
    if std::path::Path::new(&dc_path).exists() {
        println!("  {} docker-compose.yml exists", style("✓").green());
    } else {
        println!("  {} docker-compose.yml missing", style("✗").red());
        all_ok = false;
    }

    if all_ok {
        println!("\n{}", style("All checks passed!").green().bold());
    } else {
        println!(
            "\n{}",
            style("Some checks failed. Install missing tools above.").yellow()
        );
    }

    Ok(())
}

fn interactive_menu() -> Result<()> {
    let items = vec![
        ("up", "Start all services (postgres -> migrations -> restate)"),
        ("down", "Stop all services"),
        ("docker status", "Show running Docker services"),
        ("docker logs", "Follow Docker service logs"),
        ("docker psql", "Open psql in postgres container"),
        ("db migrate", "Run pending migrations"),
        ("db reset", "Drop + create + migrate"),
        ("db status", "Show migration status"),
        ("db psql", "Open psql via DATABASE_URL"),
        ("status", "Show environment info"),
        ("doctor", "Check system prerequisites"),
    ];

    let labels: Vec<String> = items
        .iter()
        .map(|(cmd, desc)| format!("{:<20} {}", cmd, style(desc).dim()))
        .collect();

    let selection = dialoguer::FuzzySelect::new()
        .with_prompt("What do you want to do?")
        .items(&labels)
        .default(0)
        .interact()
        .context("Cancelled")?;

    let chosen = items[selection].0;
    let args: Vec<&str> = chosen.split_whitespace().collect();

    // Re-parse as if these args were passed on CLI
    let mut full_args = vec!["dev"];
    full_args.extend(args);
    let cli = Cli::parse_from(full_args);
    if let Some(cmd) = cli.command {
        run(cmd)
    } else {
        Ok(())
    }
}
