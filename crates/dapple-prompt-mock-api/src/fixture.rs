use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FixtureError {
    #[error("Failed to read fixture file: {0}")]
    ReadError(#[from] std::io::Error),

    #[error("Failed to parse fixture JSON: {0}")]
    ParseError(#[from] serde_json::Error),

    #[error("Fixture validation failed: {0}")]
    ValidationError(String),
}

/// Metadata about the fixture
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureMeta {
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub created_at: Option<String>,
}

/// All entity types in the fixture
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FixtureEntities {
    #[serde(default)]
    pub categories: Vec<Value>,
    #[serde(default)]
    pub hats: Vec<Value>,
    #[serde(default)]
    pub tasks: Vec<Value>,
    #[serde(default)]
    pub steps: Vec<Value>,
    #[serde(default)]
    pub stages: Vec<Value>,
    #[serde(default)]
    pub playbooks: Vec<Value>,
    #[serde(default)]
    pub journeys: Vec<Value>,
    #[serde(default)]
    pub dispatches: Vec<Value>,
    #[serde(default)]
    pub value_slots: Vec<Value>,
    #[serde(default)]
    pub value_slot_paths: Vec<Value>,
    #[serde(default)]
    pub assets: Vec<Value>,
    #[serde(default)]
    pub asset_paths: Vec<Value>,
    #[serde(default)]
    pub fluent_messages: Vec<Value>,
    #[serde(default)]
    pub fluent_message_paths: Vec<Value>,
    #[serde(default)]
    pub briefs: Vec<Value>,
    #[serde(default)]
    pub brief_paths: Vec<Value>,
    #[serde(default)]
    pub brief_units: Vec<Value>,
    #[serde(default)]
    pub brief_groups: Vec<Value>,
    #[serde(default)]
    pub brief_group_paths: Vec<Value>,
    #[serde(default)]
    pub brief_packs: Vec<Value>,
    #[serde(default)]
    pub brief_pack_paths: Vec<Value>,
    #[serde(default)]
    pub prompts: Vec<Value>,
    #[serde(default)]
    pub prompt_paths: Vec<Value>,
    #[serde(default)]
    pub task_prompts: Vec<Value>,
    #[serde(default)]
    pub dispatch_intent_bindings: Vec<Value>,
    #[serde(default)]
    pub dispatch_task_attempts: Vec<Value>,
}

/// Root fixture structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fixture {
    pub meta: FixtureMeta,
    pub entities: FixtureEntities,
}

impl Fixture {
    /// Load a fixture from a file path
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self, FixtureError> {
        let content = fs::read_to_string(path)?;
        let fixture: Fixture = serde_json::from_str(&content)?;
        Ok(fixture)
    }

    /// Load the default fixture embedded in the binary
    pub fn load_default() -> Self {
        let content = include_str!("../assets/fixtures/default_scenario.json");
        serde_json::from_str(content).expect("Default fixture should be valid JSON")
    }

    /// Create an empty fixture (for testing)
    pub fn empty() -> Self {
        Self {
            meta: FixtureMeta {
                version: "1.0.0".to_string(),
                description: "Empty fixture".to_string(),
                created_at: None,
            },
            entities: FixtureEntities::default(),
        }
    }

    /// Validate the fixture for referential integrity
    pub fn validate(&self) -> Result<(), FixtureError> {
        let mut errors = vec![];

        // Collect all ids by type for reference checking
        let task_ids: Vec<_> = self
            .entities
            .tasks
            .iter()
            .filter_map(|t| t.get("id").and_then(|v| v.as_str()))
            .collect();

        let step_ids: Vec<_> = self
            .entities
            .steps
            .iter()
            .filter_map(|s| s.get("id").and_then(|v| v.as_str()))
            .collect();

        let playbook_ids: Vec<_> = self
            .entities
            .playbooks
            .iter()
            .filter_map(|p| p.get("id").and_then(|v| v.as_str()))
            .collect();

        // Validate journeys reference existing playbooks
        for journey in &self.entities.journeys {
            if let Some(playbook_id) = journey.get("playbook_id").and_then(|v| v.as_str())
                && !playbook_ids.contains(&playbook_id)
            {
                errors.push(format!(
                    "Journey references non-existent playbook: {}",
                    playbook_id
                ));
            }
        }

        // Validate steps reference existing tasks
        for step in &self.entities.steps {
            if let Some(tasks) = step.get("tasks").and_then(|v| v.as_array()) {
                for task_ref in tasks {
                    if let Some(task) = task_ref.get("task")
                        && let Some(task_id) = task.get("id").and_then(|v| v.as_str())
                        && !task_ids.contains(&task_id)
                    {
                        errors.push(format!("Step references non-existent task: {}", task_id));
                    }
                }
            }
        }

        // Validate stages reference existing steps
        for stage in &self.entities.stages {
            if let Some(steps) = stage.get("steps").and_then(|v| v.as_array()) {
                for step_ref in steps {
                    if let Some(step_id) = step_ref.get("id").and_then(|v| v.as_str())
                        && !step_ids.contains(&step_id)
                    {
                        errors.push(format!("Stage references non-existent step: {}", step_id));
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(FixtureError::ValidationError(errors.join("; ")))
        }
    }
}
