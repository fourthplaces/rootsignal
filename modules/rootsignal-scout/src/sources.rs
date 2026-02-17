/// Per-city source configuration.
pub struct CityProfile {
    pub name: &'static str,
    pub default_lat: f64,
    pub default_lng: f64,
    pub geo_terms: Vec<&'static str>,
    pub tavily_queries: Vec<&'static str>,
    pub curated_sources: Vec<&'static str>,
    pub instagram_accounts: Vec<&'static str>,
    pub facebook_pages: Vec<&'static str>,
    pub reddit_subreddits: Vec<&'static str>,
    /// Platform-agnostic civic topics for hashtag/keyword discovery.
    /// Searched across all platforms with discovery adapters (Instagram first).
    pub discovery_topics: Vec<&'static str>,
    pub entity_mappings: Vec<EntityMapping>,
}

/// Maps social media accounts and domains to a parent entity (organization or individual).
/// Used for cross-source corroboration (same entity = same source, don't increment)
/// and for story promotion (multi-entity gate).
pub struct EntityMapping {
    pub entity_id: &'static str,
    pub entity_type: &'static str,
    pub domains: Vec<&'static str>,
    pub instagram: Vec<&'static str>,
    pub facebook: Vec<&'static str>,
    pub reddit: Vec<&'static str>,
}

