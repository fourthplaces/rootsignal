//! Scenario 6: Active Change — organizations in transition.

use simweb::{Fact, Geography, JudgeCriteria, Site, SocialProfile, World};

pub fn world() -> World {
    World {
        name: "Shifting Ground".to_string(),
        description: "St. Paul neighborhood experiencing multiple simultaneous transitions. \
            A new food shelf is opening on the East Side (announced 1 week ago) while an old one \
            on the West Side is closing (website still up with hours listed). A community garden \
            is relocating from Frogtown to Dayton's Bluff (two addresses in circulation). \
            A long-running after-school program is expanding to a second location. \
            The challenge: detecting transitions and not silently merging conflicting info."
            .to_string(),
        facts: vec![
            Fact {
                text: "East Side Community Food Shelf opens March 1, 2026 at 795 East 7th Street".to_string(),
                referenced_by: vec![
                    "https://www.twincities.com/east-side-food-shelf-opening".to_string(),
                    "https://www.instagram.com/eastside_community_hub/".to_string(),
                ],
                category: "new_resource".to_string(),
            },
            Fact {
                text: "West Side Harvest Food Shelf closing permanently on February 28, 2026".to_string(),
                referenced_by: vec![
                    "https://www.westsideharvest.org".to_string(),
                ],
                category: "closing_resource".to_string(),
            },
            Fact {
                text: "Frogtown Community Garden relocating from 456 Dale Street to 890 Mounds Blvd in Dayton's Bluff".to_string(),
                referenced_by: vec![
                    "https://www.frogtown-garden.org/relocation".to_string(),
                ],
                category: "relocation".to_string(),
            },
            Fact {
                text: "West Side Harvest website still shows hours as Mon-Fri 10am-6pm despite impending closure".to_string(),
                referenced_by: vec![
                    "https://www.westsideharvest.org".to_string(),
                ],
                category: "stale_info".to_string(),
            },
            Fact {
                text: "Summit Academy after-school program expanding from 935 Selby Ave to second location at 1100 University Ave".to_string(),
                referenced_by: vec![
                    "https://www.summitacademy-stpaul.org/expansion".to_string(),
                ],
                category: "expansion".to_string(),
            },
        ],
        sites: vec![
            Site {
                url: "https://www.twincities.com/east-side-food-shelf-opening".to_string(),
                kind: "news".to_string(),
                content_description: "Pioneer Press article about the new East Side Community Food Shelf \
                    opening March 1 at 795 East 7th Street. Profiles the organizers, mentions \
                    the West Side closure as context.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 11).unwrap()),
                links_to: vec![],
            },
            Site {
                url: "https://www.westsideharvest.org".to_string(),
                kind: "nonprofit".to_string(),
                content_description: "West Side Harvest Food Shelf website. Still shows regular hours \
                    and services. A small banner at the top mentions closure on Feb 28 but the rest \
                    of the page is unchanged.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2025, 6, 1).unwrap()),
                links_to: vec![],
            },
            Site {
                url: "https://www.frogtown-garden.org/relocation".to_string(),
                kind: "community_org".to_string(),
                content_description: "Frogtown Community Garden relocation announcement. Moving from \
                    456 Dale Street to 890 Mounds Blvd in Dayton's Bluff. New plots available, \
                    orientation in March.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 5).unwrap()),
                links_to: vec![],
            },
            Site {
                url: "https://www.frogtown-garden.org".to_string(),
                kind: "community_org".to_string(),
                content_description: "Frogtown Community Garden main page. Still shows 456 Dale Street \
                    as the address. Mentions the garden has been at this location since 2018.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2025, 3, 1).unwrap()),
                links_to: vec!["https://www.frogtown-garden.org/relocation".to_string()],
            },
            Site {
                url: "https://www.summitacademy-stpaul.org/expansion".to_string(),
                kind: "nonprofit".to_string(),
                content_description: "Summit Academy announcing expansion of after-school program to \
                    a second location at 1100 University Ave, starting March 2026.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 3).unwrap()),
                links_to: vec![],
            },
        ],
        social_profiles: vec![
            SocialProfile {
                platform: "Instagram".to_string(),
                identifier: "eastside_community_hub".to_string(),
                persona: "New East Side community hub account. Posting about the upcoming food shelf \
                    opening, volunteer opportunities, and the March 1 launch.".to_string(),
                post_count: 6,
            },
            SocialProfile {
                platform: "Facebook".to_string(),
                identifier: "frogtown_neighbors".to_string(),
                persona: "Frogtown neighborhood page. Discussion about the garden relocation — \
                    some excitement about the new space, some sadness about leaving Dale Street.".to_string(),
                post_count: 5,
            },
        ],
        topics: vec![
            "food shelf".to_string(),
            "community garden".to_string(),
            "after-school".to_string(),
        ],
        geography: Geography {
            name: "St. Paul".to_string(),
            state_or_region: "Minnesota".to_string(),
            country: "US".to_string(),
            local_terms: vec![
                "St. Paul".to_string(),
                "Saint Paul".to_string(),
                "East Side".to_string(),
                "West Side".to_string(),
                "Frogtown".to_string(),
                "Dayton's Bluff".to_string(),
                "Ramsey".to_string(),
                "MN".to_string(),
            ],
            center_lat: 44.9537,
            center_lng: -93.0900,
        },
    }
}

pub fn criteria() -> JudgeCriteria {
    JudgeCriteria {
        checks: vec![
            "The new food shelf (East Side, opening March 1) and closing food shelf (West Side Harvest, closing Feb 28) should be detected as SEPARATE signals, not merged into one.".to_string(),
            "The community garden should show the relocation — both addresses should appear in signals or evidence, not just one.".to_string(),
            "Conflicting information (old address vs new address) should surface as distinct signals rather than being silently resolved.".to_string(),
            "The new food shelf opening should be higher confidence than the closing food shelf (whose website shows stale hours).".to_string(),
            "The Summit Academy expansion should be detected as a distinct signal.".to_string(),
        ],
        pass_threshold: 0.5,
        critical_categories: vec![
            "silent_merge".to_string(),
            "transition_detection".to_string(),
        ],
    }
}
