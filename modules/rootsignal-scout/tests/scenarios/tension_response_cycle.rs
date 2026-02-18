//! Scenario: Tension-Response Cycle
//!
//! Tests the two-phase pipeline: Phase A finds tensions (youth violence, lack of safe spaces),
//! Phase B finds responses (after-school programs, mentorship, community centers).
//! Verifies that RESPONDS_TO edges connect responses to tensions.

use simweb::{Fact, Geography, JudgeCriteria, Site, SocialProfile, World};

pub fn world() -> World {
    World {
        name: "Tension Response Cycle — North Minneapolis".to_string(),
        description: "A neighborhood experiencing a spike in youth violence and a lack of safe \
            after-school spaces. Multiple organizations offer programs that directly address \
            these tensions but are not widely known. The scout should find both the problems \
            and the responses, then link them."
            .to_string(),
        facts: vec![
            Fact {
                text: "Youth violence incidents in North Minneapolis increased 40% in the last 6 months, \
                       concentrated around Penn and Lowry avenues after school hours (3-6 PM)."
                    .to_string(),
                referenced_by: vec![
                    "https://www.startribune.com/north-mpls-youth-violence-spike".to_string(),
                    "https://www.reddit.com/r/Minneapolis/comments/youth_violence_north".to_string(),
                ],
                category: "tension".to_string(),
            },
            Fact {
                text: "Parents and community members report there are no safe spaces for teens \
                       after school in the Penn-Lowry corridor."
                    .to_string(),
                referenced_by: vec![
                    "https://www.reddit.com/r/Minneapolis/comments/youth_violence_north".to_string(),
                    "https://www.minnpost.com/community/northside-youth-safety".to_string(),
                ],
                category: "tension".to_string(),
            },
            Fact {
                text: "Northside Achievement Zone runs free after-school tutoring and mentorship \
                       at 1620 Penn Ave N, M-F 3:30-6:30 PM."
                    .to_string(),
                referenced_by: vec![
                    "https://www.northsideachievement.org/programs".to_string(),
                ],
                category: "response".to_string(),
            },
            Fact {
                text: "Plymouth Christian Youth Center (PCYC) offers drop-in basketball, arts, \
                       and homework help for ages 12-18 at 2021 Plymouth Ave N."
                    .to_string(),
                referenced_by: vec![
                    "https://www.pcyc-mpls.org/youth-programs".to_string(),
                ],
                category: "response".to_string(),
            },
            Fact {
                text: "Minneapolis Park Board launched 'Safe Spaces After School' pilot in January 2026 \
                       at North Commons Park with supervised activities until 7 PM."
                    .to_string(),
                referenced_by: vec![
                    "https://www.minneapolisparks.org/safe-spaces-pilot".to_string(),
                ],
                category: "response".to_string(),
            },
        ],
        sites: vec![
            // Tension sources
            Site {
                url: "https://www.startribune.com/north-mpls-youth-violence-spike".to_string(),
                kind: "news".to_string(),
                content_description: "Star Tribune investigative piece on the 40% spike in youth \
                    violence incidents in North Minneapolis. Interviews with police, school officials, \
                    and parents. Focus on the 3-6 PM danger window after school lets out. \
                    Community leaders call for more after-school programs and safe spaces."
                    .to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 10).unwrap()),
                links_to: vec![],
            },
            Site {
                url: "https://www.minnpost.com/community/northside-youth-safety".to_string(),
                kind: "news".to_string(),
                content_description: "MinnPost community voices column. Parents describe fear of \
                    letting kids walk home from school. Two mothers organized a walking bus but \
                    say it's not enough. 'We need places where our kids can go, not just streets \
                    to avoid.' Calls for investment in youth programming."
                    .to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 5).unwrap()),
                links_to: vec![],
            },
            // Response sources
            Site {
                url: "https://www.northsideachievement.org/programs".to_string(),
                kind: "nonprofit".to_string(),
                content_description: "Northside Achievement Zone programs page. Free after-school \
                    tutoring and mentorship program at 1620 Penn Ave N, Monday-Friday 3:30-6:30 PM. \
                    Also offers family engagement workshops and college prep. Serves youth ages 10-18. \
                    Transportation available from nearby schools."
                    .to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()),
                links_to: vec![],
            },
            Site {
                url: "https://www.pcyc-mpls.org/youth-programs".to_string(),
                kind: "nonprofit".to_string(),
                content_description: "Plymouth Christian Youth Center youth programs. Drop-in \
                    basketball league, arts studio, and homework help center for ages 12-18. \
                    Located at 2021 Plymouth Ave N. Open weekdays 3-8 PM, Saturdays 10 AM-4 PM. \
                    All programs free. Mentorship matching available."
                    .to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 1, 20).unwrap()),
                links_to: vec![],
            },
            Site {
                url: "https://www.minneapolisparks.org/safe-spaces-pilot".to_string(),
                kind: "government".to_string(),
                content_description: "Minneapolis Park Board announcement: Safe Spaces After School \
                    pilot program at North Commons Park. Supervised activities including sports, \
                    art, and study space until 7 PM on weekdays. Launched January 2026. Staffed by \
                    trained youth workers. Free for all Minneapolis residents under 18."
                    .to_string(),
                published: Some(chrono::NaiveDate::from_ymd_opt(2026, 1, 8).unwrap()),
                links_to: vec![],
            },
        ],
        social_profiles: vec![
            SocialProfile {
                platform: "Reddit".to_string(),
                identifier: "r/Minneapolis".to_string(),
                persona: "City subreddit. Recent threads about youth violence spike in North Minneapolis. \
                    Parents asking for after-school program recommendations. Some users mention \
                    PCYC and NAZ as resources. Heated debate about root causes."
                    .to_string(),
                post_count: 8,
            },
        ],
        topics: vec![
            "youth violence".to_string(),
            "after school programs".to_string(),
            "teen safe spaces".to_string(),
            "North Minneapolis".to_string(),
        ],
        geography: Geography {
            city: "Minneapolis".to_string(),
            state_or_region: "Minnesota".to_string(),
            country: "US".to_string(),
            local_terms: vec![
                "Minneapolis".to_string(),
                "North Minneapolis".to_string(),
                "Northside".to_string(),
                "Penn Ave".to_string(),
                "Lowry".to_string(),
                "Plymouth Ave".to_string(),
                "North Commons".to_string(),
                "MN".to_string(),
                "Mpls".to_string(),
            ],
            center_lat: 44.9978,
            center_lng: -93.2913,
        },
    }
}

pub fn criteria() -> JudgeCriteria {
    JudgeCriteria {
        checks: vec![
            "Scout should extract at least one Tension signal about youth violence or lack of safe spaces for teens.".to_string(),
            "Scout should extract at least one Give or Event signal about after-school programs, mentorship, or youth safe spaces.".to_string(),
            "A RESPONDS_TO edge should connect a response signal (Give/Event) to a tension about youth violence or safe spaces.".to_string(),
            "The response signals should reference specific organizations or programs (NAZ, PCYC, Safe Spaces pilot).".to_string(),
        ],
        pass_threshold: 0.6,
        critical_categories: vec![
            "missed_tension".to_string(),
            "missed_response".to_string(),
            "no_responds_to".to_string(),
        ],
    }
}
