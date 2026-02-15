use anyhow::{Context, Result};
use clap::Subcommand;
use console::style;
use dialoguer::{FuzzySelect, MultiSelect};
use std::process::Command;

use crate::repo_root;

#[derive(Debug, Clone)]
pub struct ServiceInfo {
    pub name: String,
    pub description: String,
    pub buildable: bool,
    pub shell: String,
}

/// Parse docker-compose.yml via `docker compose config --format json`
pub fn discover_services() -> Result<Vec<ServiceInfo>> {
    let root = repo_root();
    let output = Command::new("docker")
        .args(["compose", "-f"])
        .arg(format!("{root}/docker-compose.yml"))
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

/// Multi-select services with optional "All services" option
pub fn select_services(
    all_services: &[ServiceInfo],
    prompt: &str,
    allow_all: bool,
) -> Result<Vec<String>> {
    let items: Vec<String> = all_services
        .iter()
        .map(|s| {
            if s.description.is_empty() {
                s.name.clone()
            } else {
                format!("{} - {}", s.name, s.description)
            }
        })
        .collect();

    let mut items_with_all = items.clone();
    if allow_all {
        items_with_all.insert(0, "All services".to_string());
    }

    let selections = MultiSelect::new()
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

/// Single-select a service with descriptions
fn select_single_service(
    all_services: &[ServiceInfo],
    prompt: &str,
) -> Result<String> {
    let items: Vec<String> = all_services
        .iter()
        .map(|s| {
            if s.description.is_empty() {
                s.name.clone()
            } else {
                format!("{} - {}", s.name, s.description)
            }
        })
        .collect();

    let selection = FuzzySelect::new()
        .with_prompt(prompt)
        .items(&items)
        .default(0)
        .interact()
        .context("Cancelled")?;

    Ok(all_services[selection].name.clone())
}

#[derive(Subcommand)]
pub enum DockerCmd {
    /// Start docker services
    Up {
        /// Specific services to start
        services: Vec<String>,
    },
    /// Stop docker services
    Down {
        /// Specific services to stop
        services: Vec<String>,
        /// Remove volumes
        #[arg(short, long)]
        volumes: bool,
    },
    /// Restart docker services
    Restart {
        /// Specific services to restart
        services: Vec<String>,
    },
    /// Rebuild service images
    Build {
        /// Specific services to build
        services: Vec<String>,
        /// Don't use cache
        #[arg(long)]
        no_cache: bool,
    },
    /// Follow logs for services
    Logs {
        /// Specific services
        services: Vec<String>,
        /// Number of tail lines
        #[arg(short = 'n', long, default_value = "50")]
        tail: String,
    },
    /// Show status of running services
    Status,
    /// Open a shell in a container
    Shell {
        /// Service name
        service: Option<String>,
    },
    /// Open psql in the postgres container
    Psql,
}

fn compose(root: &str) -> Command {
    let mut cmd = Command::new("docker");
    cmd.args(["compose", "-f"])
        .arg(format!("{root}/docker-compose.yml"));
    cmd
}

fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().context("Failed to run docker compose")?;
    if !status.success() {
        anyhow::bail!("docker compose exited with {}", status);
    }
    Ok(())
}


pub fn exec(cmd: DockerCmd) -> Result<()> {
    let root = repo_root();

    match cmd {
        DockerCmd::Up { services } => {
            let services = if services.is_empty() {
                let all = discover_services()?;
                select_services(&all, "Select services to start", true)?
            } else {
                services
            };

            println!("{}", style("Starting services:").cyan());
            for s in &services {
                println!("  • {}", style(s).cyan());
            }

            let mut c = compose(&root);
            c.arg("up").arg("-d");
            for s in &services {
                c.arg(s);
            }
            run(&mut c)
        }
        DockerCmd::Down { services, volumes } => {
            let services = if services.is_empty() {
                let all = discover_services()?;
                select_services(&all, "Select services to stop", true)?
            } else {
                services
            };

            let mut c = compose(&root);
            c.arg("down");
            if volumes {
                c.arg("-v");
            }
            for s in &services {
                c.arg(s);
            }
            run(&mut c)
        }
        DockerCmd::Restart { services } => {
            let services = if services.is_empty() {
                let all = discover_services()?;
                select_services(&all, "Select services to restart", true)?
            } else {
                services
            };

            println!("{}", style("Restarting services:").cyan());
            for s in &services {
                println!("  • {}", style(s).cyan());
            }

            let mut c = compose(&root);
            c.arg("restart");
            for s in &services {
                c.arg(s);
            }
            run(&mut c)
        }
        DockerCmd::Build { services, no_cache } => {
            let services = if services.is_empty() {
                let all = discover_services()?;
                let buildable: Vec<ServiceInfo> =
                    all.into_iter().filter(|s| s.buildable).collect();
                if buildable.is_empty() {
                    anyhow::bail!("No buildable services found");
                }
                select_services(&buildable, "Select services to build", true)?
            } else {
                services
            };

            println!("{}", style("Building services:").cyan());
            for s in &services {
                println!("  • {}", style(s).cyan());
            }

            let mut c = compose(&root);
            c.arg("build");
            if no_cache {
                c.arg("--no-cache");
            }
            for s in &services {
                c.arg(s);
            }
            run(&mut c)?;

            // Recreate running containers with the new images
            println!("{}", style("Restarting with new images:").cyan());
            let mut c = compose(&root);
            c.args(["up", "-d", "--force-recreate"]);
            for s in &services {
                c.arg(s);
            }
            run(&mut c)
        }
        DockerCmd::Logs { services, tail } => {
            let services = if services.is_empty() {
                let all = discover_services()?;
                select_services(&all, "Select services to follow logs", true)?
            } else {
                services
            };

            let mut c = compose(&root);
            c.args(["logs", "-f", "--tail"]).arg(&tail);
            for s in &services {
                c.arg(s);
            }
            run(&mut c)
        }
        DockerCmd::Status => {
            let mut c = compose(&root);
            c.args(["ps", "--format", "table {{.Name}}\t{{.Status}}\t{{.Ports}}"]);
            run(&mut c)
        }
        DockerCmd::Shell { service } => {
            let all = discover_services()?;
            let svc = match service {
                Some(s) => s,
                None => select_single_service(&all, "Select service to open shell")?,
            };

            let shell = all
                .iter()
                .find(|s| s.name == svc)
                .map(|s| s.shell.as_str())
                .unwrap_or("sh");

            let mut c = compose(&root);
            c.args(["exec", &svc, shell]);
            run(&mut c)
        }
        DockerCmd::Psql => {
            let mut c = compose(&root);
            c.args([
                "exec", "postgres", "psql", "-U", "postgres", "-d", "rootsignal",
            ]);
            run(&mut c)
        }
    }
}
