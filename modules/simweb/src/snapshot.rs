//! RunLog for disk serialization / replay of simulated web interactions.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::types::{SimPage, SimPost, SimSearchResult};

/// A log of all interactions with a SimulatedWeb during a test run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunLog {
    pub started_at: DateTime<Utc>,
    pub entries: Vec<LogEntry>,
}

/// A single logged interaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LogEntry {
    Search {
        query: String,
        results: Vec<SimSearchResult>,
        timestamp: DateTime<Utc>,
    },
    Scrape {
        url: String,
        page: SimPage,
        timestamp: DateTime<Utc>,
    },
    Social {
        platform: String,
        identifier: String,
        posts: Vec<SimPost>,
        timestamp: DateTime<Utc>,
    },
    Hashtags {
        hashtags: Vec<String>,
        posts: Vec<SimPost>,
        timestamp: DateTime<Utc>,
    },
}

impl RunLog {
    pub fn new() -> Self {
        Self {
            started_at: Utc::now(),
            entries: Vec::new(),
        }
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        let log: Self = serde_json::from_str(&json)?;
        Ok(log)
    }
}

impl Default for RunLog {
    fn default() -> Self {
        Self::new()
    }
}
