use std::collections::HashSet;

use serde_json::Value;

use crate::{Message, ParseError, Trajectory};

/// Validates supported versions, legacy session identity, and sequential step IDs.
pub(crate) fn validate_core(trajectory: &Trajectory) -> Result<(), ParseError> {
    match trajectory.schema_version.as_str() {
        "ATIF-v1.0" | "ATIF-v1.1" | "ATIF-v1.2" | "ATIF-v1.3" | "ATIF-v1.4" | "ATIF-v1.5"
        | "ATIF-v1.6" => {
            if trajectory.session_id.is_none() {
                return Err(ParseError::new(
                    "session_id",
                    "required in ATIF v1.6 and earlier",
                ));
            }
        }
        "ATIF-v1.7" | "ATIF-v1.8" => {}
        _ => {
            return Err(ParseError::new(
                "schema_version",
                "expected ATIF-v1.0 through ATIF-v1.8",
            ))
        }
    }
    if trajectory.steps.is_empty() {
        return Err(ParseError::new("steps", "at least one step is required"));
    }
    for (index, step) in trajectory.steps.iter().enumerate() {
        let expected = index as u64 + 1;
        if step.step_id != expected {
            return Err(ParseError::new(
                format!("steps[{index}].step_id"),
                format!("expected {expected}, got {}", step.step_id),
            ));
        }
    }
    Ok(())
}

/// Deserializes an embedded document and validates its core fields and relationships.
pub(crate) fn validate_embedded(value: &Value, path: &str) -> Result<(), ParseError> {
    let trajectory: Trajectory = serde_path_to_error::deserialize(value).map_err(|error| {
        let mut error = ParseError::new(error.path().to_string(), error.inner().to_string());
        error.path = if error.path == "$" {
            path.into()
        } else {
            format!("{path}.{}", error.path)
        };
        error
    })?;
    validate_details(&trajectory, path)
}

