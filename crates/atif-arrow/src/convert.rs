use std::borrow::Cow;

use arrow_array::RecordBatch;
use arrow_schema::{DataType, Field, FieldRef, Fields};
use serde_json::{json, Map, Value};

use crate::{
    trajectory_schema,
    validation::{validate_details, validate_embedded},
    ConversionError, ParseError, ParsedTrajectory, Trajectory, TrajectoryBatchBuilder,
};

/// Converts one parsed document into one Arrow row, with caller-supplied source identity.
///
/// The record index is one-based. Media references remain unchanged; no files are fetched.
pub fn to_record_batch(
    parsed: &ParsedTrajectory,
    source_uri: Option<&str>,
    source_record_index: u64,
) -> Result<RecordBatch, ConversionError> {
    let mut builder = TrajectoryBatchBuilder::new(1, None)?;
    builder.append_trajectory(
        &parsed.trajectory,
        parsed.raw_json(),
        source_uri,
        source_record_index,
    )?;
    Ok(builder
        .flush()?
        .expect("one appended trajectory produces a batch"))
}

/// Validates and projects a trajectory onto the output schema before any builders mutate.
pub(crate) fn normalize_trajectory(
    trajectory: &Trajectory,
    raw_json: &str,
    source_uri: Option<&str>,
    source_record_index: u64,
    output_fields: &Fields,
) -> Result<Value, ConversionError> {
    if source_record_index == 0 {
        return Err(ConversionError::Field(ParseError::new(
            "source_record_index",
            "expected a one-based index",
        )));
    }
    let schema = trajectory_schema();
    let mut row = Map::new();
    for field in document_fields(schema.fields()) {
        let name = input_name(field);
        let selected = output_fields
            .iter()
            .any(|output| output.name() == field.name());
        // Borrow extension payloads; serialize typed envelope fields individually.
        let value = if let Some(value) = trajectory.additional_fields.get(name) {
            Cow::Borrowed(value)
        } else {
            match name {
                "schema_version" => Cow::Owned(Value::String(trajectory.schema_version.clone())),
                "session_id" => Cow::Owned(json_value(&trajectory.session_id)?),
                "trajectory_id" => Cow::Owned(json_value(&trajectory.trajectory_id)?),
                "agent" => Cow::Owned(json_value(&trajectory.agent)?),
                "steps" => Cow::Owned(json_value(&trajectory.steps)?),
                name => Cow::Borrowed(
                    trajectory
                        .additional_fields
                        .get(name)
                        .unwrap_or(&Value::Null),
                ),
            }
        };
        let normalized = normalize_field(field, &value, name, selected)?;
        if selected {
            row.insert(field.name().clone(), normalized);
        }
    }
    validate_details(trajectory, "").map_err(ConversionError::Field)?;
    // Provenance is already validated; avoid copying raw_json unless selected.
    for field in output_fields {
        let value = match field.name().as_str() {
            "source_uri" => json!(source_uri),
            "source_record_index" => json!(source_record_index),
            "raw_json" => json!(raw_json),
            _ => continue,
        };
        row.insert(field.name().clone(), value);
    }
    Ok(Value::Object(row))
}

fn json_value(value: &impl serde::Serialize) -> Result<Value, ConversionError> {
    serde_json::to_value(value)
        .map_err(|error| ConversionError::Field(ParseError::new("$", error.to_string())))
}

fn input_name(field: &Field) -> &str {
    match field.name().as_str() {
        "atif_schema_version" => "schema_version",
        name => name.strip_suffix("_json").unwrap_or(name),
    }
}

/// Selects ATIF fields without the caller-supplied provenance columns.
fn document_fields(fields: &Fields) -> impl Iterator<Item = &FieldRef> {
    fields.iter().filter(|field| {
        !matches!(
            field.name().as_str(),
            "source_uri" | "source_record_index" | "raw_json"
        )
    })
}

