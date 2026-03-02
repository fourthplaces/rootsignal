use rootsignal_common::events::SystemEvent;
use serde::{Deserialize, Serialize};
use seesaw_core::{aggregators, Aggregate};

/// Tracks how many beacons were created during a news scan.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct NewsScanState {
    pub beacons_created: u32,
}

impl Aggregate for NewsScanState {
    fn aggregate_type() -> &'static str {
        "NewsScan"
    }
}

#[aggregators(singleton)]
pub mod news_aggregators {
    use super::*;

    fn on_system(state: &mut NewsScanState, event: SystemEvent) {
        if matches!(event, SystemEvent::BeaconDetected { .. }) {
            state.beacons_created += 1;
        }
    }
}
