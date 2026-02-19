//! Scenario 4: Information Desert — minimal sources in a small town.

use simweb::{Fact, Geography, JudgeCriteria, Site, SocialProfile, World};

pub fn world() -> World {
    World {
        name: "Rural Minnesota".to_string(),
        description: "Northfield, Minnesota — a small college town with very few online \
            sources. The town government has a basic website, there's one local newspaper \
            (the Northfield News), and one church runs a Facebook page that occasionally posts \
            about community events. Total: 3 sources. The challenge is producing accurate signals \
            from minimal input without hallucinating information that doesn't exist."
            .to_string(),
        facts: vec![
            Fact {
                text: "Northfield City Council meeting on February 25, 2026 at 7pm, City Hall, 801 Washington Street".to_string(),
                referenced_by: vec![
                    "https://www.ci.northfield.mn.us/meetings".to_string(),
                ],
                category: "community_event".to_string(),
            },
            Fact {
                text: "Northfield Area Food Shelf serves 200 families per month".to_string(),
                referenced_by: vec![
                    "https://www.northfieldnews.com/food-shelf-update".to_string(),
                ],
                category: "resource".to_string(),
            },
            Fact {
                text: "All Saints Episcopal Church hosts a free community meal every Wednesday at noon".to_string(),
                referenced_by: vec![
                    "https://www.facebook.com/allsaintsnorthfield".to_string(),
                ],
                category: "recurring_event".to_string(),
            },
        ],
        sites: vec![
            Site {
                url: "https://www.ci.northfield.mn.us/meetings".to_string(),
                kind: "government".to_string(),
                content_description: "City of Northfield meetings page. Lists upcoming city council \
                    meetings, planning commission, park board. Basic HTML, minimal styling.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 10).unwrap()),
                links_to: vec![],
            },
            Site {
                url: "https://www.northfieldnews.com/food-shelf-update".to_string(),
                kind: "news".to_string(),
                content_description: "Northfield News article about the food shelf's annual report. \
                    200 families served monthly, volunteer needs, upcoming fundraiser.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 1).unwrap()),
                links_to: vec![],
            },
        ],
        social_profiles: vec![
            SocialProfile {
                platform: "Facebook".to_string(),
                identifier: "allsaintsnorthfield".to_string(),
                persona: "Small church Facebook page. Posts about Sunday services, \
                    Wednesday community meals, occasional volunteer opportunities. \
                    Friendly, low-key tone.".to_string(),
                post_count: 5,
            },
        ],
        topics: vec![
            "community".to_string(),
            "Northfield".to_string(),
        ],
        geography: Geography {
            city: "Northfield".to_string(),
            state_or_region: "Minnesota".to_string(),
            country: "US".to_string(),
            local_terms: vec![
                "Northfield".to_string(),
                "Rice County".to_string(),
                "MN".to_string(),
                "Minnesota".to_string(),
            ],
            center_lat: 44.4583,
            center_lng: -93.1614,
        },
    }
}

pub fn criteria() -> JudgeCriteria {
    JudgeCriteria {
        checks: vec![
            "The agent should produce signals from the minimal sources available (city council meeting, food shelf, church meals) without hallucinating extra information.".to_string(),
            "Total signal count should be small (3-8 signals) reflecting the limited source material. Having 0 signals is a failure; having 15+ signals suggests hallucination.".to_string(),
            "Each signal should be traceable to a specific source — no signals that don't correspond to any site or social profile.".to_string(),
            "The city council meeting should be detected as an Event signal.".to_string(),
            "The food shelf should be detected as a Give/resource signal.".to_string(),
        ],
        pass_threshold: 0.6,
        critical_categories: vec![
            "hallucination".to_string(),
        ],
    }
}