/// Projects ATIF fields onto the fixed schema; unknown fields remain in raw_json.
fn normalize_object<'a>(
    fields: impl IntoIterator<Item = &'a FieldRef>,
    value: &Value,
    path: &str,
    materialize: bool,
) -> Result<Value, ConversionError> {
    let object = value
        .as_object()
        .ok_or_else(|| ConversionError::Field(ParseError::new(path, "expected an object")))?;
    let mut output = Map::new();
    for field in fields {
        let name = input_name(field);
        let value = object.get(name).unwrap_or(&Value::Null);
        // Missing nullable fields need no error-path allocation.
        if value.is_null() && field.is_nullable() {
            if materialize {
                output.insert(field.name().clone(), Value::Null);
            }
            continue;
        }
        let child_path = if path.is_empty() {
            name.into()
        } else {
            format!("{path}.{name}")
        };
        let normalized = normalize_field(field, value, &child_path, materialize)?;
        if materialize {
            output.insert(field.name().clone(), normalized);
        }
    }
    Ok(Value::Object(output))
}

/// Normalizes text and JSON payloads, rejecting type mismatches and numeric overflow.
fn normalize_field(
    field: &Field,
    value: &Value,
    path: &str,
    materialize: bool,
) -> Result<Value, ConversionError> {
    if value.is_null() {
        return if field.is_nullable() {
            Ok(Value::Null)
        } else {
            Err(ConversionError::Field(ParseError::new(
                path,
                "required field cannot be missing or null",
            )))
        };
    }
    if field
        .metadata()
        .get("atif-arrow.encoding")
        .map(String::as_str)
        == Some("json")
    {
        let valid = match field.name().as_str() {
            "reasoning_effort_json" => value.is_string() || value.is_number(),
            "tool_definitions_json" | "subagent_trajectories_json" => value
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_object)),
            _ => value.is_object(),
        };
        return if valid {
            if field.name() == "subagent_trajectories_json" {
                // Validate embedded fields without changing their original JSON.
                let schema = trajectory_schema();
                for (index, child) in value.as_array().into_iter().flatten().enumerate() {
                    let child_path = format!("{path}[{index}]");
                    normalize_object(document_fields(schema.fields()), child, &child_path, false)?;
                    validate_embedded(child, &child_path).map_err(ConversionError::Field)?;
                }
            }
            Ok(if materialize {
                Value::String(value.to_string())
            } else {
                Value::Null
            })
        } else {
            Err(ConversionError::Field(ParseError::new(
                path,
                "invalid JSON payload type",
            )))
        };
    }
    match field.data_type() {
        DataType::Struct(fields) => normalize_object(fields.iter(), value, path, materialize),
        DataType::List(item) => {
            let content = matches!(field.name().as_str(), "message" | "content");
            if content && value.is_string() {
                return Ok(if materialize {
                    json!([{"type": "text", "text": value}])
                } else {
                    Value::Null
                });
            }
            let items = value.as_array().ok_or_else(|| {
                ConversionError::Field(ParseError::new(path, "expected an array"))
            })?;
            let mut output = if materialize {
                Vec::with_capacity(items.len())
            } else {
                Vec::new()
            };
            for (index, value) in items.iter().enumerate() {
                let item_path = format!("{path}[{index}]");
                let normalized = normalize_field(item, value, &item_path, materialize)?;
                if materialize {
                    output.push(normalized);
                }
            }
            Ok(Value::Array(output))
        }
        data_type => {
            let valid = match data_type {
                DataType::Utf8 | DataType::LargeBinary => value.is_string(),
                DataType::Boolean => value.is_boolean(),
                DataType::Int64 => value.as_i64().is_some(),
                DataType::UInt64 => value.as_u64().is_some(),
                DataType::Float64 => value.as_f64().is_some_and(f64::is_finite),
                _ => false,
            };
            if valid {
                Ok(if materialize {
                    value.clone()
                } else {
                    Value::Null
                })
            } else {
                Err(ConversionError::Field(ParseError::new(
                    path,
                    format!("value cannot be represented as {data_type}"),
                )))
            }
        }
    }
}