impl EntityMapping {
    /// Convert to the shared owned type used across crates.
    pub fn to_owned(&self) -> rootsignal_common::EntityMappingOwned {
        rootsignal_common::EntityMappingOwned {
            entity_id: self.entity_id.to_string(),
            domains: self.domains.iter().map(|s| s.to_string()).collect(),
            instagram: self.instagram.iter().map(|s| s.to_string()).collect(),
            facebook: self.facebook.iter().map(|s| s.to_string()).collect(),
            reddit: self.reddit.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// Resolve a source URL to its parent entity ID using entity mappings.
/// Returns the entity_id if matched, otherwise extracts the domain as a fallback entity.
pub fn resolve_entity(url: &str, mappings: &[EntityMapping]) -> String {
    let domain = rootsignal_common::extract_domain(url);

    for mapping in mappings {
        // Check domain match
        for d in &mapping.domains {
            if domain.contains(d) {
                return mapping.entity_id.to_string();
            }
        }
        // Check Instagram
        for ig in &mapping.instagram {
            if url.contains(&format!("instagram.com/{ig}")) {
                return mapping.entity_id.to_string();
            }
        }
        // Check Facebook
        for fb in &mapping.facebook {
            if url.contains(fb) {
                return mapping.entity_id.to_string();
            }
        }
        // Check Reddit
        for r in &mapping.reddit {
            if url.contains(&format!("reddit.com/user/{r}")) || url.contains(&format!("reddit.com/u/{r}")) {
                return mapping.entity_id.to_string();
            }
        }
    }

    // Fallback: use the domain itself as the entity
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
        geo_terms: vec![
            "Minneapolis", "St. Paul", "Saint Paul", "Twin Cities",
            "Minnesota", "Hennepin", "Ramsey", "MN", "Mpls",
        ],
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
            "https://www.handsontwincities.org/opportunities",
            "https://www.minneapolisparks.org/activities-events/events/",
            "https://www.stpaul.gov/departments/parks-and-recreation/events",
            "https://www.minneapolismn.gov/government/city-council/meetings-agendas-minutes/",
            "https://www.stpaul.gov/departments/city-council/city-council-meetings",
            "https://www.gofundme.com/discover/search?q=minneapolis&location=Minneapolis%2C+MN",
            "https://www.gofundme.com/discover/search?q=st+paul&location=St%20Paul%2C+MN",
            "https://www.eventbrite.com/d/mn--minneapolis/community/",
            "https://www.eventbrite.com/d/mn--minneapolis/volunteer/",
            "https://www.eventbrite.com/d/mn--st-paul/community/",
            "https://www.meetup.com/find/?location=us--mn--Minneapolis&source=EVENTS&categoryId=&distance=tenMiles",
            "https://www.meetup.com/find/?location=us--mn--St%20Paul&source=EVENTS&categoryId=&distance=tenMiles",
            "https://patch.com/minnesota/minneapolis",
            "https://patch.com/minnesota/st-paul",
            "https://bsky.app/search?q=minneapolis+community",
            "https://bsky.app/search?q=twin+cities+volunteer",
            "https://bsky.app/search?q=minneapolis+ICE+immigration",
            "https://www.volunteermatch.org/search?l=Minneapolis%2C+MN&k=&v=true",
            "https://www.volunteermatch.org/search?l=St+Paul%2C+MN&k=&v=true",
            "https://www.idealist.org/en/volunteer-opportunities?areasOfFocus=COMMUNITY_DEVELOPMENT&q=&searchMode=true&location=Minneapolis%2C+MN&lat=44.9778&lng=-93.2650&radius=25",
            "https://www.justserve.org/projects?city=Minneapolis&state=MN",
            "https://www.youthlinkmn.org/",
            "https://thelinkmn.org/what-we-do/",
            "https://www.achievetwincities.org/what-we-do",
            "https://www.ywcampls.org/what-we-do",
            "https://seniorcommunity.org/our-services/",
            "https://www.neseniors.org/",
            "https://trellisconnects.org/get-help/",
            "https://www.hjcmn.org/our-work/",
            "https://homesforallmn.org/ways-to-be-involved/",
            "https://agatemn.org/programs/",
            "https://mphaonline.org/housing/programs/",
            "https://www.canmn.org/",
            "https://nhn-tc.org/",
            "https://www.gtcuw.org/volunteer/",
            "https://www.gtcuw.org/get-assistance/",
            // Community news
            "https://sahanjournal.com/",
            "https://www.minnpost.com/community-voices/",
            // Library systems — events and programs
            "https://www.hclib.org/events",
            "https://sppl.org/events/",
            // School districts
            "https://www.mpls.k12.mn.us/community",
            "https://www.spps.org/community",
            // MPR News — community/events coverage
            "https://www.mprnews.org/topic/community",
            // Watershed districts (ecological stewardship)
            "https://www.minnehahacreek.org/events",
            "https://www.capitolregionwd.org/events/",
        ],
        instagram_accounts: vec![
            "handsontwincities",
            "unitedwaytc",
            "tchabitat",
            "secondharvestheartland",
            "everymealorg",
            "loavesandfishesmn",
            "openarmsmn",
            "communityaidnetworkmn",
            "peopleservingpeople",
            "pillsburyunited",
            "minneapolisparks",
            "friendsmissriv",
            "parkconnection",
            "miracmn",
            "unidosmn",
            "immigrantlawcentermn",
            "cluesofficial",
            "voices4rj",
            "hclib",
            "stpaulpubliclibrary",
            "youthlinkmn",
            "achievetwincities",
            "ywcampls",
            "agateservicesmn",
            "communityaidnetworkmn",
            "nhn_tc",
            "sadeinthecities",
            "sj_tapp",
        ],
        facebook_pages: vec![
            "https://www.facebook.com/HandsOnTC",
            "https://www.facebook.com/unitedwaytwincities",
            "https://www.facebook.com/2harvest",
            "https://www.facebook.com/EveryMealOrg",
            "https://www.facebook.com/openarmsmn",
            "https://www.facebook.com/tchabitat",
            "https://www.facebook.com/FriendsMissRiv",
            "https://www.facebook.com/miracmn",
            "https://www.facebook.com/unidosmn",
            "https://www.facebook.com/immigrantlawcenterMN",
            "https://www.facebook.com/CLUESPage",
        ],
        reddit_subreddits: vec![
            "https://www.reddit.com/r/Minneapolis",
            "https://www.reddit.com/r/TwinCities",
            "https://www.reddit.com/r/SaintPaul",
        ],
        discovery_topics: vec![
            "MutualAidMPLS", "MutualAidMN", "VolunteerMN",
            "MinneapolisVolunteer", "TwinCitiesMutualAid",
            "MplsMutualAid", "SaintPaulMutualAid",
        ],
        entity_mappings: vec![
            EntityMapping {
                entity_id: "handsontwincities.org",
                entity_type: "org",
                domains: vec!["handsontwincities.org"],
                instagram: vec!["handsontwincities"],
                facebook: vec!["facebook.com/HandsOnTC"],
                reddit: vec![],
            },
            EntityMapping {
                entity_id: "gtcuw.org",
                entity_type: "org",
                domains: vec!["gtcuw.org"],
                instagram: vec!["unitedwaytc"],
                facebook: vec!["facebook.com/unitedwaytwincities"],
                reddit: vec![],
            },
            EntityMapping {
                entity_id: "2harvest.org",
                entity_type: "org",
                domains: vec!["2harvest.org"],
                instagram: vec!["secondharvestheartland"],
                facebook: vec!["facebook.com/2harvest"],
                reddit: vec![],
            },
            EntityMapping {
                entity_id: "everymeal.org",
                entity_type: "org",
                domains: vec!["everymeal.org", "everymealorg"],
                instagram: vec!["everymealorg"],
                facebook: vec!["facebook.com/EveryMealOrg"],
                reddit: vec![],
            },
            EntityMapping {
                entity_id: "openarmsmn.org",
                entity_type: "org",
                domains: vec!["openarmsmn.org"],
                instagram: vec!["openarmsmn"],
                facebook: vec!["facebook.com/openarmsmn"],
                reddit: vec![],
            },
            EntityMapping {
                entity_id: "tchabitat.org",
                entity_type: "org",
                domains: vec!["tchabitat.org"],
                instagram: vec!["tchabitat"],
                facebook: vec!["facebook.com/tchabitat"],
                reddit: vec![],
            },
            EntityMapping {
                entity_id: "fmr.org",
                entity_type: "org",
                domains: vec!["fmr.org"],
                instagram: vec!["friendsmissriv"],
                facebook: vec!["facebook.com/FriendsMissRiv"],
                reddit: vec![],
            },
            EntityMapping {
                entity_id: "mirac-mn.org",
                entity_type: "org",
                domains: vec!["mirac-mn.org", "miracmn"],
                instagram: vec!["miracmn"],
                facebook: vec!["facebook.com/miracmn"],
                reddit: vec![],
            },
            EntityMapping {
                entity_id: "unidosmn.org",
                entity_type: "org",
                domains: vec!["unidosmn.org"],
                instagram: vec!["unidosmn"],
                facebook: vec!["facebook.com/unidosmn"],
                reddit: vec![],
            },
            EntityMapping {
                entity_id: "ilcm.org",
                entity_type: "org",
                domains: vec!["ilcm.org", "immigrantlawcenter"],
                instagram: vec!["immigrantlawcentermn"],
                facebook: vec!["facebook.com/immigrantlawcenterMN"],
                reddit: vec![],
            },
            EntityMapping {
                entity_id: "clues.org",
                entity_type: "org",
                domains: vec!["clues.org"],
                instagram: vec!["cluesofficial"],
                facebook: vec!["facebook.com/CLUESPage"],
                reddit: vec![],
            },
            EntityMapping {
                entity_id: "loavesandfishesmn.org",
                entity_type: "org",
                domains: vec!["loavesandfishesmn.org"],
                instagram: vec!["loavesandfishesmn"],
                facebook: vec![],
                reddit: vec![],
            },
            EntityMapping {
                entity_id: "minneapolisparks.org",
                entity_type: "org",
                domains: vec!["minneapolisparks.org"],
                instagram: vec!["minneapolisparks"],
                facebook: vec![],
                reddit: vec![],
            },
            EntityMapping {
                entity_id: "pillsburyunited.org",
                entity_type: "org",
                domains: vec!["pillsburyunited.org"],
                instagram: vec!["pillsburyunited"],
                facebook: vec![],
                reddit: vec![],
            },
            EntityMapping {
                entity_id: "hclib.org",
                entity_type: "org",
                domains: vec!["hclib.org"],
                instagram: vec!["hclib"],
                facebook: vec![],
                reddit: vec![],
            },
            EntityMapping {
                entity_id: "sppl.org",
                entity_type: "org",
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
        geo_terms: vec![
            "New York", "NYC", "Brooklyn", "Manhattan", "Queens",
            "Bronx", "Staten Island", "Harlem", "NY",
        ],
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
            "https://www.nyc.gov/events",
            "https://www.eventbrite.com/d/ny--new-york/community/",
            "https://www.eventbrite.com/d/ny--new-york/volunteer/",
            "https://www.meetup.com/find/?location=us--ny--New+York&source=EVENTS&categoryId=&distance=tenMiles",
            "https://patch.com/new-york/new-york-city",
            "https://www.volunteermatch.org/search?l=New+York%2C+NY&k=&v=true",
            "https://www.idealist.org/en/volunteer-opportunities?areasOfFocus=COMMUNITY_DEVELOPMENT&q=&searchMode=true&location=New+York%2C+NY&lat=40.7128&lng=-74.0060&radius=25",
            "https://www.gofundme.com/discover/search?q=nyc&location=New+York%2C+NY",
        ],
        instagram_accounts: vec![],
        facebook_pages: vec![],
        reddit_subreddits: vec![
            "https://www.reddit.com/r/nyc",
        ],
        discovery_topics: vec![],
        entity_mappings: vec![],
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
        geo_terms: vec![
            "Portland", "Oregon", "Multnomah", "Clackamas",
            "Washington County", "OR",
        ],
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
            "https://www.portland.gov/events",
            "https://www.eventbrite.com/d/or--portland/community/",
            "https://www.eventbrite.com/d/or--portland/volunteer/",
            "https://www.meetup.com/find/?location=us--or--Portland&source=EVENTS&categoryId=&distance=tenMiles",
            "https://patch.com/oregon/portland",
            "https://www.volunteermatch.org/search?l=Portland%2C+OR&k=&v=true",
            "https://www.idealist.org/en/volunteer-opportunities?areasOfFocus=COMMUNITY_DEVELOPMENT&q=&searchMode=true&location=Portland%2C+OR&lat=45.5152&lng=-122.6784&radius=25",
            "https://www.gofundme.com/discover/search?q=portland&location=Portland%2C+OR",
        ],
        instagram_accounts: vec![],
        facebook_pages: vec![],
        reddit_subreddits: vec![
            "https://www.reddit.com/r/Portland",
        ],
        discovery_topics: vec![],
        entity_mappings: vec![],
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
        geo_terms: vec![
            "Berlin", "Kreuzberg", "Neukölln", "Friedrichshain",
            "Charlottenburg", "Mitte", "Prenzlauer Berg",
        ],
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
            "https://www.berlin.de/en/events/",
            "https://www.eventbrite.de/d/germany--berlin/community/",
            "https://www.eventbrite.de/d/germany--berlin/volunteer/",
            "https://www.meetup.com/find/?location=de--berlin&source=EVENTS&categoryId=&distance=tenMiles",
            "https://www.nebenan.de/berlin",
            "https://www.gofundme.com/discover/search?q=berlin&location=Berlin%2C+Germany",
        ],
        instagram_accounts: vec![],
        facebook_pages: vec![],
        reddit_subreddits: vec![
            "https://www.reddit.com/r/berlin",
        ],
        discovery_topics: vec![],
        entity_mappings: vec![],
    }
}


