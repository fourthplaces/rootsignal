//! World description — the single source of truth for simulation and judging.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// A complete simulated world. Drives content generation and judge evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct World {
    pub name: String,
    pub description: String,
    pub facts: Vec<Fact>,
    pub sites: Vec<Site>,
    pub social_profiles: Vec<SocialProfile>,
    pub topics: Vec<String>,
    pub geography: Geography,
}

/// Geographic context for the simulated world.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Geography {
    pub name: String,
    pub state_or_region: String,
    pub country: String,
    pub local_terms: Vec<String>,
    pub center_lat: f64,
    pub center_lng: f64,
}

/// A website in the simulated world.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Site {
    pub url: String,
    pub kind: String,
    pub content_description: String,
    pub published: Option<NaiveDate>,
    pub links_to: Vec<String>,
}

/// A social media profile in the simulated world.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialProfile {
    pub platform: String,
    pub identifier: String,
    pub persona: String,
    pub post_count: u32,
}

/// A ground-truth fact. Referenced by sites and used for judge evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    pub text: String,
    pub referenced_by: Vec<String>,
    pub category: String,
}
