use causal_inspector::EventDisplay;
use crate::db::models::scout_run::{event_domain_prefix, event_summary};

#[derive(Clone)]
pub struct RootsignalEventDisplay;

impl EventDisplay for RootsignalEventDisplay {
    fn display_name(&self, event_type: &str, _payload: &serde_json::Value) -> String {
        let variant = event_type.split_once(':').map(|(_, v)| v).unwrap_or(event_type);
        let domain = event_domain_prefix(event_type);
        format!("{domain}:{variant}")
    }

    fn summary(&self, event_type: &str, payload: &serde_json::Value) -> Option<String> {
        let variant = event_type.split_once(':').map(|(_, v)| v).unwrap_or(event_type);
        event_summary(variant, payload)
    }
}
