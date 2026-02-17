use rootsignal_common::{ExtractionQuality, GeoAccuracy, Node};

/// Compute extraction quality for a signal node.
pub fn score(node: &Node) -> ExtractionQuality {
    let meta = match node.meta() {
        Some(m) => m,
        None => {
            return ExtractionQuality {
                actionable: false,
                has_location: false,
                has_action_url: false,
                has_timing: false,

                geo_accuracy: GeoAccuracy::Low,
                completeness: 0.0,
                confidence: 0.0,
            };
        }
    };

    let has_location = meta.location.is_some();
    let geo_accuracy = match meta.location {
        Some(ref loc) => match loc.precision {
            rootsignal_common::GeoPrecision::Exact => GeoAccuracy::High,
            rootsignal_common::GeoPrecision::Neighborhood => GeoAccuracy::Medium,
            _ => GeoAccuracy::Low,
        },
        None => GeoAccuracy::Low,
    };

    let (has_action_url, has_timing) = match node {
        Node::Event(e) => {
            // Extractor defaults: missing URL → source_url, missing time → now.
            // Detect these defaults so quality score reflects actual extraction quality.
            let has_real_url = !e.action_url.is_empty() && e.action_url != meta.source_url;
            let has_real_timing = (e.starts_at - meta.extracted_at).num_seconds().abs() > 60;
            (has_real_url, has_real_timing)
        }
        Node::Give(g) => (!g.action_url.is_empty(), g.is_ongoing),
        Node::Ask(a) => (a.action_url.is_some(), false),
        Node::Notice(_) => (false, false),
        Node::Tension(_) => (false, false),
        Node::Evidence(_) => (false, false),
    };

    let actionable = matches!(node, Node::Notice(_))
        || (has_action_url
            && (has_timing
                || matches!(node, Node::Give(g) if g.is_ongoing)
                || matches!(node, Node::Ask(_))));

    // Completeness: fraction of 7 key fields populated
    // title, summary, signal_type (always), audience_roles, location, action_url, timing
    let mut filled = 3.0_f32; // title, summary, signal_type always present
    if !meta.audience_roles.is_empty() {
        filled += 1.0;
    }
    if has_location {
        filled += 1.0;
    }
    if has_action_url {
        filled += 1.0;
    }
    if has_timing {
        filled += 1.0;
    }
    let completeness = filled / 7.0;

    // Confidence: weighted average
    let geo_score = match geo_accuracy {
        GeoAccuracy::High => 1.0_f32,
        GeoAccuracy::Medium => 0.7,
        GeoAccuracy::Low => 0.3,
    };
    let confidence =
        meta.source_trust * 0.3 + geo_score * 0.2 + completeness * 0.3 + meta.freshness_score * 0.2;

    ExtractionQuality {
        actionable,
        has_location,
        has_action_url,
        has_timing,
        geo_accuracy,
        completeness,
        confidence: confidence.clamp(0.0, 1.0),
    }
}
