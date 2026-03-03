use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::graphql::schema::AdminEvent;

/// Shared handle to the event broadcast channel.
/// Each GraphQL subscription receives events by calling `subscribe()`.
#[derive(Clone)]
pub struct EventBroadcast {
    sender: broadcast::Sender<AdminEvent>,
}

impl EventBroadcast {
    /// Create the broadcast channel and spawn a background task that
    /// listens to Postgres `NOTIFY events` and fans out `AdminEvent`s.
    pub fn spawn(pool: PgPool) -> Self {
        let (sender, _) = broadcast::channel::<AdminEvent>(1024);
        let tx = sender.clone();

        tokio::spawn(async move {
            loop {
                match run_listener(&pool, &tx).await {
                    Ok(()) => info!("PgListener disconnected cleanly, reconnecting…"),
                    Err(e) => warn!(error = %e, "PgListener error, reconnecting in 2s…"),
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        });

        info!("EventBroadcast spawned — listening on pg_notify('events')");
        Self { sender }
    }

    /// Get a new receiver for live events.
    pub fn subscribe(&self) -> broadcast::Receiver<AdminEvent> {
        self.sender.subscribe()
    }
}

async fn run_listener(
    pool: &PgPool,
    tx: &broadcast::Sender<AdminEvent>,
) -> Result<(), sqlx::Error> {
    let mut listener = sqlx::postgres::PgListener::connect_with(pool).await?;
    listener.listen("events").await?;

    loop {
        let notification = listener.recv().await?;
        let payload = notification.payload();

        let seq: i64 = match payload.parse() {
            Ok(s) => s,
            Err(_) => {
                warn!(payload, "Non-integer payload on events channel, skipping");
                continue;
            }
        };

        match crate::db::scout_run::get_event_by_seq(pool, seq).await {
            Ok(Some(row)) => {
                let event = AdminEvent::from(row);
                // Ignore send errors — means no active subscribers
                let _ = tx.send(event);
            }
            Ok(None) => {
                warn!(seq, "Event seq from notification not found in DB");
            }
            Err(e) => {
                warn!(seq, error = %e, "Failed to fetch event by seq");
            }
        }
    }
}
