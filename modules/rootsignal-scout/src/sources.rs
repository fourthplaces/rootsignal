/// Per-city source configuration.
pub struct CityProfile {
    pub name: &'static str,
    pub default_lat: f64,
    pub default_lng: f64,
    pub tavily_queries: Vec<&'static str>,
    pub curated_sources: Vec<(&'static str, f32)>,
    pub instagram_accounts: Vec<(&'static str, f32)>,
    pub facebook_pages: Vec<(&'static str, f32)>,
    pub reddit_subreddits: Vec<(&'static str, f32)>,
    pub org_mappings: Vec<OrgMapping>,
}

/// Maps social media accounts and domains to a parent organization.
/// Used for cross-source corroboration (same org = same source, don't increment)
/// and for story promotion (multi-org gate).
pub struct OrgMapping {
    pub org_id: &'static str,
    pub domains: Vec<&'static str>,
    pub instagram: Vec<&'static str>,
    pub facebook: Vec<&'static str>,
    pub reddit: Vec<&'static str>,
}

/// Resolve a source URL to its parent organization ID using org mappings.
/// Returns the org_id if matched, otherwise extracts the domain as a fallback org.
pub fn resolve_org(url: &str, mappings: &[OrgMapping]) -> String {
    let domain = extract_domain(url);

    for mapping in mappings {
        // Check domain match
        for d in &mapping.domains {
            if domain.contains(d) {
                return mapping.org_id.to_string();
            }
        }
        // Check Instagram
        for ig in &mapping.instagram {
            if url.contains(&format!("instagram.com/{ig}")) {
                return mapping.org_id.to_string();
            }
        }
        // Check Facebook
        for fb in &mapping.facebook {
            if url.contains(fb) {
                return mapping.org_id.to_string();
            }
        }
        // Check Reddit
        for r in &mapping.reddit {
            if url.contains(&format!("reddit.com/user/{r}")) || url.contains(&format!("reddit.com/u/{r}")) {
                return mapping.org_id.to_string();
            }
        }
    }

    // Fallback: use the domain itself as the org
    domain
}

/// Build a CityProfile for the given city key.
/// Panics if the city is not recognized.
pub fn city_profile(city: &str) -> CityProfile {
    match city {
        "twincities" => twincities_profile(),
        "nyc" => nyc_profile(),
        "portland" => portland_profile(),
        "berlin" => berlin_profile(),
        other => panic!("Unknown city: {other}. Supported: twincities, nyc, portland, berlin"),
    }
}

// ---------------------------------------------------------------------------
// Twin Cities (existing, unchanged)
// ---------------------------------------------------------------------------

fn twincities_profile() -> CityProfile {
    CityProfile {
        name: "Twin Cities (Minneapolis-St. Paul, Minnesota)",
        default_lat: 44.9778,
        default_lng: -93.2650,
        tavily_queries: vec![
            "Twin Cities volunteer opportunities 2026",
            "Minneapolis food shelf food bank hours",
            "St Paul mutual aid community support",
            "Twin Cities community events this week",
            "Minneapolis park cleanup environmental volunteer",
            "Twin Cities ecological restoration citizen science",
            "Minnesota river lake cleanup volunteer",
            "Minneapolis city council public hearing 2026",
            "St Paul planning commission meeting",
            "Twin Cities civic participation advocacy",
            "Twin Cities repair cafe tool library",
            "Minneapolis food co-op local market",
            "Twin Cities GoFundMe community fundraiser",
            "Minneapolis St Paul volunteers needed",
            "Minneapolis zoning housing development dispute",
            "Twin Cities school board education policy",
            "Minneapolis ICE immigration enforcement raid",
            "Twin Cities immigrant community response sanctuary",
            "Twin Cities youth programs mentoring employment",
            "Minneapolis youth services drop-in center",
            "Minneapolis senior services programs resources",
            "Twin Cities senior care aging services",
            "Minneapolis housing advocacy tenant rights",
            "Twin Cities affordable housing homelessness services",
            "Twin Cities mutual aid network community support 2026",
            // Community news sources
            "site:sahanjournal.com Minneapolis St Paul community",
            "site:minnpost.com community events resources",
            // Public infrastructure
            "Minneapolis Public Schools community events 2026",
            "St Paul Public Schools community meetings 2026",
            "Hennepin County Library events programs 2026",
            "Minnesota watershed district meetings volunteer",
        ],
        curated_sources: vec![
            ("https://www.handsontwincities.org/opportunities", 0.85),
            ("https://www.minneapolisparks.org/activities-events/events/", 0.9),
            ("https://www.stpaul.gov/departments/parks-and-recreation/events", 0.9),
            ("https://www.minneapolismn.gov/government/city-council/meetings-agendas-minutes/", 0.9),
            ("https://www.stpaul.gov/departments/city-council/city-council-meetings", 0.9),
            ("https://www.gofundme.com/discover/search?q=minneapolis&location=Minneapolis%2C+MN", 0.5),
            ("https://www.gofundme.com/discover/search?q=st+paul&location=St%20Paul%2C+MN", 0.5),
            ("https://www.eventbrite.com/d/mn--minneapolis/community/", 0.75),
            ("https://www.eventbrite.com/d/mn--minneapolis/volunteer/", 0.75),
            ("https://www.eventbrite.com/d/mn--st-paul/community/", 0.75),
            ("https://www.meetup.com/find/?location=us--mn--Minneapolis&source=EVENTS&categoryId=&distance=tenMiles", 0.7),
            ("https://www.meetup.com/find/?location=us--mn--St%20Paul&source=EVENTS&categoryId=&distance=tenMiles", 0.7),
            ("https://patch.com/minnesota/minneapolis", 0.65),
            ("https://patch.com/minnesota/st-paul", 0.65),
            ("https://bsky.app/search?q=minneapolis+community", 0.4),
            ("https://bsky.app/search?q=twin+cities+volunteer", 0.4),
            ("https://bsky.app/search?q=minneapolis+ICE+immigration", 0.4),
            ("https://www.volunteermatch.org/search?l=Minneapolis%2C+MN&k=&v=true", 0.75),
            ("https://www.volunteermatch.org/search?l=St+Paul%2C+MN&k=&v=true", 0.75),
            ("https://www.idealist.org/en/volunteer-opportunities?areasOfFocus=COMMUNITY_DEVELOPMENT&q=&searchMode=true&location=Minneapolis%2C+MN&lat=44.9778&lng=-93.2650&radius=25", 0.7),
            ("https://www.justserve.org/projects?city=Minneapolis&state=MN", 0.7),
            ("https://www.youthlinkmn.org/", 0.75),
            ("https://thelinkmn.org/what-we-do/", 0.75),
            ("https://www.achievetwincities.org/what-we-do", 0.75),
            ("https://www.ywcampls.org/what-we-do", 0.8),
            ("https://seniorcommunity.org/our-services/", 0.75),
            ("https://www.neseniors.org/", 0.65),
            ("https://trellisconnects.org/get-help/", 0.75),
            ("https://www.hjcmn.org/our-work/", 0.75),
            ("https://homesforallmn.org/ways-to-be-involved/", 0.7),
            ("https://agatemn.org/programs/", 0.75),
            ("https://mphaonline.org/housing/programs/", 0.8),
            ("https://www.canmn.org/", 0.65),
            ("https://nhn-tc.org/", 0.65),
            ("https://www.gtcuw.org/volunteer/", 0.8),
            ("https://www.gtcuw.org/get-assistance/", 0.8),
            // Community news
            ("https://sahanjournal.com/", 0.8),
            ("https://www.minnpost.com/community-voices/", 0.75),
            // Library systems — events and programs
            ("https://www.hclib.org/events", 0.85),
            ("https://sppl.org/events/", 0.85),
            // School districts
            ("https://www.mpls.k12.mn.us/community", 0.85),
            ("https://www.spps.org/community", 0.85),
            // MPR News — community/events coverage
            ("https://www.mprnews.org/topic/community", 0.85),
            // Watershed districts (ecological stewardship)
            ("https://www.minnehahacreek.org/events", 0.8),
            ("https://www.capitolregionwd.org/events/", 0.8),
        ],
        instagram_accounts: vec![
            ("handsontwincities", 0.7),
            ("unitedwaytc", 0.7),
            ("tchabitat", 0.7),
            ("secondharvestheartland", 0.7),
            ("everymealorg", 0.7),
            ("loavesandfishesmn", 0.6),
            ("openarmsmn", 0.7),
            ("communityaidnetworkmn", 0.6),
            ("peopleservingpeople", 0.6),
            ("pillsburyunited", 0.7),
            ("minneapolisparks", 0.7),
            ("friendsmissriv", 0.6),
            ("parkconnection", 0.6),
            ("miracmn", 0.5),
            ("unidosmn", 0.5),
            ("immigrantlawcentermn", 0.6),
            ("cluesofficial", 0.6),
            ("voices4rj", 0.5),
            ("hclib", 0.7),
            ("stpaulpubliclibrary", 0.7),
            ("youthlinkmn", 0.7),
            ("achievetwincities", 0.7),
            ("ywcampls", 0.7),
            ("agateservicesmn", 0.7),
            ("communityaidnetworkmn", 0.6),
            ("nhn_tc", 0.6),
        ],
        facebook_pages: vec![
            ("https://www.facebook.com/HandsOnTC", 0.7),
            ("https://www.facebook.com/unitedwaytwincities", 0.7),
            ("https://www.facebook.com/2harvest", 0.7),
            ("https://www.facebook.com/EveryMealOrg", 0.7),
            ("https://www.facebook.com/openarmsmn", 0.7),
            ("https://www.facebook.com/tchabitat", 0.7),
            ("https://www.facebook.com/FriendsMissRiv", 0.6),
            ("https://www.facebook.com/miracmn", 0.5),
            ("https://www.facebook.com/unidosmn", 0.5),
            ("https://www.facebook.com/immigrantlawcenterMN", 0.6),
            ("https://www.facebook.com/CLUESPage", 0.6),
        ],
        reddit_subreddits: vec![
            ("https://www.reddit.com/r/Minneapolis", 0.4),
            ("https://www.reddit.com/r/TwinCities", 0.4),
            ("https://www.reddit.com/r/SaintPaul", 0.4),
        ],
        org_mappings: vec![
            OrgMapping {
                org_id: "handsontwincities.org",
                domains: vec!["handsontwincities.org"],
                instagram: vec!["handsontwincities"],
                facebook: vec!["facebook.com/HandsOnTC"],
                reddit: vec![],
            },
            OrgMapping {
                org_id: "gtcuw.org",
                domains: vec!["gtcuw.org"],
                instagram: vec!["unitedwaytc"],
                facebook: vec!["facebook.com/unitedwaytwincities"],
                reddit: vec![],
            },
            OrgMapping {
                org_id: "2harvest.org",
                domains: vec!["2harvest.org"],
                instagram: vec!["secondharvestheartland"],
                facebook: vec!["facebook.com/2harvest"],
                reddit: vec![],
            },
            OrgMapping {
                org_id: "everymeal.org",
                domains: vec!["everymeal.org", "everymealorg"],
                instagram: vec!["everymealorg"],
                facebook: vec!["facebook.com/EveryMealOrg"],
                reddit: vec![],
            },
            OrgMapping {
                org_id: "openarmsmn.org",
                domains: vec!["openarmsmn.org"],
                instagram: vec!["openarmsmn"],
                facebook: vec!["facebook.com/openarmsmn"],
                reddit: vec![],
            },
            OrgMapping {
                org_id: "tchabitat.org",
                domains: vec!["tchabitat.org"],
                instagram: vec!["tchabitat"],
                facebook: vec!["facebook.com/tchabitat"],
                reddit: vec![],
            },
            OrgMapping {
                org_id: "fmr.org",
                domains: vec!["fmr.org"],
                instagram: vec!["friendsmissriv"],
                facebook: vec!["facebook.com/FriendsMissRiv"],
                reddit: vec![],
            },
            OrgMapping {
                org_id: "mirac-mn.org",
                domains: vec!["mirac-mn.org", "miracmn"],
                instagram: vec!["miracmn"],
                facebook: vec!["facebook.com/miracmn"],
                reddit: vec![],
            },
            OrgMapping {
                org_id: "unidosmn.org",
                domains: vec!["unidosmn.org"],
                instagram: vec!["unidosmn"],
                facebook: vec!["facebook.com/unidosmn"],
                reddit: vec![],
            },
            OrgMapping {
                org_id: "ilcm.org",
                domains: vec!["ilcm.org", "immigrantlawcenter"],
                instagram: vec!["immigrantlawcentermn"],
                facebook: vec!["facebook.com/immigrantlawcenterMN"],
                reddit: vec![],
            },
            OrgMapping {
                org_id: "clues.org",
                domains: vec!["clues.org"],
                instagram: vec!["cluesofficial"],
                facebook: vec!["facebook.com/CLUESPage"],
                reddit: vec![],
            },
            OrgMapping {
                org_id: "loavesandfishesmn.org",
                domains: vec!["loavesandfishesmn.org"],
                instagram: vec!["loavesandfishesmn"],
                facebook: vec![],
                reddit: vec![],
            },
            OrgMapping {
                org_id: "minneapolisparks.org",
                domains: vec!["minneapolisparks.org"],
                instagram: vec!["minneapolisparks"],
                facebook: vec![],
                reddit: vec![],
            },
            OrgMapping {
                org_id: "pillsburyunited.org",
                domains: vec!["pillsburyunited.org"],
                instagram: vec!["pillsburyunited"],
                facebook: vec![],
                reddit: vec![],
            },
            OrgMapping {
                org_id: "hclib.org",
                domains: vec!["hclib.org"],
                instagram: vec!["hclib"],
                facebook: vec![],
                reddit: vec![],
            },
            OrgMapping {
                org_id: "sppl.org",
                domains: vec!["sppl.org"],
                instagram: vec!["stpaulpubliclibrary"],
                facebook: vec![],
                reddit: vec![],
            },
        ],
    }
}

// ---------------------------------------------------------------------------
// NYC
// ---------------------------------------------------------------------------

fn nyc_profile() -> CityProfile {
    CityProfile {
        name: "New York City",
        default_lat: 40.7128,
        default_lng: -74.0060,
        tavily_queries: vec![
            "NYC volunteer opportunities 2026",
            "New York City food bank pantry hours",
            "NYC mutual aid community support",
            "NYC community events this week",
            "NYC park cleanup environmental volunteer",
            "New York City community garden volunteer",
            "NYC city council public hearing 2026",
            "NYC community board meeting 2026",
            "NYC civic participation advocacy",
            "NYC repair cafe tool library",
            "NYC GoFundMe community fundraiser",
            "NYC volunteers needed",
            "NYC zoning housing development dispute",
            "NYC school board education policy",
            "NYC ICE immigration enforcement",
            "NYC immigrant community response sanctuary",
            "NYC youth programs mentoring employment",
            "NYC senior services programs resources",
            "NYC housing advocacy tenant rights",
            "NYC affordable housing homelessness services",
            "NYC mutual aid network community support 2026",
        ],
        curated_sources: vec![
            ("https://www.nyc.gov/events", 0.9),
            ("https://www.eventbrite.com/d/ny--new-york/community/", 0.75),
            ("https://www.eventbrite.com/d/ny--new-york/volunteer/", 0.75),
            ("https://www.meetup.com/find/?location=us--ny--New+York&source=EVENTS&categoryId=&distance=tenMiles", 0.7),
            ("https://patch.com/new-york/new-york-city", 0.65),
            ("https://www.volunteermatch.org/search?l=New+York%2C+NY&k=&v=true", 0.75),
            ("https://www.idealist.org/en/volunteer-opportunities?areasOfFocus=COMMUNITY_DEVELOPMENT&q=&searchMode=true&location=New+York%2C+NY&lat=40.7128&lng=-74.0060&radius=25", 0.7),
            ("https://www.gofundme.com/discover/search?q=nyc&location=New+York%2C+NY", 0.5),
        ],
        instagram_accounts: vec![],
        facebook_pages: vec![],
        reddit_subreddits: vec![
            ("https://www.reddit.com/r/nyc", 0.4),
        ],
        org_mappings: vec![],
    }
}

// ---------------------------------------------------------------------------
// Portland
// ---------------------------------------------------------------------------

fn portland_profile() -> CityProfile {
    CityProfile {
        name: "Portland, Oregon",
        default_lat: 45.5152,
        default_lng: -122.6784,
        tavily_queries: vec![
            "Portland Oregon volunteer opportunities 2026",
            "Portland food bank pantry hours",
            "Portland Oregon mutual aid community support",
            "Portland community events this week",
            "Portland park cleanup environmental volunteer",
            "Portland community garden volunteer",
            "Portland city council meeting 2026",
            "Portland neighborhood association meeting",
            "Portland civic participation advocacy",
            "Portland repair cafe tool library",
            "Portland GoFundMe community fundraiser",
            "Portland Oregon volunteers needed",
            "Portland zoning housing development dispute",
            "Portland school board education policy",
            "Portland immigrant community resources",
            "Portland youth programs mentoring employment",
            "Portland senior services programs resources",
            "Portland housing advocacy tenant rights",
            "Portland affordable housing homelessness services",
            "Portland mutual aid network community support 2026",
        ],
        curated_sources: vec![
            ("https://www.portland.gov/events", 0.9),
            ("https://www.eventbrite.com/d/or--portland/community/", 0.75),
            ("https://www.eventbrite.com/d/or--portland/volunteer/", 0.75),
            ("https://www.meetup.com/find/?location=us--or--Portland&source=EVENTS&categoryId=&distance=tenMiles", 0.7),
            ("https://patch.com/oregon/portland", 0.65),
            ("https://www.volunteermatch.org/search?l=Portland%2C+OR&k=&v=true", 0.75),
            ("https://www.idealist.org/en/volunteer-opportunities?areasOfFocus=COMMUNITY_DEVELOPMENT&q=&searchMode=true&location=Portland%2C+OR&lat=45.5152&lng=-122.6784&radius=25", 0.7),
            ("https://www.gofundme.com/discover/search?q=portland&location=Portland%2C+OR", 0.5),
        ],
        instagram_accounts: vec![],
        facebook_pages: vec![],
        reddit_subreddits: vec![
            ("https://www.reddit.com/r/Portland", 0.4),
        ],
        org_mappings: vec![],
    }
}

// ---------------------------------------------------------------------------
// Berlin
// ---------------------------------------------------------------------------

fn berlin_profile() -> CityProfile {
    CityProfile {
        name: "Berlin, Germany",
        default_lat: 52.5200,
        default_lng: 13.4050,
        tavily_queries: vec![
            "Berlin volunteer opportunities 2026",
            "Berlin community events this week",
            "Berlin Bürgeramt public meeting",
            "Berlin neighborhood initiatives Kiezprojekte",
            "Berlin food bank Tafel hours",
            "Berlin mutual aid community support",
            "Berlin park cleanup environmental volunteer",
            "Berlin community garden volunteer",
            "Berlin civic participation Bürgerbeteiligung",
            "Berlin repair cafe",
            "Berlin GoFundMe community fundraiser",
            "Berlin volunteers needed ehrenamt",
            "Berlin housing protest tenant rights Mieterschutz",
            "Berlin immigrant community resources integration",
            "Berlin youth programs mentoring",
            "Berlin senior services Seniorenberatung",
            "Berlin affordable housing homelessness services",
            "Berlin mutual aid network 2026",
        ],
        curated_sources: vec![
            ("https://www.berlin.de/en/events/", 0.9),
            ("https://www.eventbrite.de/d/germany--berlin/community/", 0.75),
            ("https://www.eventbrite.de/d/germany--berlin/volunteer/", 0.75),
            ("https://www.meetup.com/find/?location=de--berlin&source=EVENTS&categoryId=&distance=tenMiles", 0.7),
            ("https://www.nebenan.de/berlin", 0.65),
            ("https://www.gofundme.com/discover/search?q=berlin&location=Berlin%2C+Germany", 0.5),
        ],
        instagram_accounts: vec![],
        facebook_pages: vec![],
        reddit_subreddits: vec![
            ("https://www.reddit.com/r/berlin", 0.4),
        ],
        org_mappings: vec![],
    }
}

// ---------------------------------------------------------------------------
// Source trust (domain-based, city-agnostic with city-specific entries)
// ---------------------------------------------------------------------------

/// Source trust scores — baseline trust for different source types.
pub fn source_trust(url: &str) -> f32 {
    let domain = extract_domain(url);
    match domain {
        // Government (.gov) — highest trust
        d if d.ends_with(".gov") => 0.9,
        // Established city/county sites — Twin Cities
        d if d.contains("minneapolismn.gov") || d.contains("stpaul.gov") || d.contains("hennepin.us") || d.contains("ramseycounty.us") => 0.9,
        // NYC government
        d if d.contains("nyc.gov") => 0.9,
        // Portland government
        d if d.contains("portland.gov") => 0.9,
        // Berlin government
        d if d.contains("berlin.de") => 0.9,
        // Established nonprofits and org sites
        d if d.ends_with(".org") => 0.8,
        // Event platforms
        d if d.contains("eventbrite.com") || d.contains("eventbrite.de") => 0.75,
        d if d.contains("meetup.com") => 0.7,
        // News outlets — Twin Cities
        d if d.contains("startribune.com") || d.contains("mprnews.org") || d.contains("swtimes.com") || d.contains("minnpost.com") || d.contains("sahanjournal.com") => 0.85,
        // Libraries
        d if d.contains("hclib.org") || d.contains("sppl.org") => 0.85,
        // School districts
        d if d.contains("mpls.k12.mn.us") || d.contains("spps.org") => 0.85,
        // Watershed districts
        d if d.contains("minnehahacreek.org") || d.contains("capitolregionwd.org") => 0.8,
        // News outlets — Portland
        d if d.contains("oregonlive.com") => 0.85,
        // German neighborhood platform
        d if d.contains("nebenan.de") => 0.65,
        // Fundraising
        d if d.contains("gofundme.com") => 0.5,
        // Public-first social platforms
        d if d.contains("reddit.com") => 0.4,
        d if d.contains("bsky.app") => 0.4,
        // Walled social media (scraped via Apify)
        d if d.contains("facebook.com") || d.contains("instagram.com") || d.contains("twitter.com") || d.contains("x.com") => 0.3,
        // Volunteer platforms
        d if d.contains("volunteermatch.org") => 0.75,
        d if d.contains("idealist.org") => 0.7,
        d if d.contains("justserve.org") => 0.7,
        // Local news aggregator
        d if d.contains("patch.com") => 0.65,
        // Default
        _ => 0.5,
    }
}

fn extract_domain(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("")
        .to_lowercase()
}
