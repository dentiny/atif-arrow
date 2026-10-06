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

/// Recursively validates multimodal content, embedded identities, and subagent references.
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
    // validate_core has already checked the exact ATIF-v1.0 through ATIF-v1.8 spelling.
    let minor = trajectory.schema_version.as_bytes()[8] - b'0';
    let mut embedded_ids = HashSet::new();
    if let Some(value) = trajectory
        .additional_fields
        .get("subagent_trajectories")
        .filter(|value| !value.is_null())
    {
        let field_path = format!("{prefix}subagent_trajectories");
        if minor < 7 {
            return Err(ParseError::new(
                field_path,
                "embedded subagents require ATIF v1.7 or later",
            ));
        }
        let children = value
            .as_array()
            .ok_or_else(|| ParseError::new(&field_path, "expected an array"))?;
        for (index, value) in children.iter().enumerate() {
            let child_path = format!("{field_path}[{index}]");
            let child: Trajectory = serde_path_to_error::deserialize(value).map_err(|error| {
                let location = error.path().to_string();
                let location = if matches!(location.as_str(), "" | "." | "?") {
                    child_path.clone()
                } else {
                    format!("{child_path}.{location}")
                };
                ParseError::new(location, error.inner().to_string())
            })?;
            let id = child.trajectory_id.as_ref().ok_or_else(|| {
                ParseError::new(
                    format!("{child_path}.trajectory_id"),
                    "required for embedded subagents",
                )
            })?;
            if !embedded_ids.insert(id.clone()) {
                return Err(ParseError::new(
                    format!("{child_path}.trajectory_id"),
                    "duplicate embedded trajectory ID",
                ));
            }
            validate_details(&child, &child_path)?;
        }
    }
    for (index, step) in trajectory.steps.iter().enumerate() {
        let step_path = format!("{prefix}steps[{index}]");
        if let Message::Parts(parts) = &step.message {
            validate_content(parts, minor, &format!("{step_path}.message"))?;
        }
        let Some(results) = step
            .additional_fields
            .get("observation")
            .and_then(|observation| observation.get("results"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        for (index, result) in results.iter().enumerate() {
            let result_path = format!("{step_path}.observation.results[{index}]");
            if let Some(parts) = result.get("content").and_then(Value::as_array) {
                validate_content(parts, minor, &format!("{result_path}.content"))?;
            }
            if let Some(call_id) = result.get("source_call_id").and_then(Value::as_str) {
                let found = step
                    .additional_fields
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .is_some_and(|calls| {
                        calls.iter().any(|call| {
                            call.get("tool_call_id").and_then(Value::as_str) == Some(call_id)
                        })
                    });
                if !found {
                    return Err(ParseError::new(
                        format!("{result_path}.source_call_id"),
                        "must reference a tool call in the same step",
                    ));
                }
            }
            let Some(value) = result
                .get("subagent_trajectory_ref")
                .filter(|value| !value.is_null())
            else {
                continue;
            };
            let refs_path = format!("{result_path}.subagent_trajectory_ref");
            let references = value
                .as_array()
                .ok_or_else(|| ParseError::new(&refs_path, "expected an array"))?;
            for (index, reference) in references.iter().enumerate() {
                let ref_path = format!("{refs_path}[{index}]");
                if !reference.is_object() {
                    return Err(ParseError::new(&ref_path, "expected an object"));
                }
                for name in ["trajectory_id", "session_id", "trajectory_path"] {
                    if reference
                        .get(name)
                        .is_some_and(|value| !value.is_null() && !value.is_string())
                    {
                        return Err(ParseError::new(
                            format!("{ref_path}.{name}"),
                            "expected a string",
                        ));
                    }
                }
                let id = reference.get("trajectory_id").and_then(Value::as_str);
                let file = reference.get("trajectory_path").and_then(Value::as_str);
                let session = reference.get("session_id").and_then(Value::as_str);
                if minor < 7 {
                    if session.is_none() {
                        return Err(ParseError::new(
                            format!("{ref_path}.session_id"),
                            "required in ATIF v1.6 and earlier",
                        ));
                    }
                } else if id.is_none() && file.is_none() {
                    return Err(ParseError::new(
                        &ref_path,
                        "set trajectory_id or trajectory_path; session_id is not a resolution key",
                    ));
                } else if file.is_none() && !id.is_some_and(|id| embedded_ids.contains(id)) {
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

/// Validates ordered text/image/audio parts against the declaring document's ATIF version.
fn validate_content(parts: &[Value], minor: u8, path: &str) -> Result<(), ParseError> {
    if minor < 6 {
        return Err(ParseError::new(
            path,
            "content arrays require ATIF v1.6 or later",
        ));
    }
    for (index, part) in parts.iter().enumerate() {
        let path = format!("{path}[{index}]");
        let kind = part.get("type").and_then(Value::as_str).ok_or_else(|| {
            ParseError::new(format!("{path}.type"), "expected text, image, or audio")
        })?;
        let text = part.get("text").filter(|value| !value.is_null());
        let source = part.get("source").filter(|value| !value.is_null());
        if kind == "text" {
            if !text.is_some_and(Value::is_string) {
                return Err(ParseError::new(
                    format!("{path}.text"),
                    "text content requires a string",
                ));
            }
            if source.is_some() {
                return Err(ParseError::new(
                    format!("{path}.source"),
                    "text content cannot contain a media source",
                ));
            }
            continue;
        }
        if !matches!(kind, "image" | "audio") {
            return Err(ParseError::new(
                format!("{path}.type"),
                "expected text, image, or audio",
            ));
        }
        if kind == "audio" && minor < 8 {
            return Err(ParseError::new(
                format!("{path}.type"),
                "audio requires ATIF v1.8 or later",
            ));
        }
        if text.is_some() {
            return Err(ParseError::new(
                format!("{path}.text"),
                "media content cannot contain text",
            ));
        }
        let source = source.and_then(Value::as_object).ok_or_else(|| {
            ParseError::new(
                format!("{path}.source"),
                "media content requires a source object",
            )
        })?;
        let media_type = source
            .get("media_type")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ParseError::new(
                    format!("{path}.source.media_type"),
                    "expected a MIME type string",
                )
            })?;
        let valid = if kind == "image" {
            matches!(
                media_type,
                "image/jpeg" | "image/png" | "image/gif" | "image/webp"
            )
        } else {
            // Harbor accepts common audio MIME aliases; retain the original spelling in Arrow.
            matches!(
                media_type.trim().to_ascii_lowercase().as_str(),
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
            )
        };
        if !valid {
            return Err(ParseError::new(
                format!("{path}.source.media_type"),
                "MIME type does not match the content type",
            ));
        }
        if !source.get("path").is_some_and(Value::is_string) {
            return Err(ParseError::new(
                format!("{path}.source.path"),
                "media source requires a path string",
            ));
        }
        if let Some(duration) = source.get("duration_sec").filter(|value| !value.is_null()) {
            if kind != "audio"
                || !duration
                    .as_f64()
                    .is_some_and(|duration| duration.is_finite() && duration >= 0.0)
            {
                return Err(ParseError::new(
                    format!("{path}.source.duration_sec"),
                    "only audio may have a finite, nonnegative duration",
                ));
            }
        }
    }
    Ok(())
}
