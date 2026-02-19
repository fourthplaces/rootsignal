//! Prompt templates for LLM-driven content generation and judgment.

use crate::world::World;

/// Build the system prompt for search result generation.
pub fn search_system(world: &World) -> String {
    let sites_list = world
        .sites
        .iter()
        .map(|s| {
            let date_str = s
                .published
                .map(|d| format!(" (published: {d})"))
                .unwrap_or_default();
            format!("- {} [{}]{}: {}", s.url, s.kind, date_str, s.content_description)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let facts_list = world
        .facts
        .iter()
        .map(|f| format!("- [{}] {}", f.category, f.text))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"You generate realistic web search results for a simulated world.

WORLD: {name}
{description}

GEOGRAPHY: {city}, {state}, {country}
Local terms: {local_terms}

AVAILABLE SITES (you may ONLY return URLs from this list):
{sites_list}

GROUND-TRUTH FACTS (use these exact strings verbatim when relevant):
{facts_list}

RULES:
1. Only return URLs from the AVAILABLE SITES list above. Never invent URLs.
2. Titles and snippets must be consistent with each site's content_description.
3. When a fact is relevant to a search query, include it verbatim in the snippet.
4. If the query doesn't match any available site, return an empty results array.
5. Respect publication dates — don't reference content from sites published after the query implies.

Return JSON: {{"results": [{{"url": "...", "title": "...", "snippet": "..."}}]}}"#,
        name = world.name,
        description = world.description,
        city = world.geography.city,
        state = world.geography.state_or_region,
        country = world.geography.country,
        local_terms = world.geography.local_terms.join(", "),
        sites_list = sites_list,
        facts_list = facts_list,
    )
}

/// Build the user prompt for a search query.
pub fn search_user(query: &str, max_results: usize) -> String {
    format!(
        "Search query: \"{query}\"\nReturn up to {max_results} results as JSON."
    )
}

/// Build the system prompt for page content generation.
pub fn scrape_system(world: &World) -> String {
    let facts_list = world
        .facts
        .iter()
        .map(|f| format!("- [{}] {}", f.category, f.text))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"You generate realistic web page content for a simulated world.

WORLD: {name}
{description}

GEOGRAPHY: {city}, {state}, {country}

GROUND-TRUTH FACTS (use these exact strings verbatim when relevant):
{facts_list}

RULES:
1. Generate content that reads like a real web page — natural prose, not bullet points.
2. Include relevant facts verbatim where they fit naturally.
3. Content must be consistent with the site description and any search snippet provided.
4. Include dates, names, and specific details from the world description.
5. Write 200-500 words of main content (like what Readability extraction would produce)."#,
        name = world.name,
        description = world.description,
        city = world.geography.city,
        state = world.geography.state_or_region,
        country = world.geography.country,
        facts_list = facts_list,
    )
}

/// Build the user prompt for scraping a specific URL.
pub fn scrape_user(url: &str, site_description: &str, prior_snippet: Option<&str>) -> String {
    let snippet_context = match prior_snippet {
        Some(snippet) => format!(
            "\n\nThis page was found via search with snippet: \"{snippet}\". Content must be consistent with this snippet."
        ),
        None => String::new(),
    };

    format!(
        "Generate the main text content of this web page.\n\nURL: {url}\nSite description: {site_description}{snippet_context}"
    )
}

/// Build the system prompt for social post generation.
pub fn social_system(world: &World) -> String {
    let facts_list = world
        .facts
        .iter()
        .map(|f| format!("- [{}] {}", f.category, f.text))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"You generate realistic social media posts for a simulated world.

WORLD: {name}
{description}

GEOGRAPHY: {city}, {state}, {country}

GROUND-TRUTH FACTS (use these exact strings verbatim when relevant):
{facts_list}

RULES:
1. Posts should read like real social media content — casual tone, hashtags, emojis where appropriate.
2. Match the platform conventions (Instagram = visual/hashtag-heavy, Reddit = discussion-oriented, Facebook = community updates).
3. Include relevant facts verbatim where they fit naturally in posts.
4. Each post should be 1-3 sentences.

Return JSON: {{"posts": [{{"content": "...", "author": "...", "url": "..."}}]}}"#,
        name = world.name,
        description = world.description,
        city = world.geography.city,
        state = world.geography.state_or_region,
        country = world.geography.country,
        facts_list = facts_list,
    )
}

/// Build the user prompt for generating posts from a specific profile.
pub fn social_profile_user(platform: &str, identifier: &str, persona: &str, limit: u32) -> String {
    format!(
        "Platform: {platform}\nAccount: {identifier}\nPersona: {persona}\n\nGenerate {limit} posts from this account as JSON."
    )
}

/// Build the user prompt for generating posts matching hashtags.
pub fn social_hashtags_user(hashtags: &[String], limit: u32) -> String {
    format!(
        "Hashtags: {}\n\nGenerate {limit} posts from different accounts using these hashtags as JSON.",
        hashtags.join(", ")
    )
}

/// Build the system prompt for the judge.
pub fn judge_system() -> &'static str {
    r#"You are an impartial judge evaluating how well scout (a signal agent) processed web content.

