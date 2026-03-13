use std::collections::HashSet;
use uuid::Uuid;

/// A structured annotation parsed from `[type:identifier]` tokens in markdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    pub kind: String,
    pub identifier: String,
    /// The full raw token, e.g. `[signal:abc-123]`
    pub raw: String,
}

/// Parse all `[type:identifier]` annotations from a text body.
///
/// Recognizes tokens like `[signal:UUID]`, `[actor:UUID]`, `[location:UUID]`,
/// `[url:https://...]`. The `type` must be alphabetic; the `identifier` runs
/// until the closing `]`.
pub fn extract_annotations(body: &str) -> Vec<Annotation> {
    let mut results = Vec::new();
    let bytes = body.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'[' {
            let start = i;
            i += 1;

            // Read the kind (alphabetic chars before the colon)
            let kind_start = i;
            while i < len && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            let kind_end = i;

            if i < len && bytes[i] == b':' && kind_end > kind_start {
                i += 1; // skip colon
                let id_start = i;

                // Read until closing bracket (stop at `[` to avoid spanning across tokens)
                while i < len && bytes[i] != b']' && bytes[i] != b'[' {
                    i += 1;
                }

                if i < len && bytes[i] == b']' {
                    let identifier = &body[id_start..i];
                    if !identifier.is_empty() {
                        results.push(Annotation {
                            kind: body[kind_start..kind_end].to_string(),
                            identifier: identifier.to_string(),
                            raw: body[start..=i].to_string(),
                        });
                    }
                    i += 1;
                    continue;
                }
            }
            // Not a valid annotation — resume scanning from after the `[`
            i = start + 1;
        } else {
            i += 1;
        }
    }

    results
}

/// Extract signal UUIDs from `[signal:UUID]` annotations in a text body.
pub fn extract_signal_ids(body: &str) -> Vec<Uuid> {
    extract_annotations(body)
        .iter()
        .filter(|a| a.kind == "signal")
        .filter_map(|a| Uuid::parse_str(&a.identifier).ok())
        .collect()
}

/// Remove `[signal:UUID]` tokens whose UUID is not in the valid set.
/// Returns the cleaned body and the number of citations stripped.
pub fn strip_invalid_signal_citations(body: &str, valid_ids: &HashSet<Uuid>) -> (String, usize) {
    let annotations = extract_annotations(body);
    let mut result = body.to_string();
    let mut stripped = 0;

    // Process in reverse so earlier offsets aren't invalidated
    for ann in annotations.iter().rev() {
        if ann.kind != "signal" {
            continue;
        }
        let parsed = match Uuid::parse_str(&ann.identifier) {
            Ok(id) => id,
            Err(_) => continue, // not a valid UUID — leave it alone
        };

        if !valid_ids.contains(&parsed) {
            if let Some(pos) = result.rfind(&ann.raw) {
                result.replace_range(pos..pos + ann.raw.len(), "");
                stripped += 1;
            }
        }
    }

    (result, stripped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_signal_citation_extracts_correctly() {
        let id = Uuid::new_v4();
        let body = format!("A cleanup is planned [signal:{}] for Saturday.", id);
        let annotations = extract_annotations(&body);

        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].kind, "signal");
        assert_eq!(annotations[0].identifier, id.to_string());
        assert_eq!(annotations[0].raw, format!("[signal:{}]", id));
    }

    #[test]
    fn multiple_annotation_types_parse() {
        let body = "[signal:abc] some text [actor:def] more [url:https://example.com]";
        let annotations = extract_annotations(body);

        assert_eq!(annotations.len(), 3);
        assert_eq!(annotations[0].kind, "signal");
        assert_eq!(annotations[0].identifier, "abc");
        assert_eq!(annotations[1].kind, "actor");
        assert_eq!(annotations[1].identifier, "def");
        assert_eq!(annotations[2].kind, "url");
        assert_eq!(annotations[2].identifier, "https://example.com");
    }

    #[test]
    fn stacked_citations_parse_individually() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let body = format!("Volunteers needed [signal:{}][signal:{}] this week.", a, b);
        let ids = extract_signal_ids(&body);

        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], a);
        assert_eq!(ids[1], b);
    }

    #[test]
    fn no_annotations_returns_empty() {
        let annotations = extract_annotations("Just plain text with [markdown](links).");
        assert!(annotations.is_empty());
    }

    #[test]
    fn malformed_tokens_are_skipped() {
        let body = "[signal:] [signal [noclose:abc [:broken] [123:numeric]";
        let annotations = extract_annotations(body);
        // [signal:] has empty identifier — skipped
        // [signal has no colon after kind — skipped
        // [noclose:abc has no closing bracket — skipped
        // [:broken] has empty kind — skipped
        // [123:numeric] kind starts with digit — skipped
        assert!(annotations.is_empty());
    }

    #[test]
    fn extract_signal_ids_filters_non_signal_annotations() {
        let id = Uuid::new_v4();
        let body = format!("[actor:someone] [signal:{}] [url:https://x.com]", id);
        let ids = extract_signal_ids(&body);

        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], id);
    }

    #[test]
    fn extract_signal_ids_skips_invalid_uuids() {
        let body = "[signal:not-a-uuid] [signal:also-bad]";
        let ids = extract_signal_ids(body);
        assert!(ids.is_empty());
    }

    #[test]
    fn strip_invalid_citations_removes_hallucinated_ids() {
        let valid = Uuid::new_v4();
        let invalid = Uuid::new_v4();
        let body = format!(
            "Cleanup planned [signal:{}] and more info [signal:{}] here.",
            valid, invalid
        );
        let valid_set: HashSet<Uuid> = [valid].into_iter().collect();

        let (cleaned, stripped) = strip_invalid_signal_citations(&body, &valid_set);

        assert_eq!(stripped, 1);
        assert!(cleaned.contains(&format!("[signal:{}]", valid)));
        assert!(!cleaned.contains(&format!("[signal:{}]", invalid)));
    }

    #[test]
    fn strip_preserves_non_signal_annotations() {
        let body = "[actor:someone] [signal:not-a-uuid] text";
        let valid_set: HashSet<Uuid> = HashSet::new();

        let (cleaned, stripped) = strip_invalid_signal_citations(body, &valid_set);

        // [signal:not-a-uuid] has an invalid UUID so strip_invalid_signal_citations
        // only processes parseable UUIDs — non-parseable ones are left alone
        assert_eq!(stripped, 0);
        assert!(cleaned.contains("[actor:someone]"));
    }

    #[test]
    fn strip_with_all_valid_citations_changes_nothing() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let body = format!("Info [signal:{}] and [signal:{}] here.", a, b);
        let valid_set: HashSet<Uuid> = [a, b].into_iter().collect();

        let (cleaned, stripped) = strip_invalid_signal_citations(&body, &valid_set);

        assert_eq!(stripped, 0);
        assert_eq!(cleaned, body);
    }
}
