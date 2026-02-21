// Shared utility functions and constants for the scout module.
//
// These were consolidated from duplicate implementations across scout.rs,
// investigator.rs, response_finder.rs, and gathering_finder.rs.

/// Seeds deserve faster first scrapes; `compute_weight` takes over after first scrape.
pub const COLD_START_SOURCE_WEIGHT: f64 = 0.5;

/// Default weight for LLM-discovered and actor-discovered sources.
pub const DISCOVERED_SOURCE_WEIGHT: f64 = 0.3;

/// Reddit posts are shorter; subreddits have many authors.
pub const REDDIT_POST_LIMIT: u32 = 20;

/// Standard limit for single-org social accounts.
pub const SOCIAL_POST_LIMIT: u32 = 10;

/// Shared tension category list for LLM prompts. These are guidance, not constraints —
/// the LLM may propose categories outside this list per Principle 13 ("Emergent Over Engineered").
pub const TENSION_CATEGORIES: &str =
    "housing, safety, economic, health, education, infrastructure, \
environment, social, governance, immigration, civil_rights, other";

// Re-export content_hash from common for backwards compatibility within scout.
pub use rootsignal_common::content_hash;

/// Cosine similarity between two f64 vectors. Returns 0.0 for zero-norm inputs.
pub fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

/// Strip tracking parameters from URLs that may contain PII or cause dedup mismatches.
pub fn sanitize_url(url: &str) -> String {
    const TRACKING_PARAMS: &[&str] = &[
        "_dt",
        "fbclid",
        "gclid",
        "utm_source",
        "utm_medium",
        "utm_campaign",
        "utm_term",
        "utm_content",
        "modal",
        "ref",
        "mc_cid",
        "mc_eid",
    ];

    let Ok(mut parsed) = url::Url::parse(url) else {
        return url.to_string();
    };

    let had_query = parsed.query().is_some();
    if !had_query {
        return url.to_string();
    }

    let clean_pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(key, _)| !TRACKING_PARAMS.contains(&key.as_ref()))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    if clean_pairs.is_empty() {
        parsed.set_query(None);
    } else {
        parsed.query_pairs_mut().clear().extend_pairs(clean_pairs);
    }

    parsed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_deterministic() {
        let h1 = content_hash("hello world");
        let h2 = content_hash("hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn content_hash_different_inputs() {
        let h1 = content_hash("hello");
        let h2 = content_hash("world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn cosine_similarity_identical() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);
    }

    #[test]
    fn cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let c = vec![0.0, 1.0, 0.0];
        assert!(cosine_similarity(&a, &c).abs() < 0.001);
    }

    #[test]
    fn cosine_similarity_zero_vector() {
        let a = vec![1.0, 0.0, 0.0];
        let d = vec![0.0, 0.0, 0.0];
        assert!(cosine_similarity(&a, &d).abs() < 0.001);
    }

    #[test]
    fn sanitize_url_strips_tracking() {
        let url = "https://example.com/page?id=123&utm_source=twitter&fbclid=abc";
        let clean = sanitize_url(url);
        assert!(clean.contains("id=123"));
        assert!(!clean.contains("utm_source"));
        assert!(!clean.contains("fbclid"));
    }

    #[test]
    fn sanitize_url_preserves_clean_urls() {
        let url = "https://example.com/page?id=123";
        assert_eq!(sanitize_url(url), url);
    }

    #[test]
    fn sanitize_url_handles_no_query() {
        let url = "https://example.com/page";
        assert_eq!(sanitize_url(url), url);
    }

    #[test]
    fn sanitize_url_removes_all_tracking() {
        let url = "https://example.com/page?utm_source=x&utm_medium=y";
        let clean = sanitize_url(url);
        assert!(!clean.contains('?'));
    }
}
