use anyhow::Result;
use tracing::info;
use tracing_subscriber::EnvFilter;

use rootsignal_common::Config;
use rootsignal_graph::{migrate::migrate, GraphClient, GraphWriter};
use rootsignal_scout_supervisor::{
    notify::noop::NoopBackend,
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

    // Connect to Memgraph
    let client =
        GraphClient::connect(&config.neo4j_uri, &config.neo4j_user, &config.neo4j_password)
            .await?;

    // Run migrations (idempotent)
    migrate(&client).await?;

    // Load city node
    let writer = GraphWriter::new(client.clone());
    let city = writer
        .get_city(&config.city)
        .await?
        .ok_or_else(|| anyhow::anyhow!("City '{}' not found in graph. Run scout first.", config.city))?;

    info!(slug = city.slug.as_str(), name = city.name.as_str(), "Loaded city");

    // Build notification backend
    // Phase 1: NoopBackend (Slack wired in Phase 2)
    let notifier: Box<dyn rootsignal_scout_supervisor::notify::backend::NotifyBackend> =
        Box::new(NoopBackend);

    // Create and run supervisor
    let supervisor = Supervisor::new(client, city, notifier);
    let stats = supervisor.run().await?;

    info!("Supervisor complete. {stats}");
    Ok(())
}
