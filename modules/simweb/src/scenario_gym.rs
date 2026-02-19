//! Scenario gym — manages hand-written and generated test scenarios.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::judge::JudgeCriteria;
use crate::world::World;

/// A single scenario in the gym.
pub struct ScenarioEntry {
    pub name: String,
    pub world: World,
    pub criteria: JudgeCriteria,
    pub source: ScenarioSource,
}

/// Where a scenario came from.
pub enum ScenarioSource {
    HandWritten,
    Generated {
        blind_spot: String,
        promoted_at: DateTime<Utc>,
    },
}

/// Persisted format for generated scenarios.
#[derive(Serialize, Deserialize)]
struct PersistedScenario {
    name: String,
    world: World,
    criteria: JudgeCriteria,
    blind_spot: String,
    promoted_at: DateTime<Utc>,
}

/// Collection of scenarios that grows over time as adversarial scenarios are promoted.
pub struct ScenarioGym {
    entries: Vec<ScenarioEntry>,
    generated_dir: Option<PathBuf>,
}

impl ScenarioGym {
    /// Load the gym from hand-written scenarios + generated JSON files on disk.
    pub fn load(hand_written: Vec<ScenarioEntry>, generated_dir: &Path) -> Self {
        let mut entries = hand_written;

        if generated_dir.exists() {
            if let Ok(dir) = std::fs::read_dir(generated_dir) {
                for entry in dir.flatten() {
                    let path = entry.path();
                    if path.extension().map_or(false, |e| e == "json") {
                        match load_generated_scenario(&path) {
                            Ok(scenario) => entries.push(scenario),
                            Err(e) => {
                                tracing::warn!(
                                    path = %path.display(),
                                    error = %e,
                                    "Failed to load generated scenario"
                                );
                            }
                        }
                    }
                }
            }
        }

        Self {
            entries,
            generated_dir: Some(generated_dir.to_path_buf()),
        }
    }

    /// Create a gym from entries only (no disk persistence).
    pub fn from_entries(entries: Vec<ScenarioEntry>) -> Self {
        Self {
            entries,
            generated_dir: None,
        }
    }

    /// Promote a generated scenario into the gym and persist to disk.
    pub fn promote(
        &mut self,
        name: String,
        world: World,
        criteria: JudgeCriteria,
        blind_spot: String,
    ) -> anyhow::Result<()> {
        let promoted_at = Utc::now();

        // Persist to disk if we have a directory
        if let Some(dir) = &self.generated_dir {
            std::fs::create_dir_all(dir)?;
            let filename = format!(
                "{}_{}.json",
                name.to_lowercase().replace(' ', "_"),
                promoted_at.format("%Y%m%d_%H%M%S")
            );
            let path = dir.join(filename);
            let persisted = PersistedScenario {
                name: name.clone(),
                world: world.clone(),
                criteria: criteria.clone(),
                blind_spot: blind_spot.clone(),
                promoted_at,
            };
            let json = serde_json::to_string_pretty(&persisted)?;
            std::fs::write(&path, json)?;
            tracing::info!(path = %path.display(), "Promoted scenario to gym");
        }

        self.entries.push(ScenarioEntry {
            name,
            world,
            criteria,
            source: ScenarioSource::Generated {
                blind_spot,
                promoted_at,
            },
        });

        Ok(())
    }

    /// All scenarios in the gym.
    pub fn scenarios(&self) -> &[ScenarioEntry] {
        &self.entries
    }

    /// Number of hand-written scenarios.
    pub fn hand_written_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e.source, ScenarioSource::HandWritten))
            .count()
    }

    /// Number of generated scenarios.
    pub fn generated_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e.source, ScenarioSource::Generated { .. }))
            .count()
    }
}

fn load_generated_scenario(path: &Path) -> anyhow::Result<ScenarioEntry> {
    let data = std::fs::read_to_string(path)?;
    let persisted: PersistedScenario = serde_json::from_str(&data)?;
    Ok(ScenarioEntry {
        name: persisted.name,
        world: persisted.world,
        criteria: persisted.criteria,
        source: ScenarioSource::Generated {
            blind_spot: persisted.blind_spot,
            promoted_at: persisted.promoted_at,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::judge::JudgeCriteria;
    use crate::world::{Geography, World};

    fn test_world() -> World {
        World {
            name: "test".to_string(),
            description: "test scenario".to_string(),
            facts: vec![],
            sites: vec![],
            social_profiles: vec![],
            topics: vec![],
            geography: Geography {
                city: "TestCity".to_string(),
                state_or_region: "TS".to_string(),
                country: "US".to_string(),
                local_terms: vec![],
                center_lat: 0.0,
                center_lng: 0.0,
            },
        }
    }

    fn test_criteria() -> JudgeCriteria {
        JudgeCriteria {
            checks: vec!["test".to_string()],
            pass_threshold: 0.5,
            critical_categories: vec![],
        }
    }

    #[test]
    fn from_entries_works() {
        let entries = vec![ScenarioEntry {
            name: "test".to_string(),
            world: test_world(),
            criteria: test_criteria(),
            source: ScenarioSource::HandWritten,
        }];
        let gym = ScenarioGym::from_entries(entries);
        assert_eq!(gym.scenarios().len(), 1);
        assert_eq!(gym.hand_written_count(), 1);
        assert_eq!(gym.generated_count(), 0);
    }

    #[test]
    fn promote_adds_to_entries() {
        let mut gym = ScenarioGym::from_entries(vec![]);
        gym.promote(
            "adversarial".to_string(),
            test_world(),
            test_criteria(),
            "missing tension extraction".to_string(),
        )
        .unwrap();
        assert_eq!(gym.scenarios().len(), 1);
        assert_eq!(gym.generated_count(), 1);
    }
}
