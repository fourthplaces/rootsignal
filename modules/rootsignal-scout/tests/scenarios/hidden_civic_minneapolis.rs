//! Scenario 5: Invisible Civic Spaces — civic life in non-obvious places.

use simweb::{Fact, Geography, JudgeCriteria, Site, SocialProfile, World};

pub fn world() -> World {
    World {
        name: "Hidden Civic Minneapolis".to_string(),
        description: "Minneapolis neighborhood where civic life happens in non-obvious places. \
            A barbershop on Lake Street runs voter registration drives (mentioned in an Instagram post). \
            A church basement hosts weekly immigrant rights meetings (mentioned in a community blog). \
            A laundromat has a bulletin board that's the neighborhood's de facto community hub \
            (mentioned in a Google review excerpt). None of these are .org or .gov sites."
            .to_string(),
        facts: vec![
            Fact {
                text: "Ray's Barbershop at 3201 Lake Street runs voter registration every Saturday in election season".to_string(),
                referenced_by: vec![
                    "https://www.instagram.com/lakest_community/".to_string(),
                ],
                category: "informal_civic".to_string(),
            },
            Fact {
                text: "Holy Rosary Church basement hosts immigrant rights meetings every Tuesday at 7pm".to_string(),
                referenced_by: vec![
                    "https://powderhorn-voice.org/hidden-community-spaces".to_string(),
                ],
                category: "informal_civic".to_string(),
            },
            Fact {
                text: "Sunshine Laundromat at 2845 Bloomington Ave has a community bulletin board used for mutual aid requests, lost pets, and event flyers".to_string(),
                referenced_by: vec![
                    "https://www.reddit.com/r/Minneapolis/comments/laundromat_community".to_string(),
                ],
                category: "informal_civic".to_string(),
            },
            Fact {
                text: "These three locations form an informal civic infrastructure network in Powderhorn/Phillips neighborhoods".to_string(),
                referenced_by: vec![
                    "https://powderhorn-voice.org/hidden-community-spaces".to_string(),
                ],
                category: "civic_infrastructure".to_string(),
            },
        ],
        sites: vec![
            Site {
                url: "https://powderhorn-voice.org/hidden-community-spaces".to_string(),
                kind: "community_blog".to_string(),
                content_description: "Blog post titled 'The Hidden Civic Spaces of South Minneapolis'. \
                    Profiles Ray's Barbershop voter registration, Holy Rosary immigrant rights meetings, \
                    and other informal community gathering spots.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 1, 25).unwrap()),
                links_to: vec![],
            },
            Site {
                url: "https://www.reddit.com/r/Minneapolis/comments/laundromat_community".to_string(),
                kind: "forum".to_string(),
                content_description: "Reddit post about Sunshine Laundromat's community bulletin board. \
                    Commenters share stories of finding help through the board — babysitting swaps, \
                    free furniture, ESL class flyers.".to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 8).unwrap()),
                links_to: vec![],
            },
        ],
        social_profiles: vec![
            SocialProfile {
                platform: "Instagram".to_string(),
                identifier: "lakest_community".to_string(),
                persona: "Lake Street community account. Posts about local businesses, \
                    events, and civic activities including the barbershop voter registration. \
                    Mix of English and Spanish.".to_string(),
                post_count: 8,
            },
            SocialProfile {
                platform: "Facebook".to_string(),
                identifier: "powderhorn_neighbors".to_string(),
                persona: "Neighborhood Facebook group. Posts about meetings, resources, \
                    and community events. Mentions church basement meetings.".to_string(),
                post_count: 6,
            },
        ],
        topics: vec![
            "voter registration".to_string(),
            "immigrant rights".to_string(),
            "mutual aid".to_string(),
            "community spaces".to_string(),
        ],
        geography: Geography {
            city: "Minneapolis".to_string(),
            state_or_region: "Minnesota".to_string(),
            country: "US".to_string(),
            local_terms: vec![
                "Minneapolis".to_string(),
                "Lake Street".to_string(),
                "Powderhorn".to_string(),
                "Phillips".to_string(),
                "South Minneapolis".to_string(),
                "Hennepin".to_string(),
                "MN".to_string(),
            ],
            center_lat: 44.9489,
            center_lng: -93.2573,
        },
    }
}

pub fn criteria() -> JudgeCriteria {
    JudgeCriteria {
        checks: vec![
            "The agent should detect civic activity from informal sources (barbershop, church basement, laundromat) even though none are .org or .gov sites.".to_string(),
            "The voter registration at Ray's Barbershop should produce a signal (Event or Give).".to_string(),
            "The immigrant rights meetings at Holy Rosary Church should produce a signal (Event).".to_string(),
            "The agent should NOT require formal institutional sources to recognize civic infrastructure.".to_string(),
            "Signals from informal sources may have lower confidence, but they should still be detected.".to_string(),
        ],
        pass_threshold: 0.5,
        critical_categories: vec![
            "informal_civic_detection".to_string(),
        ],
    }
}
