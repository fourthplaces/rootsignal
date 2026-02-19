use anyhow::Result;
use tracing::info;
use tracing_subscriber::EnvFilter;

use rootsignal_common::Config;
use rootsignal_graph::{migrate::migrate, GraphClient, GraphWriter};
use rootsignal_scout_supervisor::{
    notify::{backend::NotifyBackend, noop::NoopBackend, router::NotifyRouter},
    supervisor::Supervisor,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("rootsignal=info".parse()?))
        .init();

    info!("Root Signal Scout Supervisor starting...");

    // Load config
    let config = Config::supervisor_from_env();
    config.log_redacted();

    // Connect to Neo4j
    let client = GraphClient::connect(
        &config.neo4j_uri,
        &config.neo4j_user,
        &config.neo4j_password,
    )
    .await?;

    // Run migrations (idempotent)
    migrate(&client).await?;

    // Load city node
    let writer = GraphWriter::new(client.clone());
    let city = writer.get_city(&config.city).await?.ok_or_else(|| {
        anyhow::anyhow!(
            "City '{}' not found in graph. Run scout first.",
            config.city
        )
    })?;

    info!(
        slug = city.slug.as_str(),
        name = city.name.as_str(),
        "Loaded city"
    );

    // Build notification backend: Slack if configured, otherwise Noop
    let notifier: Box<dyn NotifyBackend> = match NotifyRouter::from_env() {
        Some(router) => {
            info!("Slack notifications enabled");
            Box::new(router)
        }
        None => {
            info!("No SLACK_WEBHOOK_URL set, notifications disabled");
            Box::new(NoopBackend)
        }
    };

    // Create and run supervisor
    let supervisor = Supervisor::new(client, city, config.anthropic_api_key.clone(), notifier);
    let stats = supervisor.run().await?;

    info!("Supervisor complete. {stats}");
    Ok(())
}
