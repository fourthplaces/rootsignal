//! Scenario 3: Slow-Burn Tension — scattered complaints in Cedar-Riverside.

use simweb::{Fact, Geography, JudgeCriteria, Site, SocialProfile, World};

pub fn world() -> World {
    World {
        name: "Simmering Cedar-Riverside".to_string(),
        description: "Cedar-Riverside neighborhood in Minneapolis where no single crisis article \
            exists, but scattered complaints across Reddit, tweets, and one blog post all point \
            to the same pattern: rent increases of 15-25% over the past 6 months across multiple \
            buildings owned by the same management company (Riverside Property Management). \
            No organized response yet — just individual residents expressing frustration."
            .to_string(),
        facts: vec![
            Fact {
                text: "Riverside Property Management raised rents 15-25% across Cedar-Riverside buildings in 2025-2026".to_string(),
                referenced_by: vec![
                    "https://www.reddit.com/r/Minneapolis/comments/cedar_riverside_rent_1".to_string(),
                    "https://www.reddit.com/r/Minneapolis/comments/cedar_riverside_rent_2".to_string(),
                    "https://cedarriverside-voice.org/rent-increases-hitting-hard".to_string(),
                ],
                category: "tension_pattern".to_string(),
            },
            Fact {
                text: "At least 4 buildings affected: Riverside Towers, Cedar Square West, Currie Park Apartments, and The Oaks".to_string(),
                referenced_by: vec![
                    "https://www.reddit.com/r/Minneapolis/comments/cedar_riverside_rent_1".to_string(),
                    "https://www.reddit.com/r/Minneapolis/comments/cedar_riverside_rent_2".to_string(),
                ],
                category: "scope".to_string(),
            },
            Fact {
                text: "Several residents are East African immigrants unfamiliar with tenant rights processes".to_string(),
                referenced_by: vec![
                    "https://cedarriverside-voice.org/rent-increases-hitting-hard".to_string(),
                ],
                category: "vulnerability".to_string(),
            },
        ],
        sites: vec![
            Site {
                url: "https://www.reddit.com/r/Minneapolis/comments/cedar_riverside_rent_1".to_string(),
                kind: "forum".to_string(),
                content_description: "Reddit post from a Riverside Towers resident about 20% rent \
                    increase notice. Several comments from other Cedar-Riverside residents saying \
                    they got similar notices. Posted 3 months ago.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2025, 11, 15).unwrap()),
                links_to: vec![],
            },
            Site {
                url: "https://www.reddit.com/r/Minneapolis/comments/cedar_riverside_rent_2".to_string(),
                kind: "forum".to_string(),
                content_description: "Reddit post from Cedar Square West resident asking if others \
                    are dealing with Riverside Property Management rent hikes. Mentions 25% increase. \
                    Posted 6 weeks ago.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 1, 5).unwrap()),
                links_to: vec![],
            },
            Site {
                url: "https://cedarriverside-voice.org/rent-increases-hitting-hard".to_string(),
                kind: "community_blog".to_string(),
                content_description: "Blog post from Cedar-Riverside community voice about rent \
                    increases affecting immigrant families. Interviews with 3 residents. Mentions \
                    Riverside Property Management by name. Published 2 weeks ago.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 4).unwrap()),
                links_to: vec![],
            },
        ],
        social_profiles: vec![
            SocialProfile {
                platform: "Reddit".to_string(),
                identifier: "r/Minneapolis".to_string(),
                persona: "City subreddit. Multiple unrelated users posting about Cedar-Riverside \
                    rent increases over the past few months. No organized campaign — just individual \
                    complaints that happen to point to the same management company.".to_string(),
                post_count: 6,
            },
            SocialProfile {
                platform: "Instagram".to_string(),
                identifier: "cedarriverside_voice".to_string(),
                persona: "Small community blog account. Posts about neighborhood life, occasionally \
                    shares articles about local issues including housing.".to_string(),
                post_count: 4,
            },
        ],
        topics: vec![
            "rent increase".to_string(),
            "Cedar-Riverside".to_string(),
            "tenant rights".to_string(),
        ],
        geography: Geography {
            name: "Minneapolis".to_string(),
            state_or_region: "Minnesota".to_string(),
            country: "US".to_string(),
            local_terms: vec![
                "Minneapolis".to_string(),
                "Cedar-Riverside".to_string(),
                "Cedar".to_string(),
                "Riverside".to_string(),
                "West Bank".to_string(),
                "Hennepin".to_string(),
                "MN".to_string(),
            ],
            center_lat: 44.9692,
            center_lng: -93.2540,
        },
    }
}

pub fn criteria() -> JudgeCriteria {
    JudgeCriteria {
        checks: vec![
            "The agent should assemble a Tension signal about rent increases in Cedar-Riverside, even though no single source tells the complete story.".to_string(),
            "Individual complaints from different Reddit posts should corroborate into a pattern rather than being treated as isolated incidents.".to_string(),
            "No single source should dominate — the tension should be recognized as emerging from multiple independent reports.".to_string(),
            "Riverside Property Management should be identified as a key actor.".to_string(),
            "The vulnerability of immigrant residents should be captured in the signal if the source material mentions it.".to_string(),
        ],
        pass_threshold: 0.5,
        critical_categories: vec![
            "pattern_detection".to_string(),
        ],
    }
}
