//! ScoutGenome — treats scout's prompts as a mutable genome for evolution.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A genome representing a scout's prompt configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoutGenome {
    pub id: String,
    pub parent_id: Option<String>,
    pub generation: u32,
    pub created_at: DateTime<Utc>,
    /// Extractor system prompt template with `{city_name}` and `{today}` placeholders.
    pub extractor_prompt: String,
    /// Discovery system prompt template with `{city_name}` placeholder.
    pub discovery_prompt: String,
    /// Which prompt was mutated (if any).
    pub mutation_target: Option<String>,
    /// Why this mutation was made.
    pub mutation_reasoning: Option<String>,
    /// Fitness after evaluation (None until evaluated).
    pub fitness: Option<FitnessScore>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FitnessScore {
    pub total: f64,
    pub scenario_scores: Vec<ScenarioScore>,
    pub audit_pass_rate: f64,
    pub regressions: u32,
    pub evaluated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioScore {
    pub name: String,
    pub verdict_pass: bool,
    pub verdict_score: f32,
    pub audit_passed: usize,
    pub audit_total: usize,
}

impl ScoutGenome {
    /// Create the baseline genome from current prompt templates.
    ///
    /// Callers pass in the current extractor and discovery prompt templates
    /// (with `{city_name}` / `{today}` placeholders intact).
    pub fn baseline(extractor_prompt: String, discovery_prompt: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            parent_id: None,
            generation: 0,
            created_at: Utc::now(),
            extractor_prompt,
            discovery_prompt,
            mutation_target: None,
            mutation_reasoning: None,
            fitness: None,
        }
    }

    /// Create a child genome with a mutated extractor prompt.
    pub fn child_extractor(&self, new_prompt: String, reasoning: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            parent_id: Some(self.id.clone()),
            generation: self.generation + 1,
            created_at: Utc::now(),
            extractor_prompt: new_prompt,
            discovery_prompt: self.discovery_prompt.clone(),
            mutation_target: Some("extractor".to_string()),
            mutation_reasoning: Some(reasoning),
            fitness: None,
        }
    }

    /// Render the extractor prompt for a specific city, substituting placeholders.
    pub fn render_extractor_prompt(&self, city_name: &str) -> String {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        self.extractor_prompt
            .replace("{city_name}", city_name)
            .replace("{today}", &today)
    }

    /// Render the discovery prompt for a specific city.
    pub fn render_discovery_prompt(&self, city_name: &str) -> String {
        self.discovery_prompt.replace("{city_name}", city_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_creates_generation_zero() {
        let genome = ScoutGenome::baseline(
            "Extract signals for {city_name} on {today}".to_string(),
            "Discover sources for {city_name}".to_string(),
        );
        assert_eq!(genome.generation, 0);
        assert!(genome.parent_id.is_none());
        assert!(genome.fitness.is_none());
    }

    #[test]
    fn child_increments_generation() {
        let parent = ScoutGenome::baseline("prompt".to_string(), "disc".to_string());
        let child = parent.child_extractor("new prompt".to_string(), "fix X".to_string());
        assert_eq!(child.generation, 1);
        assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
        assert_eq!(child.mutation_target.as_deref(), Some("extractor"));
    }

    #[test]
    fn render_substitutes_placeholders() {
        let genome = ScoutGenome::baseline(
            "Extract for {city_name} on {today}".to_string(),
            "Discover for {city_name}".to_string(),
        );
        let rendered = genome.render_extractor_prompt("Minneapolis");
        assert!(rendered.contains("Minneapolis"));
        assert!(!rendered.contains("{city_name}"));
        assert!(!rendered.contains("{today}"));

        let disc = genome.render_discovery_prompt("Portland");
        assert!(disc.contains("Portland"));
        assert!(!disc.contains("{city_name}"));
    }
}
