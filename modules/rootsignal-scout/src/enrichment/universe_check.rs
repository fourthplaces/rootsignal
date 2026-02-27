//! In-universe check — LLM-based filter that decides whether a URL belongs
//! in our universe of local civic content before we spend resources scraping it.

use ai_client::claude::Claude;
use schemars::JsonSchema;
use serde::Deserialize;
use tracing::{info, warn};

use rootsignal_common::ScoutScope;

use crate::traits::ContentFetcher;

#[derive(Deserialize, JsonSchema)]
struct UniverseVerdict {
    /// true if the URL could contain local civic content for this region
    in_universe: bool,
    /// true if you cannot determine from the URL alone and need to see page content
    needs_content: bool,
}

const UNIVERSE_CHECK_SYSTEM: &str = "\
You evaluate whether a URL belongs in our universe of local civic content.\n\n\
IN-UNIVERSE sources contain:\n\
- Local journalism reporting on community impacts\n\
- Community organizations, mutual aid, neighborhood groups\n\
- Local government services, meetings, public records\n\
- Event listings from local organizers\n\
- Social media accounts of local people/orgs\n\
- Local businesses serving the community\n\n\
NOT in our universe:\n\
- E-commerce (Amazon, eBay, Etsy storefronts)\n\
- SaaS/platform marketing (Webflow, Squarespace, Mailchimp)\n\
- National/international media opinion/punditry\n\
- Corporate sites with no local civic connection\n\
- Documentation, tutorials, developer tools\n\
- Entertainment, sports scores, celebrity news";

pub async fn check_in_universe(
    url: &str,
    region: &ScoutScope,
    anthropic_api_key: &str,
    fetcher: &dyn ContentFetcher,
) -> bool {
    let claude = Claude::new(anthropic_api_key, "claude-haiku-4-5-20251001");

    // Phase 1: URL-only check
    let prompt = format!(
        "Region: {region}\nURL: {url}\n\n\
         If you can determine from the URL alone, set needs_content to false.\n\
         If the URL is ambiguous, set needs_content to true and in_universe to false.",
        region = region.name,
        url = url,
    );

    let verdict: UniverseVerdict = match claude.extract(UNIVERSE_CHECK_SYSTEM, &prompt).await {
        Ok(v) => v,
        Err(e) => {
            warn!(url, error = %e, "Universe check failed, defaulting to pass");
            return true; // fail-open
        }
    };

    if !verdict.needs_content {
        info!(
            url,
            in_universe = verdict.in_universe,
            "Universe check (URL-only)"
        );
        return verdict.in_universe;
    }

    // Phase 2: fetch content, re-evaluate
    let content = match fetcher.page(url).await {
        Ok(p) if !p.markdown.is_empty() => p.markdown,
        _ => {
            info!(
                url,
                "Universe check: content fetch failed, defaulting to pass"
            );
            return true;
        }
    };

    let content_prompt = format!(
        "Region: {region}\nURL: {url}\n\nPage content (first 4000 chars):\n{content}",
        region = region.name,
        url = url,
        content = &content[..content.len().min(4000)],
    );

    match claude
        .extract::<UniverseVerdict>(UNIVERSE_CHECK_SYSTEM, &content_prompt)
        .await
    {
        Ok(v) => {
            info!(
                url,
                in_universe = v.in_universe,
                "Universe check (with content)"
            );
            v.in_universe
        }
        Err(e) => {
            warn!(url, error = %e, "Universe check phase 2 failed, defaulting to pass");
            true
        }
    }
}
