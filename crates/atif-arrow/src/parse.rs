use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{error::ParseError, validation::validate_core};

/// A parsed trajectory together with the original JSON document.
#[derive(Debug)]
pub struct ParsedTrajectory {
    pub trajectory: Trajectory,
    raw_json: String,
}

impl ParsedTrajectory {
    /// Returns the original input, including formatting and unmodeled fields.
    pub fn raw_json(&self) -> &str {
        &self.raw_json
    }
}

/// Core ATIF envelope; remaining root fields are preserved as JSON values.
#[derive(Debug, Deserialize, Serialize)]
pub struct Trajectory {
    pub schema_version: String,
    pub session_id: Option<String>,
    pub trajectory_id: Option<String>,
    pub agent: Agent,
    pub steps: Vec<Step>,
    #[serde(flatten)]
    pub additional_fields: Map<String, Value>,
}

/// Agent identity and configuration, with unmodeled fields retained.
#[derive(Debug, Deserialize, Serialize)]
pub struct Agent {
    pub name: String,
    pub version: String,
    pub model_name: Option<String>,
    #[serde(flatten)]
    pub additional_fields: Map<String, Value>,
}

/// An interaction's core fields; tool calls, metrics, and other payloads remain JSON.
#[derive(Debug, Deserialize, Serialize)]
pub struct Step {
    pub step_id: u64,
    pub source: StepSource,
    pub message: Message,
    #[serde(flatten)]
    pub additional_fields: Map<String, Value>,
}

/// The three interaction sources defined by ATIF.
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StepSource {
    System,
    User,
    Agent,
}

/// Text or ordered multimodal parts; conversion validates their content.
#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Message {
    Text(String),
    Parts(Vec<Value>),
}

/// Parses exactly one JSON document and validates its core ATIF fields.
pub fn parse_trajectory(input: &str) -> Result<ParsedTrajectory, ParseError> {
    let mut deserializer = serde_json::Deserializer::from_str(input);
    let trajectory: Trajectory = serde_path_to_error::deserialize(&mut deserializer)
        .map_err(|error| ParseError::new(error.path().to_string(), error.inner().to_string()))?;
    deserializer
        .end()
        .map_err(|error| ParseError::new("$", error.to_string()))?;
    validate_core(&trajectory)?;
    Ok(ParsedTrajectory {
        trajectory,
        raw_json: input.to_owned(),
    })
}
