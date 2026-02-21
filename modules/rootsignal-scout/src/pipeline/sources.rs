use rootsignal_common::canonical_value;

/// Build a canonical key from a source value.
/// The key is the canonical_value itself — region-independent.
pub fn make_canonical_key(value: &str) -> String {
    canonical_value(value)
}
