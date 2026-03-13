use uuid::Uuid;

/// Result of a coalescing run — both seed and feed modes.
pub struct CoalescingResult {
    pub new_groups: Vec<ProtoGroup>,
    pub fed_signals: Vec<FedSignal>,
    pub refined_queries: Vec<(Uuid, Vec<String>)>,
}

/// A group discovered during seed mode, not yet persisted.
pub struct ProtoGroup {
    pub group_id: Uuid,
    pub label: String,
    pub queries: Vec<String>,
    pub signal_ids: Vec<(Uuid, f64)>,
}

/// A signal added to an existing group during feed mode.
pub struct FedSignal {
    pub signal_id: Uuid,
    pub group_id: Uuid,
    pub confidence: f64,
}
