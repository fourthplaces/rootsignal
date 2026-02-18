//! Scenario 2: Legitimate vs. Astroturf — Portland organizing campaigns.

use simweb::{Fact, Geography, JudgeCriteria, Site, SocialProfile, World};

pub fn world() -> World {
    World {
        name: "Organizing Portland".to_string(),
        description: "Portland, Oregon neighborhood with two competing narratives. \
            The Burnside Tenant Union is a legitimate 501(c)(3) with 5 years of history, \
            Oregonian press coverage, and a coordinated social campaign about rent stabilization. \
            Pearl District Partners is a developer-backed astroturf group (no 501(c)(3), PR-firm-linked, \
            formed 3 months ago) running a similar-looking coordinated campaign about 'community investment' \
            that's actually opposing rent stabilization."
            .to_string(),
        facts: vec![
            Fact {
                text: "Burnside Tenant Union is a registered 501(c)(3) nonprofit, founded 2021".to_string(),
                referenced_by: vec![
                    "https://www.burnsidetenantunion.org/about".to_string(),
                    "https://www.oregonlive.com/portland/burnside-tenant-victory".to_string(),
                ],
                category: "organization_legitimacy".to_string(),
            },
            Fact {
                text: "Pearl District Partners was registered as an LLC in November 2025 by Cascade Development Group".to_string(),
                referenced_by: vec![
                    "https://www.wweek.com/pearl-district-partners-investigation".to_string(),
                ],
                category: "astroturf_indicator".to_string(),
            },
            Fact {
                text: "Portland City Council hearing on rent stabilization ordinance scheduled for March 15, 2026".to_string(),
                referenced_by: vec![
                    "https://www.portland.gov/bds/rent-stabilization-hearing".to_string(),
                ],
                category: "civic_event".to_string(),
            },
            Fact {
                text: "Burnside Tenant Union has won 3 code enforcement cases since 2023".to_string(),
                referenced_by: vec![
                    "https://www.oregonlive.com/portland/burnside-tenant-victory".to_string(),
                ],
                category: "track_record".to_string(),
            },
            Fact {
                text: "Pearl District Partners social media posts use identical phrasing across accounts, suggesting coordinated PR".to_string(),
                referenced_by: vec![
                    "https://www.wweek.com/pearl-district-partners-investigation".to_string(),
                ],
                category: "astroturf_indicator".to_string(),
            },
        ],
        sites: vec![
            Site {
                url: "https://www.burnsidetenantunion.org/about".to_string(),
                kind: "nonprofit".to_string(),
                content_description: "Burnside Tenant Union about page. History since 2021, \
                    501(c)(3) status, board members, mission statement focused on tenant rights.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2024, 1, 10).unwrap()),
                links_to: vec![],
            },
            Site {
                url: "https://www.burnsidetenantunion.org/campaign".to_string(),
                kind: "nonprofit".to_string(),
                content_description: "Current rent stabilization campaign page. Calls to action for \
                    March 15 hearing, tenant testimonials, fact sheets.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 1).unwrap()),
                links_to: vec!["https://www.portland.gov/bds/rent-stabilization-hearing".to_string()],
            },
            Site {
                url: "https://www.pearldistrict-partners.com".to_string(),
                kind: "advocacy".to_string(),
                content_description: "Pearl District Partners landing page. Slick design, talks about \
                    'community investment' and 'responsible growth'. No history, no board listed, \
                    no 501(c)(3) status. Founded late 2025.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2025, 11, 15).unwrap()),
                links_to: vec![],
            },
            Site {
                url: "https://www.oregonlive.com/portland/burnside-tenant-victory".to_string(),
                kind: "news".to_string(),
                content_description: "Oregonian article about Burnside Tenant Union winning a major \
                    code enforcement case. Quotes from tenants and union organizers.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2025, 9, 20).unwrap()),
                links_to: vec!["https://www.burnsidetenantunion.org/about".to_string()],
            },
            Site {
                url: "https://www.wweek.com/pearl-district-partners-investigation".to_string(),
                kind: "news".to_string(),
                content_description: "Willamette Week investigative piece revealing Pearl District Partners \
                    was founded by Cascade Development Group and uses PR firm-coordinated messaging.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 10).unwrap()),
                links_to: vec![],
            },
            Site {
                url: "https://www.portland.gov/bds/rent-stabilization-hearing".to_string(),
                kind: "government".to_string(),
                content_description: "Portland City Council page for the March 15 rent stabilization \
                    hearing. Public comment instructions, agenda, relevant ordinance text.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 8).unwrap()),
                links_to: vec![],
            },
        ],
        social_profiles: vec![
            SocialProfile {
                platform: "Instagram".to_string(),
                identifier: "burnside_tenant_union".to_string(),
                persona: "Established tenant rights org. Posts about meetings, victories, \
                    and calls to action for the rent stabilization hearing. Authentic grassroots tone.".to_string(),
                post_count: 15,
            },
            SocialProfile {
                platform: "Instagram".to_string(),
                identifier: "pearldistrict_partners".to_string(),
                persona: "Developer-backed group. Polished posts about 'community investment' and \
                    'balanced growth'. Opposes rent stabilization without saying so directly. \
                    Identical phrasing across posts suggesting PR coordination.".to_string(),
                post_count: 12,
            },
            SocialProfile {
                platform: "Reddit".to_string(),
                identifier: "r/Portland".to_string(),
                persona: "City subreddit. Heated debate about rent stabilization, with some \
                    accounts posting Pearl District Partners talking points verbatim.".to_string(),
                post_count: 8,
            },
        ],
        topics: vec![
            "rent stabilization".to_string(),
            "tenant rights".to_string(),
            "housing".to_string(),
        ],
        geography: Geography {
            city: "Portland".to_string(),
            state_or_region: "Oregon".to_string(),
            country: "US".to_string(),
            local_terms: vec![
                "Portland".to_string(),
                "Burnside".to_string(),
                "Pearl District".to_string(),
                "Multnomah".to_string(),
                "PDX".to_string(),
                "Oregon".to_string(),
            ],
            center_lat: 45.5152,
            center_lng: -122.6784,
        },
    }
}

pub fn criteria() -> JudgeCriteria {
    JudgeCriteria {
        checks: vec![
            "Both the Burnside Tenant Union campaign and Pearl District Partners campaign should be detected as signals.".to_string(),
            "The Burnside Tenant Union (501(c)(3), 5 years old, news coverage) should have higher confidence than Pearl District Partners.".to_string(),
            "Pearl District Partners should be flagged as low-confidence or have indicators of astroturf (no history, PR-coordinated, developer-linked).".to_string(),
            "The March 15 rent stabilization hearing should be detected as an Event signal.".to_string(),
            "The agent should detect the housing tension around rent stabilization.".to_string(),
        ],
        pass_threshold: 0.6,
        critical_categories: vec![
            "astroturf_detection".to_string(),
        ],
    }
}
