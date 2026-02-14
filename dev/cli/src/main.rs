mod cmd;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use console::style;
use std::process::Command;

/// Root Signal developer CLI
#[derive(Parser)]
#[command(name = "dev", about = "Root Signal developer tools")]
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
        "🚀 Start environment (up)",
        "🛑 Stop environment (down)",
        "🐳 Docker services →",
        "🗄️  Database →",
        "📊 Status",
        "🩺 Doctor",
        "❌ Exit",
    ];

    loop {
        println!();
        let choice = dialoguer::FuzzySelect::new()
            .with_prompt("What would you like to do?")
            .items(&items)
            .default(0)
            .interact()
            .context("Cancelled")?;

        match choice {
            0 => cmd_up()?,
            1 => cmd::docker::exec(cmd::docker::DockerCmd::Down {
                services: vec![],
                volumes: false,
            })?,
            2 => docker_submenu()?,
            3 => db_submenu()?,
            4 => cmd_status()?,
            5 => cmd_doctor()?,
            _ => break,
        }
    }

    Ok(())
}

fn docker_submenu() -> Result<()> {
    let items = vec![
        "Start services",
        "Stop services",
        "Restart services",
        "Rebuild images",
        "Follow logs",
        "Status",
        "Shell into container",
        "PostgreSQL shell",
        "← Back",
    ];

    let choice = dialoguer::Select::new()
        .with_prompt("Docker")
        .items(&items)
        .default(0)
        .interact()
        .context("Cancelled")?;

    match choice {
        0 => cmd::docker::exec(cmd::docker::DockerCmd::Up {
            services: vec![],
        }),
        1 => cmd::docker::exec(cmd::docker::DockerCmd::Down {
            services: vec![],
            volumes: false,
        }),
        2 => cmd::docker::exec(cmd::docker::DockerCmd::Restart {
            services: vec![],
        }),
        3 => cmd::docker::exec(cmd::docker::DockerCmd::Build {
            services: vec![],
            no_cache: false,
        }),
        4 => cmd::docker::exec(cmd::docker::DockerCmd::Logs {
            services: vec![],
            tail: "100".to_string(),
        }),
        5 => cmd::docker::exec(cmd::docker::DockerCmd::Status),
        6 => cmd::docker::exec(cmd::docker::DockerCmd::Shell { service: None }),
        7 => cmd::docker::exec(cmd::docker::DockerCmd::Psql),
        _ => Ok(()),
    }
}

fn db_submenu() -> Result<()> {
    let items = vec![
        "Run migrations",
        "Reset database (drop + migrate)",
        "Migration status",
        "PostgreSQL shell",
        "← Back",
    ];

    let choice = dialoguer::Select::new()
        .with_prompt("Database")
        .items(&items)
        .default(0)
        .interact()
        .context("Cancelled")?;

    match choice {
        0 => cmd::db::exec(cmd::db::DbCmd::Migrate),
        1 => cmd::db::exec(cmd::db::DbCmd::Reset),
        2 => cmd::db::exec(cmd::db::DbCmd::Status),
        3 => cmd::db::exec(cmd::db::DbCmd::Psql),
        _ => Ok(()),
    }
}
