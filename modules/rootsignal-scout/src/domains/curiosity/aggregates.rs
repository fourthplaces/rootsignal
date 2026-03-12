use causal::{aggregator, aggregators, Aggregate};
use serde::{Deserialize, Serialize};

use super::events::CuriosityEvent;

/// Per-signal lifecycle — tracks which curiosity operations have run.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SignalLifecycle {
    pub investigated: bool,
    pub concern_linked: bool,
}

impl Aggregate for SignalLifecycle {
    fn aggregate_type() -> &'static str {
        "SignalLifecycle"
    }
}

/// Per-concern lifecycle — tracks which curiosity operations have run.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ConcernLifecycle {
    pub responses_scouted: bool,
    pub gatherings_scouted: bool,
}

impl Aggregate for ConcernLifecycle {
    fn aggregate_type() -> &'static str {
        "ConcernLifecycle"
    }
}

#[aggregators]
pub mod curiosity_aggregators {
    use super::*;

    #[aggregator(id_fn = "lifecycle_signal_id")]
    fn on_signal_lifecycle(state: &mut SignalLifecycle, event: CuriosityEvent) {
        match &event {
            CuriosityEvent::SignalInvestigated { .. } => {
                state.investigated = true;
            }
            CuriosityEvent::SignalConcernLinked { .. } => {
                state.concern_linked = true;
            }
            CuriosityEvent::TensionDiscovered { .. }
            | CuriosityEvent::SignalDiscovered { .. }
            | CuriosityEvent::EmergentTensionDiscovered { .. } => {
                state.concern_linked = true;
                state.investigated = true;
            }
            _ => {}
        }
    }

    #[aggregator(id_fn = "lifecycle_concern_id")]
    fn on_concern_lifecycle(state: &mut ConcernLifecycle, event: CuriosityEvent) {
        match &event {
            CuriosityEvent::ConcernResponsesScouted { .. } => {
                state.responses_scouted = true;
            }
            CuriosityEvent::ConcernGatheringsScouted { .. } => {
                state.gatherings_scouted = true;
            }
            CuriosityEvent::EmergentTensionDiscovered { .. } => {
                state.responses_scouted = true;
            }
            _ => {}
        }
    }
}
