//! Node.js frontend app commands

use anyhow::{Context, Result};
use clap::Subcommand;
use devkit_core::AppContext;
use dialoguer::Select;

#[derive(Subcommand)]
pub enum NodeCommand {
    /// Run a frontend app (interactive selection)
    Run {
        /// App to run: admin-app or search-app
        app: Option<String>,
    },

    /// Install dependencies for a frontend app
    Install {
        /// App to install: admin-app or search-app
        app: Option<String>,
    },
}

const APPS: &[(&str, &str)] = &[
    ("admin-app", "Admin dashboard (port 5173)"),
    ("search-app", "Search interface (port 5174)"),
];

pub fn run(ctx: &AppContext, cmd: NodeCommand) -> Result<()> {
    match cmd {
        NodeCommand::Run { app } => {
            let app = resolve_app(ctx, app.as_deref())?;
            run_dev(ctx, &app)
        }
        NodeCommand::Install { app } => {
            let app = resolve_app(ctx, app.as_deref())?;
            run_install(ctx, &app)
        }
    }
}

fn resolve_app(ctx: &AppContext, app: Option<&str>) -> Result<String> {
    if let Some(name) = app {
        if APPS.iter().any(|(n, _)| *n == name) {
            return Ok(name.to_string());
        }
        anyhow::bail!("Unknown app '{}'. Available: admin-app, search-app", name);
    }

    let items: Vec<String> = APPS.iter().map(|(n, d)| format!("{n} — {d}")).collect();

    let choice = Select::with_theme(&ctx.theme())
        .with_prompt("Select app")
        .items(&items)
        .default(0)
        .interact()?;

    Ok(APPS[choice].0.to_string())
}

fn run_dev(ctx: &AppContext, app: &str) -> Result<()> {
    ctx.print_header(&format!("Starting {app}"));
    println!();

    let app_dir = ctx.repo.join("modules").join(app);
    if !app_dir.exists() {
        anyhow::bail!("App directory not found: {}", app_dir.display());
    }

    // Install deps if node_modules missing
    if !app_dir.join("node_modules").exists() {
        ctx.print_info("Installing dependencies first...");
        let status = std::process::Command::new("npm")
            .arg("install")
            .current_dir(&app_dir)
            .status()
            .context("Failed to run npm install")?;
        if !status.success() {
            anyhow::bail!("npm install failed");
        }
        println!();
    }

    let status = std::process::Command::new("npm")
        .args(["run", "dev"])
        .current_dir(&app_dir)
        .status()
        .context("Failed to run npm run dev")?;

    if !status.success() {
        anyhow::bail!("{app} exited with error");
    }

    Ok(())
}

fn run_install(ctx: &AppContext, app: &str) -> Result<()> {
    ctx.print_header(&format!("Installing {app} dependencies"));
    println!();

    let app_dir = ctx.repo.join("modules").join(app);
    if !app_dir.exists() {
        anyhow::bail!("App directory not found: {}", app_dir.display());
    }

    let status = std::process::Command::new("npm")
        .arg("install")
        .current_dir(&app_dir)
        .status()
        .context("Failed to run npm install")?;

    if status.success() {
        ctx.print_success("Dependencies installed");
    } else {
        anyhow::bail!("npm install failed");
    }

    Ok(())
}

/// Interactive menu for node commands
pub fn interactive_menu(ctx: &AppContext) -> Result<()> {
    let items = vec![
        "Run admin-app (port 5173)",
        "Run search-app (port 5174)",
        "Install admin-app deps",
        "Install search-app deps",
        "← Back",
    ];

    let choice = Select::with_theme(&ctx.theme())
        .with_prompt("Node")
        .items(&items)
        .default(0)
        .interact()?;

    match choice {
        0 => run_dev(ctx, "admin-app"),
        1 => run_dev(ctx, "search-app"),
        2 => run_install(ctx, "admin-app"),
        3 => run_install(ctx, "search-app"),
        _ => Ok(()),
    }
}
