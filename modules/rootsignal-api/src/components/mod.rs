use rootsignal_common::{EvidenceNode, Node, NodeType, TensionResponse};

pub mod map;
pub mod quality;
pub mod signal_detail;
pub mod signals_list;

pub use map::render_map;
pub use quality::render_quality;
pub use signal_detail::render_signal_detail;
pub use signals_list::render_signals_list;

// --- View Models ---

#[derive(Clone, PartialEq)]
pub struct NodeView {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub type_label: String,
    pub type_class: String,
    pub confidence: f32,
    pub corroboration_count: u32,
    pub source_diversity: u32,
    pub external_ratio: f32,
    pub cause_heat: f64,
    pub last_confirmed: String,
    pub action_url: String,
    pub completeness_label: String,
    pub tension_category: Option<String>,
    pub tension_what_would_help: Option<String>,
}

#[derive(Clone, PartialEq)]
pub struct EvidenceView {
    pub source_url: String,
    pub snippet: Option<String>,
    pub relevance: Option<String>,
    pub evidence_confidence: Option<f32>,
}

#[derive(Clone, PartialEq)]
pub struct ResponseView {
    pub id: String,
    pub title: String,
    pub type_label: String,
    pub type_class: String,
    pub match_strength: f64,
    pub explanation: String,
}

pub fn node_to_view(node: &Node) -> NodeView {
    let meta = node.meta();
    let (type_label, type_class) = match node.node_type() {
        NodeType::Event => ("Event", "event"),
        NodeType::Give => ("Give", "give"),
        NodeType::Ask => ("Ask", "ask"),
        NodeType::Notice => ("Notice", "notice"),
        NodeType::Tension => ("Tension", "tension"),
        NodeType::Evidence => ("Evidence", "evidence"),
    };

    let action_url = match node {
        Node::Event(e) => e.action_url.clone(),
        Node::Give(g) => g.action_url.clone(),
        Node::Ask(a) => a.action_url.clone().unwrap_or_default(),
        Node::Notice(_) => String::new(),
        _ => String::new(),
    };

    let confidence = meta.map(|m| m.confidence).unwrap_or(0.0);

    let has_loc = meta.map(|m| m.location.is_some()).unwrap_or(false);
    let completeness_label = if has_loc && !action_url.is_empty() {
        "Has location, timing, and action link"
    } else if has_loc {
        "Has location (missing action link)"
    } else if !action_url.is_empty() {
        "Has action link (missing location)"
    } else {
        "Limited details available"
    };

    let last_confirmed = meta
        .map(|m| {
            let days = (chrono::Utc::now() - m.last_confirmed_active).num_days();
            if days == 0 {
                "today".to_string()
            } else if days == 1 {
                "yesterday".to_string()
            } else {
                format!("{days} days ago")
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    let (tension_category, tension_what_would_help) = match node {
        Node::Tension(t) => (t.category.clone(), t.what_would_help.clone()),
        _ => (None, None),
    };

    NodeView {
        id: node.id().to_string(),
        title: node.title().to_string(),
        summary: meta.map(|m| m.summary.clone()).unwrap_or_default(),
        type_label: type_label.to_string(),
        type_class: type_class.to_string(),
        confidence,
        corroboration_count: meta.map(|m| m.corroboration_count).unwrap_or(0),
        source_diversity: meta.map(|m| m.source_diversity).unwrap_or(1),
        external_ratio: meta.map(|m| m.external_ratio).unwrap_or(0.0),
        cause_heat: meta.map(|m| m.cause_heat).unwrap_or(0.0),
        last_confirmed,
        action_url,
        completeness_label: completeness_label.to_string(),
        tension_category,
        tension_what_would_help,
    }
}

pub fn evidence_to_view(ev: &EvidenceNode) -> EvidenceView {
    EvidenceView {
        source_url: ev.source_url.clone(),
        snippet: ev.snippet.clone(),
        relevance: ev.relevance.clone(),
        evidence_confidence: ev.evidence_confidence,
    }
}

pub fn tension_response_to_view(tr: &TensionResponse) -> ResponseView {
    let (type_label, type_class) = match tr.node.node_type() {
        NodeType::Event => ("Event", "event"),
        NodeType::Give => ("Give", "give"),
        NodeType::Ask => ("Ask", "ask"),
        NodeType::Notice => ("Notice", "notice"),
        NodeType::Tension => ("Tension", "tension"),
        NodeType::Evidence => ("Evidence", "evidence"),
    };
    ResponseView {
        id: tr.node.id().to_string(),
        title: tr.node.title().to_string(),
        type_label: type_label.to_string(),
        type_class: type_class.to_string(),
        match_strength: tr.match_strength,
        explanation: tr.explanation.clone(),
    }
}