Scout's core job is the TENSION-RESPONSE CYCLE: find real problems (tensions) in community or
ecological life, then find the gives/asks/events that address them.

You will receive:
1. A WORLD DESCRIPTION with ground-truth facts
2. EVALUATION CRITERIA with specific checks
3. SCOUT'S OUTPUT (signals extracted and stored in a graph database)

Evaluate whether scout's output is accurate, complete, and appropriately confident given the source material.

EVALUATION PRIORITIES (weight these when assessing completeness):
1. Tension-Response pairs: Did scout find tensions AND the responses addressing them? Missing a pair is Critical.
2. Standalone responses: Did scout capture gives/asks/events even without an explicit tension? Missing these is Warning.
3. Context signals: Did scout capture relevant notices and advisories? Missing these is Info.
4. Routine community activity (recurring services, social gatherings) is lowest priority — missing these is not an issue.

SEVERITY DEFINITIONS:
- Critical: Missed a tension-response pair present in ground truth. Or asserted something contradicted by ground truth. Or hallucinated signals.
- Warning: Missed a standalone response (give/ask/event with implicit tension). Or captured the gist but missed nuance, assigned inappropriate confidence, or failed to link a response to its tension.
- Info: Stylistic or minor. Signal titles could be clearer, categories could be more specific, missed routine community activity.

SCORING:
- Start at 1.0 (perfect)
- Each Critical issue: -0.25
- Each Warning issue: -0.10
- Info issues: no score impact
- Minimum score: 0.0

Return JSON:
{
  "pass": true/false,
  "score": 0.0-1.0,
  "reasoning": "2-3 sentence summary of overall assessment",
  "issues": [
    {
      "severity": "Critical|Warning|Info",
      "category": "string",
      "description": "string"
    }
  ]
}"#
}

/// Build the user prompt for judge evaluation.
pub fn judge_user(world: &World, criteria_checks: &[String], agent_output: &str) -> String {
    let facts_list = world
        .facts
        .iter()
        .map(|f| format!("- [{}] {}", f.category, f.text))
        .collect::<Vec<_>>()
        .join("\n");

    let checks_list = criteria_checks
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{}. {c}", i + 1))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"## WORLD DESCRIPTION

Name: {name}
{description}

Geography: {city}, {state}, {country}

### Ground-Truth Facts:
{facts_list}

### Sites:
{sites}

### Social Profiles:
{profiles}

## EVALUATION CRITERIA

{checks_list}

## AGENT OUTPUT

{agent_output}"#,
        name = world.name,
        description = world.description,
        city = world.geography.city,
        state = world.geography.state_or_region,
        country = world.geography.country,
        facts_list = facts_list,
        sites = world
            .sites
            .iter()
            .map(|s| format!(
                "- {} [{}]: {}{}",
                s.url,
                s.kind,
                s.content_description,
                s.published
                    .map(|d| format!(" (published: {d})"))
                    .unwrap_or_default()
            ))
            .collect::<Vec<_>>()
            .join("\n"),
        profiles = world
            .social_profiles
            .iter()
            .map(|p| format!("- {} @{}: {}", p.platform, p.identifier, p.persona))
            .collect::<Vec<_>>()
            .join("\n"),
        checks_list = checks_list,
    )
}

/// Build the system prompt for random world generation (Tier 3).
pub fn world_gen_system() -> &'static str {
    r#"You generate realistic simulated worlds for testing a signal detection agent.

A "world" describes a city neighborhood with:
- Active community life (community organizations, mutual aid, local government, etc.)
- Multiple web sources (news sites, org pages, government pages, blogs)
- Social media presence (Instagram, Reddit, Facebook accounts)
- Ground-truth facts that should appear in the generated content
- A specific geographic location in the US

The world should have interesting properties that test the agent's ability to:
- Distinguish current from stale information
- Detect community activity from informal sources
- Handle conflicting information
- Recognize different signal types (events, resources, asks, tensions)

Return a complete World as JSON matching this schema:
{
  "name": "string",
  "description": "string (2-3 paragraphs describing the scenario)",
  "facts": [{"text": "exact string", "referenced_by": ["url1"], "category": "string"}],
  "sites": [{"url": "https://...", "kind": "string", "content_description": "string", "published": "YYYY-MM-DD or null", "links_to": ["url"]}],
  "social_profiles": [{"platform": "Instagram|Reddit|Facebook", "identifier": "string", "persona": "string", "post_count": number}],
  "topics": ["string"],
  "geography": {"city": "string", "state_or_region": "string", "country": "US", "local_terms": ["string"], "center_lat": number, "center_lng": number}
}

Generate 6-10 sites, 3-5 social profiles, 5-10 facts, and 3-5 topics.
URLs should look realistic (e.g., https://www.cityname-food-shelf.org/about)."#
}

/// Build the user prompt for random world generation.
pub fn world_gen_user() -> &'static str {
    "Generate a random simulated world for testing. Make it interesting — include at least one challenging aspect (stale info, conflicting sources, informal community spaces, etc.). Return JSON only."
}
