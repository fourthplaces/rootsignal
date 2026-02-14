use anyhow::{Context, Result};
use clap::Subcommand;
use std::process::Command;

use crate::repo_root;

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

/// List service names from docker-compose.yml
pub fn list_services() -> Result<Vec<String>> {
    let root = repo_root();
    let output = compose(&root)
        .args(["config", "--services"])
        .output()
        .context("Failed to read docker-compose services")?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text.lines().map(|s| s.to_string()).collect())
}

pub fn exec(cmd: DockerCmd) -> Result<()> {
    let root = repo_root();

    match cmd {
        DockerCmd::Up { services } => {
            let mut c = compose(&root);
            c.arg("up").arg("-d");
            for s in &services {
                c.arg(s);
            }
            run(&mut c)
        }
        DockerCmd::Down { services, volumes } => {
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
            let mut c = compose(&root);
            c.arg("restart");
            for s in &services {
                c.arg(s);
            }
            run(&mut c)
        }
        DockerCmd::Logs { services, tail } => {
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
            let svc = match service {
                Some(s) => s,
                None => pick_service()?,
            };
            let mut c = compose(&root);
            c.args(["exec", &svc, "sh", "-c", "bash || sh"]);
            run(&mut c)
        }
        DockerCmd::Psql => {
            let mut c = compose(&root);
            c.args([
                "exec", "postgres", "psql", "-U", "postgres", "-d", "taproot",
            ]);
            run(&mut c)
        }
    }
}

fn pick_service() -> Result<String> {
    let services = list_services()?;
    if services.is_empty() {
        anyhow::bail!("No services found in docker-compose.yml");
    }
    let selection = dialoguer::FuzzySelect::new()
        .with_prompt("Select a service")
        .items(&services)
        .default(0)
        .interact()
        .context("Cancelled")?;
    Ok(services[selection].clone())
}
