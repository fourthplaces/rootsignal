//! Scenario 1: Temporal Confusion — stale vs. current sources in Minneapolis.

use simweb::{Fact, Geography, JudgeCriteria, Site, SocialProfile, World};

pub fn world() -> World {
    World {
        name: "Stale Minneapolis".to_string(),
        description: "Minneapolis neighborhood where half the sources are 6-18 months old. \
            A food shelf moved locations 8 months ago but the old address still appears on several \
            pages. An old community garden schedule (from last growing season) is still online. \
            Meanwhile, a new mutual aid group launched 2 weeks ago with active social media, \
            and a tenant union has been organizing for 3 months with recent press coverage."
            .to_string(),
        facts: vec![
            Fact {
                text: "North Minneapolis Food Shelf moved from 1200 Penn Ave N to 3400 Fremont Ave N in June 2025".to_string(),
                referenced_by: vec![
                    "https://www.northmpls-foodshelf.org/about".to_string(),
                    "https://www.minnpost.com/community/food-shelf-relocation".to_string(),
                ],
                category: "location_change".to_string(),
            },
            Fact {
                text: "Sumner-Glenwood Community Garden 2026 season starts April 15 with new plot assignments".to_string(),
                referenced_by: vec![
                    "https://www.sumner-glenwood-garden.org/schedule".to_string(),
                ],
                category: "current_schedule".to_string(),
            },
            Fact {
                text: "Northside Neighbors Mutual Aid launched January 30, 2026".to_string(),
                referenced_by: vec![
                    "https://www.instagram.com/northside_mutual_aid/".to_string(),
                ],
                category: "new_organization".to_string(),
            },
            Fact {
                text: "Lowry Avenue Tenant Union has filed 12 complaints with the city since November 2025".to_string(),
                referenced_by: vec![
                    "https://www.startribune.com/lowry-tenant-union-complaints".to_string(),
                ],
                category: "ongoing_organizing".to_string(),
            },
            Fact {
                text: "Old food shelf address 1200 Penn Ave N is permanently closed".to_string(),
                referenced_by: vec![
                    "https://www.northmpls-foodshelf.org/old-location".to_string(),
                ],
                category: "stale_info".to_string(),
            },
        ],
        sites: vec![
            // Stale sources (6-18 months old)
            Site {
                url: "https://www.northmpls-foodshelf.org/old-location".to_string(),
                kind: "nonprofit".to_string(),
                content_description: "Old food shelf page showing 1200 Penn Ave N address, hours M-F 9-5. \
                    Page last updated March 2025. Still indexed by Google.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2025, 3, 15).unwrap()),
                links_to: vec![],
            },
            Site {
                url: "https://www.sumner-glenwood-garden.org/2025-schedule".to_string(),
                kind: "community_org".to_string(),
                content_description: "2025 growing season schedule (April-October 2025). Plot assignments, \
                    volunteer days. Outdated but still accessible.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2025, 4, 1).unwrap()),
                links_to: vec![],
            },
            Site {
                url: "https://www.reddit.com/r/Minneapolis/comments/old_garden_post".to_string(),
                kind: "forum".to_string(),
                content_description: "Reddit post from 8 months ago asking about community garden plots. \
                    Contains old food shelf address in comments.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2025, 6, 10).unwrap()),
                links_to: vec![],
            },
            // Current sources
            Site {
                url: "https://www.northmpls-foodshelf.org/about".to_string(),
                kind: "nonprofit".to_string(),
                content_description: "Current food shelf page at 3400 Fremont Ave N. Updated hours, \
                    new programs including home delivery starting February 2026.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()),
                links_to: vec![],
            },
            Site {
                url: "https://www.sumner-glenwood-garden.org/schedule".to_string(),
                kind: "community_org".to_string(),
                content_description: "2026 growing season schedule. New plot assignments, orientation \
                    dates in March, first planting day April 15.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 1).unwrap()),
                links_to: vec![],
            },
            Site {
                url: "https://www.minnpost.com/community/food-shelf-relocation".to_string(),
                kind: "news".to_string(),
                content_description: "MinnPost article about the food shelf relocation, new programs, \
                    and expanded capacity at 3400 Fremont Ave N.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2025, 7, 20).unwrap()),
                links_to: vec!["https://www.northmpls-foodshelf.org/about".to_string()],
            },
            Site {
                url: "https://www.startribune.com/lowry-tenant-union-complaints".to_string(),
                kind: "news".to_string(),
                content_description: "Star Tribune coverage of Lowry Avenue Tenant Union filing \
                    12 complaints about building code violations. Quotes from tenants and landlord.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 5).unwrap()),
                links_to: vec![],
            },
        ],
        social_profiles: vec![
            SocialProfile {
                platform: "Instagram".to_string(),
                identifier: "northside_mutual_aid".to_string(),
                persona: "New mutual aid group, enthusiastic, posting about supply drives, \
                    neighbor check-ins, and upcoming events. Founded 2 weeks ago.".to_string(),
                post_count: 8,
            },
            SocialProfile {
                platform: "Reddit".to_string(),
                identifier: "r/Minneapolis".to_string(),
                persona: "City subreddit. Mix of questions about resources, complaints about \
                    landlords, and community event announcements.".to_string(),
                post_count: 10,
            },
        ],
        topics: vec![
            "mutual aid".to_string(),
            "food shelf".to_string(),
            "tenant rights".to_string(),
            "community garden".to_string(),
        ],
        geography: Geography {
            name: "Minneapolis".to_string(),
            state_or_region: "Minnesota".to_string(),
            country: "US".to_string(),
            local_terms: vec![
                "Minneapolis".to_string(),
                "North Minneapolis".to_string(),
                "Northside".to_string(),
                "Lowry".to_string(),
                "Sumner-Glenwood".to_string(),
                "Hennepin".to_string(),
                "MN".to_string(),
                "Mpls".to_string(),
            ],
            center_lat: 44.9778,
            center_lng: -93.2650,
        },
    }
}

pub fn criteria() -> JudgeCriteria {
    JudgeCriteria {
        checks: vec![
            "Stale sources (2025 dates) should NOT produce high-confidence signals. Any signal from 1200 Penn Ave N should be low confidence or absent.".to_string(),
            "The old food shelf address (1200 Penn Ave N) should NOT appear in any high-confidence signal. The current address (3400 Fremont Ave N) should appear instead.".to_string(),
            "Current sources (2026 dates) should rank higher in confidence than stale sources.".to_string(),
            "The new mutual aid group (Northside Neighbors Mutual Aid, launched Jan 30 2026) should be detected as a signal.".to_string(),
            "The Lowry Avenue Tenant Union complaints should produce at least one Tension signal.".to_string(),
            "The 2026 community garden schedule should be preferred over the 2025 schedule.".to_string(),
        ],
        pass_threshold: 0.6,
        critical_categories: vec![
            "stale_info".to_string(),
            "wrong_address".to_string(),
        ],
    }
}
