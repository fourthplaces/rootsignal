//! simweb — Simulates a coherent web from a world description for testing.
//!
//! Domain-agnostic: no dependency on rootsignal types.
//! Uses Claude Haiku for content generation, Sonnet for judge evaluation.

pub mod improve;
pub mod judge;
pub mod prompt;
pub mod sim;
pub mod snapshot;
pub mod types;
pub mod world;

pub use improve::{BlindSpot, BlindSpotSeverity, ImprovementReport, Improver, PromptFix, TestFailure};
pub use judge::{generate_random_world, Issue, Judge, JudgeCriteria, Severity, Verdict};
pub use sim::SimulatedWeb;
pub use types::{SimPage, SimPost, SimSearchResult};
pub use world::{Fact, Geography, Site, SocialProfile, World};
