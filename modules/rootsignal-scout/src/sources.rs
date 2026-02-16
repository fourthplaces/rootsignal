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
        // Social media
        d if d.contains("facebook.com") || d.contains("instagram.com") || d.contains("twitter.com") || d.contains("x.com") => 0.3,
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
    ]
}
