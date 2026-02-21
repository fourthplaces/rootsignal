//! Scenario: Tension Discovery Bridge
//!
//! Tests that mid-run discovery generates useful response-seeking queries after
//! finding tensions. Only tension sources are in the initial world — response
//! sources exist but must be found through discovery queries.

use simweb::{Fact, Geography, JudgeCriteria, Site, SocialProfile, World};

pub fn world() -> World {
    World {
        name: "Tension Discovery Bridge — St. Paul".to_string(),
        description: "A neighborhood with visible tensions (rent hikes, eviction notices) \
            but no response sources in the initial seed list. Organizations that help with \
            housing exist but should be discovered through mid-run discovery queries \
            triggered by the tensions found in Phase A."
            .to_string(),
        facts: vec![
            Fact {
                text: "Rents in the Frogtown neighborhood of St. Paul have increased 25% in the past year, \
                       with multiple buildings issuing 30-day rent increase notices."
                    .to_string(),
                referenced_by: vec![
                    "https://www.twincities.com/frogtown-rent-crisis".to_string(),
                    "https://www.reddit.com/r/SaintPaul/comments/frogtown_rent_hikes".to_string(),
                ],
                category: "tension".to_string(),
            },
            Fact {
                text: "At least 15 families in the Dale Street apartments received eviction notices \
                       in January 2026 after refusing to pay the increased rent."
                    .to_string(),
                referenced_by: vec![
                    "https://www.twincities.com/frogtown-rent-crisis".to_string(),
                ],
                category: "tension".to_string(),
            },
            Fact {
                text: "HOME Line provides free tenant hotline (612-728-5767) and legal assistance \
                       for renters facing eviction in Ramsey County."
                    .to_string(),
                referenced_by: vec![
                    "https://www.homelinemn.org/tenant-services".to_string(),
                ],
                category: "response".to_string(),
            },
            Fact {
                text: "Frogtown Neighborhood Association runs a tenant rights workshop every \
                       second Saturday at Frogtown Community Center."
                    .to_string(),
                referenced_by: vec![
                    "https://www.frogtownna.org/tenant-workshops".to_string(),
                ],
                category: "response".to_string(),
            },
        ],
        sites: vec![
            // Tension sources (these are in the initial seed)
            Site {
                url: "https://www.twincities.com/frogtown-rent-crisis".to_string(),
                kind: "news".to_string(),
                content_description: "Pioneer Press investigation into the Frogtown rent crisis. \
                    Documents 25% rent increases across multiple apartment buildings. Interviews with \
                    families facing eviction. Landlords cite rising property taxes and insurance. \
                    Community organizers say renters need legal help but don't know where to find it."
                    .to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 1).unwrap()),
                links_to: vec![],
            },
            // Response sources (NOT in initial seed — should be found by discovery)
            Site {
                url: "https://www.homelinemn.org/tenant-services".to_string(),
                kind: "nonprofit".to_string(),
                content_description: "HOME Line tenant services page. Free tenant hotline \
                    (612-728-5767) available M-F 9-5. Legal assistance for eviction defense. \
                    Know-your-rights workshops. Serving all of Minnesota. Special focus on \
                    Ramsey and Hennepin counties."
                    .to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 1, 10).unwrap()),
                links_to: vec![],
            },
            Site {
                url: "https://www.frogtownna.org/tenant-workshops".to_string(),
                kind: "community_org".to_string(),
                content_description: "Frogtown Neighborhood Association tenant rights workshop \
                    series. Every second Saturday at Frogtown Community Center, 10 AM-12 PM. \
                    Free. Topics include lease review, eviction defense, rent negotiation, \
                    and connecting with legal aid. Hmong and Somali interpreters available."
                    .to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 1, 25).unwrap()),
                links_to: vec![],
            },
        ],
        social_profiles: vec![
            SocialProfile {
                platform: "Reddit".to_string(),
                identifier: "r/SaintPaul".to_string(),
                persona: "St. Paul subreddit. Active threads about Frogtown rent increases. \
                    Tenants sharing eviction notice photos. Some users recommending HOME Line \
                    hotline. Discussion about tenant organizing."
                    .to_string(),
                post_count: 6,
            },
        ],
        topics: vec![
            "rent increase".to_string(),
            "eviction".to_string(),
            "tenant rights".to_string(),
            "Frogtown".to_string(),
        ],
        geography: Geography {
            name: "St. Paul".to_string(),
            state_or_region: "Minnesota".to_string(),
            country: "US".to_string(),
            local_terms: vec![
                "St. Paul".to_string(),
                "Saint Paul".to_string(),
                "Frogtown".to_string(),
                "Ramsey County".to_string(),
                "Dale Street".to_string(),
                "Twin Cities".to_string(),
                "Minnesota".to_string(),
                "MN".to_string(),
            ],
            center_lat: 44.9537,
            center_lng: -93.1050,
        },
    }
}

pub fn criteria() -> JudgeCriteria {
    JudgeCriteria {
        checks: vec![
            "Scout should extract at least one Tension signal about rent increases or evictions in Frogtown.".to_string(),
            "Scout should create discovery query sources seeking responses to the identified housing tensions.".to_string(),
            "Discovery queries should be specific to tenant rights, legal aid, or housing assistance — not generic.".to_string(),
        ],
        pass_threshold: 0.5, // Discovery is inherently exploratory
        critical_categories: vec![],
    }
}