/// Checks document relationships after schema normalization has checked field types.
pub(crate) fn validate_details(trajectory: &Trajectory, path: &str) -> Result<(), ParseError> {
    let prefix = if path.is_empty() {
        String::new()
    } else {
        format!("{path}.")
    };
    validate_core(trajectory).map_err(|mut error| {
        error.path = format!("{prefix}{}", error.path);
        error
    })?;
    // The exact version spelling was checked by validate_core.
    let minor = trajectory.schema_version.as_bytes()[8] - b'0';
    let embedded = trajectory
        .additional_fields
        .get("subagent_trajectories")
        .unwrap_or(&Value::Null);
    if !embedded.is_null() && minor < 7 {
        return Err(ParseError::new(
            format!("{prefix}subagent_trajectories"),
            "embedded subagents require ATIF v1.7 or later",
        ));
    }
    let mut ids = HashSet::new();
    for (index, child) in embedded.as_array().into_iter().flatten().enumerate() {
        let id_path = format!("{prefix}subagent_trajectories[{index}].trajectory_id");
        let id = child["trajectory_id"]
            .as_str()
            .ok_or_else(|| ParseError::new(&id_path, "required for embedded subagents"))?;
        if !ids.insert(id) {
            return Err(ParseError::new(id_path, "duplicate embedded trajectory ID"));
        }
    }
    for (index, step) in trajectory.steps.iter().enumerate() {
        let step_path = format!("{prefix}steps[{index}]");
        if let Message::Parts(parts) = &step.message {
            validate_content(parts, minor, &format!("{step_path}.message"))?;
        }
        let results = step
            .additional_fields
            .get("observation")
            .and_then(|value| value.get("results"))
            .and_then(Value::as_array);
        let calls = step
            .additional_fields
            .get("tool_calls")
            .and_then(Value::as_array);
        for (index, result) in results.into_iter().flatten().enumerate() {
            let result_path = format!("{step_path}.observation.results[{index}]");
            if let Some(parts) = result["content"].as_array() {
                validate_content(parts, minor, &format!("{result_path}.content"))?;
            }
            if let Some(id) = result["source_call_id"].as_str() {
                if !calls
                    .into_iter()
                    .flatten()
                    .any(|call| call["tool_call_id"].as_str() == Some(id))
                {
                    return Err(ParseError::new(
                        format!("{result_path}.source_call_id"),
                        "must reference a tool call in the same step",
                    ));
                }
            }
            for (index, reference) in result["subagent_trajectory_ref"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                let ref_path = format!("{result_path}.subagent_trajectory_ref[{index}]");
                let id = reference["trajectory_id"].as_str();
                let file = reference["trajectory_path"].as_str();
                if minor < 7 {
                    if reference["session_id"].is_null() {
                        return Err(ParseError::new(
                            format!("{ref_path}.session_id"),
                            "required in ATIF v1.6 and earlier",
                        ));
                    }
                } else if id.is_none() && file.is_none() {
                    return Err(ParseError::new(
                        ref_path,
                        "set trajectory_id or trajectory_path; session_id is not a resolution key",
                    ));
                } else if file.is_none() && !id.is_some_and(|id| ids.contains(id)) {
                    return Err(ParseError::new(
                        format!("{ref_path}.trajectory_id"),
                        "must match an embedded subagent in this document",
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Checks version and content/source rules; the schema already checked field types.
fn validate_content(parts: &[Value], minor: u8, path: &str) -> Result<(), ParseError> {
    if minor < 6 {
        return Err(ParseError::new(
            path,
            "content arrays require ATIF v1.6 or later",
        ));
    }
    for (index, part) in parts.iter().enumerate() {
        let kind = part["type"].as_str().unwrap_or_default();
        let source = &part["source"];
        let mime = source["media_type"].as_str().unwrap_or_default();
        let valid_mime = match kind {
            "image" => matches!(
                mime,
                "image/jpeg" | "image/png" | "image/gif" | "image/webp"
            ),
            // Accept Harbor's aliases, while preserving the original MIME spelling.
            "audio" => matches!(
                mime.trim().to_ascii_lowercase().as_str(),
                "audio/wav"
                    | "audio/mpeg"
                    | "audio/mp4"
                    | "audio/aac"
                    | "audio/ogg"
                    | "audio/flac"
                    | "audio/webm"
                    | "audio/aiff"
                    | "audio/mp3"
                    | "audio/mpga"
                    | "audio/x-mpeg"
                    | "audio/x-wav"
                    | "audio/wave"
                    | "audio/vnd.wave"
                    | "audio/x-m4a"
                    | "audio/m4a"
                    | "audio/x-aac"
                    | "audio/x-flac"
                    | "audio/x-aiff"
            ),
            _ => false,
        };
        let issue = match kind {
            "text" if part["text"].is_null() => Some(("text", "text content requires a string")),
            "text" if !source.is_null() => {
                Some(("source", "text content cannot contain a media source"))
            }
            "text" => None,
            "audio" if minor < 8 => Some(("type", "audio requires ATIF v1.8 or later")),
            "image" | "audio" if !part["text"].is_null() => {
                Some(("text", "media content cannot contain text"))
            }
            "image" | "audio" if source.is_null() => {
                Some(("source", "media content requires a source object"))
            }
            "image" | "audio" if !valid_mime => Some((
                "source.media_type",
                "MIME type does not match the content type",
            )),
            "image" if !source["duration_sec"].is_null() => {
                Some(("source.duration_sec", "only audio may have a duration"))
            }
            "audio"
                if source["duration_sec"]
                    .as_f64()
                    .is_some_and(|duration| duration < 0.0) =>
            {
                Some(("source.duration_sec", "audio duration must be nonnegative"))
            }
            "image" | "audio" => None,
            _ => Some(("type", "expected text, image, or audio")),
        };
        if let Some((field, message)) = issue {
            return Err(ParseError::new(format!("{path}[{index}].{field}"), message));
        }
    }
    Ok(())
}
