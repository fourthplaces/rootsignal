/// Trait shared by all event layers: WorldEvent, SystemEvent, TelemetryEvent.
///
/// Enables generic `append_and_project` — any event that implements this can be
/// stored in the EventStore (which accepts `event_type` + `serde_json::Value`).
pub trait Eventlike: std::fmt::Debug + Send + Sync {
    /// The snake_case event type string (matches the `event_type` column in Postgres).
    fn event_type(&self) -> &'static str;

    /// Serialize this event to a JSON Value for the EventStore payload.
    fn to_payload(&self) -> serde_json::Value;
}
