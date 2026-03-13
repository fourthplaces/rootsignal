#[cfg(test)]
mod tests {
    use rootsignal_common::types::SourceNode;

    use crate::testing::{actor_with_external_url, actor_without_external_url, make_source, social_source};
    use crate::domains::enrichment::activities::actor_serp_expansion;

    // ---------------------------------------------------------------
    // Happy path: actors that should trigger SERP expansion
    // ---------------------------------------------------------------

    #[test]
    fn actor_with_single_social_source_gets_search_query() {
        let actor = actor_without_external_url(
            "Sanctuary Supply",
            "instagram.com/sanctuarysupply",
        );
        let ig = social_source("https://www.instagram.com/sanctuarysupply");

        let queries = actor_serp_expansion::expand_actors_via_serp(
            &[(actor, vec![ig])],
            Some("Minneapolis"),
        );

        assert_eq!(queries.len(), 1);
        let q = &queries[0];
        assert!(q.canonical_value.contains("Sanctuary Supply"), "query should include actor name");
        assert!(q.canonical_value.contains("Minneapolis"), "query should include region");
        assert!(q.url.is_none(), "web queries have no URL");
    }

    #[test]
    fn query_works_without_region() {
        let actor = actor_without_external_url(
            "Mutual Aid Network",
            "instagram.com/mutualaidnetwork",
        );
        let ig = social_source("https://www.instagram.com/mutualaidnetwork");

        let queries = actor_serp_expansion::expand_actors_via_serp(
            &[(actor, vec![ig])],
            None,
        );

        assert_eq!(queries.len(), 1);
        assert!(queries[0].canonical_value.contains("Mutual Aid Network"));
    }

    // ---------------------------------------------------------------
    // Actors that should NOT trigger SERP expansion
    // ---------------------------------------------------------------

    #[test]
    fn actor_with_website_source_skipped() {
        let actor = actor_with_external_url(
            "Sanctuary Supply",
            "instagram.com/sanctuarysupply",
            "https://www.sanctuarysupply.org",
        );
        let ig = social_source("https://www.instagram.com/sanctuarysupply");
        let website = make_source("https://www.sanctuarysupply.org", "sanctuarysupply.org");

        let queries = actor_serp_expansion::expand_actors_via_serp(
            &[(actor, vec![ig, website])],
            Some("Minneapolis"),
        );

        assert!(queries.is_empty(), "actor with a website source doesn't need SERP expansion");
    }

    #[test]
    fn actor_with_multiple_social_sources_skipped() {
        let actor = actor_with_external_url(
            "Sanctuary Supply",
            "instagram.com/sanctuarysupply",
            "https://www.facebook.com/sanctuarysupply",
        );
        let ig = social_source("https://www.instagram.com/sanctuarysupply");
        let fb = social_source("https://www.facebook.com/sanctuarysupply");

        let queries = actor_serp_expansion::expand_actors_via_serp(
            &[(actor, vec![ig, fb])],
            Some("Minneapolis"),
        );

        assert!(queries.is_empty(), "actor with 2+ sources doesn't need SERP expansion");
    }

    // ---------------------------------------------------------------
    // Batch behavior
    // ---------------------------------------------------------------

    #[test]
    fn expansion_capped_per_run() {
        let actors: Vec<_> = (0..20).map(|i| {
            let name = format!("Org {i}");
            let ck = format!("instagram.com/org{i}");
            let actor = actor_without_external_url(&name, &ck);
            let source = social_source(&format!("https://www.instagram.com/org{i}"));
            (actor, vec![source])
        }).collect();

        let queries = actor_serp_expansion::expand_actors_via_serp(
            &actors,
            Some("Minneapolis"),
        );

        assert!(
            queries.len() <= actor_serp_expansion::MAX_SERP_QUERIES_PER_RUN,
            "should cap at {} queries, got {}",
            actor_serp_expansion::MAX_SERP_QUERIES_PER_RUN,
            queries.len(),
        );
    }

    #[test]
    fn mixed_batch_only_expands_eligible_actors() {
        let eligible = actor_without_external_url("Solo Source Org", "instagram.com/solo");
        let ig = social_source("https://www.instagram.com/solo");

        let covered = actor_with_external_url("Well Known Org", "instagram.com/wellknown", "https://wellknown.org");
        let ig2 = social_source("https://www.instagram.com/wellknown");
        let website = make_source("https://wellknown.org", "wellknown.org");

        let no_url = actor_without_external_url("No Sources Org", "instagram.com/nosources");
        let ig3 = social_source("https://www.instagram.com/nosources");

        let actors = vec![
            (eligible, vec![ig]),
            (covered, vec![ig2, website]),
            (no_url, vec![ig3]),
        ];

        let queries = actor_serp_expansion::expand_actors_via_serp(
            &actors,
            Some("Minneapolis"),
        );

        // eligible + no_url should expand, covered should not
        assert_eq!(queries.len(), 2, "only actors with a single source should expand");
    }

    // ---------------------------------------------------------------
    // Edge cases
    // ---------------------------------------------------------------

    #[test]
    fn single_webpage_source_not_expanded() {
        let actor = actor_without_external_url("Local News", "localnews.com");
        let web = make_source("https://localnews.com", "localnews.com");

        let queries = actor_serp_expansion::expand_actors_via_serp(
            &[(actor, vec![web])],
            Some("Minneapolis"),
        );

        assert!(queries.is_empty(), "actor whose only source is a website doesn't need SERP expansion");
    }

    #[test]
    fn empty_actor_name_skipped() {
        let actor = actor_without_external_url("", "instagram.com/blank");
        let ig = social_source("https://www.instagram.com/blank");

        let queries = actor_serp_expansion::expand_actors_via_serp(
            &[(actor, vec![ig])],
            Some("Minneapolis"),
        );

        assert!(queries.is_empty(), "actor with empty name should not generate a query");
    }
}
