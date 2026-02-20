use rootsignal_common::canonical_value;

/// Build a canonical key: `city_slug:canonical_value`.
pub fn make_canonical_key(city_slug: &str, value: &str) -> String {
    let cv = canonical_value(value);
    format!("{}:{}", city_slug, cv)
}
