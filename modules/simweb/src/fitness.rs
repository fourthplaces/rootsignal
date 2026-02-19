//! Fitness scoring for genome evaluation.

use chrono::Utc;

use crate::genome::{FitnessScore, ScenarioScore};

/// Score a genome's performance across scenarios.
///
/// Formula:
/// - `verdict_avg` = mean of all `verdict_score` values (0.0–1.0)
/// - `audit_avg` = mean of `audit_passed / audit_total` per scenario
/// - `raw = 0.7 * verdict_avg + 0.3 * audit_avg`
/// - `regressions` = count of scenarios baseline passed but mutant fails
/// - `total = max(0, raw - regressions * 0.05)`
pub fn score_genome(scores: &[ScenarioScore], baseline: Option<&[ScenarioScore]>) -> FitnessScore {
    if scores.is_empty() {
        return FitnessScore {
            total: 0.0,
            scenario_scores: vec![],
            audit_pass_rate: 0.0,
            regressions: 0,
            evaluated_at: Utc::now(),
        };
    }

    let verdict_avg: f64 =
        scores.iter().map(|s| s.verdict_score as f64).sum::<f64>() / scores.len() as f64;

    let audit_avg: f64 = scores
        .iter()
        .map(|s| {
            if s.audit_total == 0 {
                1.0
            } else {
                s.audit_passed as f64 / s.audit_total as f64
            }
        })
        .sum::<f64>()
        / scores.len() as f64;

    let regressions = count_regressions(scores, baseline);

    let raw = 0.7 * verdict_avg + 0.3 * audit_avg;
    let total = (raw - regressions as f64 * 0.05).max(0.0);

    FitnessScore {
        total,
        scenario_scores: scores.to_vec(),
        audit_pass_rate: audit_avg,
        regressions,
        evaluated_at: Utc::now(),
    }
}

/// Count scenarios where baseline passed but mutant fails.
fn count_regressions(scores: &[ScenarioScore], baseline: Option<&[ScenarioScore]>) -> u32 {
    let baseline = match baseline {
        Some(b) => b,
        None => return 0,
    };

    let mut count = 0u32;
    for score in scores {
        if let Some(base) = baseline.iter().find(|b| b.name == score.name) {
            if base.verdict_pass && !score.verdict_pass {
                count += 1;
            }
        }
    }
    count
}

/// Selection rule: mutant replaces champion only if total > champion AND zero regressions.
pub fn is_improvement(mutant: &FitnessScore, champion: &FitnessScore) -> bool {
    mutant.total > champion.total && mutant.regressions == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_score(
        name: &str,
        pass: bool,
        score: f32,
        audit_passed: usize,
        audit_total: usize,
    ) -> ScenarioScore {
        ScenarioScore {
            name: name.to_string(),
            verdict_pass: pass,
            verdict_score: score,
            audit_passed,
            audit_total,
        }
    }

    #[test]
    fn empty_scores_yield_zero() {
        let fitness = score_genome(&[], None);
        assert_eq!(fitness.total, 0.0);
    }

    #[test]
    fn perfect_scores_yield_one() {
        let scores = vec![
            make_score("a", true, 1.0, 5, 5),
            make_score("b", true, 1.0, 3, 3),
        ];
        let fitness = score_genome(&scores, None);
        assert!((fitness.total - 1.0).abs() < 0.001);
    }

    #[test]
    fn regressions_penalize() {
        let baseline = vec![
            make_score("a", true, 0.8, 4, 5),
            make_score("b", true, 0.7, 3, 5),
        ];
        let mutant = vec![
            make_score("a", false, 0.3, 2, 5), // regression
            make_score("b", true, 0.9, 5, 5),
        ];
        let fitness = score_genome(&mutant, Some(&baseline));
        assert_eq!(fitness.regressions, 1);
        // raw = 0.7 * 0.6 + 0.3 * 0.7 = 0.42 + 0.21 = 0.63, penalty = 0.05 → 0.58
        assert!(fitness.total < 0.63);
    }

    #[test]
    fn improvement_requires_zero_regressions() {
        let champion = FitnessScore {
            total: 0.5,
            scenario_scores: vec![],
            audit_pass_rate: 0.5,
            regressions: 0,
            evaluated_at: Utc::now(),
        };
        let better_but_regressed = FitnessScore {
            total: 0.6,
            scenario_scores: vec![],
            audit_pass_rate: 0.7,
            regressions: 1,
            evaluated_at: Utc::now(),
        };
        assert!(!is_improvement(&better_but_regressed, &champion));

        let clean_improvement = FitnessScore {
            total: 0.6,
            scenario_scores: vec![],
            audit_pass_rate: 0.7,
            regressions: 0,
            evaluated_at: Utc::now(),
        };
        assert!(is_improvement(&clean_improvement, &champion));
    }
}
