/// Source trust scores — baseline trust for different source types.
pub fn source_trust(url: &str) -> f32 {
    let domain = extract_domain(url);
    match domain {
        // Government (.gov) — highest trust
        d if d.ends_with(".gov") => 0.9,
        // Established city/county sites
        d if d.contains("minneapolismn.gov") || d.contains("stpaul.gov") || d.contains("hennepin.us") || d.contains("ramseycounty.us") => 0.9,
        // Established nonprofits and org sites
        d if d.ends_with(".org") => 0.8,
        // Event platforms
        d if d.contains("eventbrite.com") => 0.75,
        d if d.contains("meetup.com") => 0.7,
        // News outlets
        d if d.contains("startribune.com") || d.contains("mprnews.org") || d.contains("swtimes.com") || d.contains("minnpost.com") => 0.85,
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

/// Tavily search queries for diverse civic signal types in Twin Cities.
/// Covers all engagement types from the civic engagement landscape doc.
pub fn tavily_queries() -> Vec<&'static str> {
    vec![
        // Human needs
        "Twin Cities volunteer opportunities 2026",
        "Minneapolis food shelf food bank hours",
        "St Paul mutual aid community support",
        "Twin Cities community events this week",
        // Ecological stewardship
        "Minneapolis park cleanup environmental volunteer",
        "Twin Cities ecological restoration citizen science",
        "Minnesota river lake cleanup volunteer",
        // Civic engagement
        "Minneapolis city council public hearing 2026",
        "St Paul planning commission meeting",
        "Twin Cities civic participation advocacy",
        // Ethical consumption / local economy
        "Twin Cities repair cafe tool library",
        "Minneapolis food co-op local market",
        // Community asks / needs
        "Twin Cities GoFundMe community fundraiser",
        "Minneapolis St Paul volunteers needed",
        // Tension / civic context
        "Minneapolis zoning housing development dispute",
        "Twin Cities school board education policy",
        // Immigration / enforcement
        "Minneapolis ICE immigration enforcement raid",
        "Twin Cities immigrant community response sanctuary",
    ]
}

/// Curated organization websites to scrape directly.
/// These are high-trust, high-signal sources.
pub fn curated_sources() -> Vec<(&'static str, f32)> {
    vec![
        // Nonprofits / community orgs
        ("https://www.handsontwincities.org/opportunities", 0.85),
        ("https://www.minneapolisparks.org/activities-events/events/", 0.9),
        ("https://www.stpaul.gov/departments/parks-and-recreation/events", 0.9),
        ("https://www.minneapolismn.gov/government/city-council/meetings-agendas-minutes/", 0.9),
        ("https://www.stpaul.gov/departments/city-council/city-council-meetings", 0.9),
        // GoFundMe — community fundraisers (Ask signals)
        ("https://www.gofundme.com/discover/search?q=minneapolis&location=Minneapolis%2C+MN", 0.5),
        ("https://www.gofundme.com/discover/search?q=st+paul&location=St%20Paul%2C+MN", 0.5),
        // Eventbrite — community & volunteer events
        ("https://www.eventbrite.com/d/mn--minneapolis/community/", 0.75),
        ("https://www.eventbrite.com/d/mn--minneapolis/volunteer/", 0.75),
        ("https://www.eventbrite.com/d/mn--st-paul/community/", 0.75),
        // Meetup — community events
        ("https://www.meetup.com/find/?location=us--mn--Minneapolis&source=EVENTS&categoryId=&distance=tenMiles", 0.7),
        ("https://www.meetup.com/find/?location=us--mn--St%20Paul&source=EVENTS&categoryId=&distance=tenMiles", 0.7),
        // Patch — local news & events
        ("https://patch.com/minnesota/minneapolis", 0.65),
        ("https://patch.com/minnesota/st-paul", 0.65),
        // Reddit — community voice (old.reddit.com is scrapable without auth)
        ("https://old.reddit.com/r/Minneapolis/new/", 0.4),
        ("https://old.reddit.com/r/TwinCities/new/", 0.4),
        ("https://old.reddit.com/r/SaintPaul/new/", 0.4),
        ("https://old.reddit.com/r/Minneapolis/search?q=ICE+immigration&restrict_sr=on&sort=new&t=week", 0.4),
        ("https://old.reddit.com/r/TwinCities/search?q=volunteer+OR+mutual+aid+OR+community&restrict_sr=on&sort=new&t=week", 0.4),
        // Bluesky — public, no auth wall
        ("https://bsky.app/search?q=minneapolis+community", 0.4),
        ("https://bsky.app/search?q=twin+cities+volunteer", 0.4),
        ("https://bsky.app/search?q=minneapolis+ICE+immigration", 0.4),
        // Volunteer platforms
        ("https://www.volunteermatch.org/search?l=Minneapolis%2C+MN&k=&v=true", 0.75),
        ("https://www.volunteermatch.org/search?l=St+Paul%2C+MN&k=&v=true", 0.75),
        ("https://www.idealist.org/en/volunteer-opportunities?areasOfFocus=COMMUNITY_DEVELOPMENT&q=&searchMode=true&location=Minneapolis%2C+MN&lat=44.9778&lng=-93.2650&radius=25", 0.7),
        ("https://www.justserve.org/projects?city=Minneapolis&state=MN", 0.7),
    ]
}

/// Instagram accounts to scrape via Apify.
/// These are Twin Cities orgs that post volunteer needs, events, and community updates.
/// Returns (username, trust_score).
pub fn instagram_accounts() -> Vec<(&'static str, f32)> {
    vec![
        // Volunteer / community orgs
        ("handsontwincities", 0.7),
        ("unitedwaytwincities", 0.7),
        ("gtcuw", 0.7), // Greater Twin Cities United Way
        // Food shelves / mutual aid
        ("secondharvestheartland", 0.7),
        ("loaborealforest", 0.6),
        ("pillsburyunited", 0.7),
        // Ecological / parks
        ("maborealforest", 0.6),
        ("friendsofthemsriver", 0.6),
        ("mplsparks", 0.7),
        // Civic / advocacy
        ("acabortionfund", 0.5),
        ("navigatormn", 0.6),
        ("mnimmigrantrights", 0.5),
        // Community centers
        ("eastsidefreedomlibrary", 0.6),
        ("midaborecenter", 0.6),
    ]
}

/// Facebook pages to scrape via Apify.
/// Returns (page_url, trust_score).
pub fn facebook_pages() -> Vec<(&'static str, f32)> {
    vec![
        ("https://www.facebook.com/handsontwincities", 0.7),
        ("https://www.facebook.com/MinneapolisParks", 0.7),
        ("https://www.facebook.com/SecondHarvestHeartland", 0.7),
        ("https://www.facebook.com/UnitedWayTwinCities", 0.7),
        ("https://www.facebook.com/PillsburyUnited", 0.7),
        ("https://www.facebook.com/OpenArmsMinnesota", 0.7),
        ("https://www.facebook.com/simpsonhousing", 0.6),
    ]
}
