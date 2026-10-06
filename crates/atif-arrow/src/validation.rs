use crate::{ParseError, Trajectory};

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
