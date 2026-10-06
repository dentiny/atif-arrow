use arrow_array::RecordBatch;
use arrow_json::ReaderBuilder;
use arrow_schema::{DataType, Field, Fields};
use serde_json::{json, Map, Value};

use crate::{
    parse::validate_core, trajectory_schema, ConversionError, ParseError, ParsedTrajectory,
};

/// Converts one parsed document into one Arrow row, with caller-supplied source identity.
///
/// The record index is one-based. Only text content is supported at this stage.
pub fn to_record_batch(
    parsed: &ParsedTrajectory,
    source_uri: Option<&str>,
    source_record_index: u64,
) -> Result<RecordBatch, ConversionError> {
    validate_core(&parsed.trajectory).map_err(ConversionError::Field)?;
    if source_record_index == 0 {
        return Err(invalid("source_record_index", "expected a one-based index"));
    }
    let mut document = serde_json::to_value(&parsed.trajectory)
        .map_err(|error| invalid("$", error.to_string()))?;
    document["source_uri"] = json!(source_uri);
    document["source_record_index"] = json!(source_record_index);
    document["raw_json"] = json!(parsed.raw_json());

    let schema = trajectory_schema();
    let row = normalize_object(schema.fields(), &document, "")?;
    // Decode JSON text rather than serializing Value into Arrow's tape: arbitrary-
    // precision numbers must stay numbers, not Serde's private number representation.
    let encoded = row.to_string();
    let mut reader = ReaderBuilder::new(schema).build(encoded.as_bytes())?;
    reader
        .next()
        .ok_or_else(|| invalid("$", "Arrow decoder returned no row"))?
        .map_err(ConversionError::Arrow)
}

/// Projects ATIF fields onto the fixed schema; unknown fields remain in raw_json.
fn normalize_object(fields: &Fields, value: &Value, path: &str) -> Result<Value, ConversionError> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid(path, "expected an object"))?;
    let mut output = Map::new();
    for field in fields {
        let name = match field.name().as_str() {
            "atif_schema_version" => "schema_version",
            "raw_json" => "raw_json",
            name => name.strip_suffix("_json").unwrap_or(name),
        };
        let child_path = if path.is_empty() {
            name.into()
        } else {
            format!("{path}.{name}")
        };
        let value = object.get(name).unwrap_or(&Value::Null);
        output.insert(
            field.name().clone(),
            normalize_field(field, value, &child_path)?,
        );
    }
    Ok(Value::Object(output))
}

/// Normalizes text and JSON payloads, rejecting type mismatches and numeric overflow.
fn normalize_field(field: &Field, value: &Value, path: &str) -> Result<Value, ConversionError> {
    if value.is_null() {
        return if field.is_nullable() {
            Ok(Value::Null)
        } else {
            Err(invalid(path, "required field cannot be missing or null"))
        };
    }
    if field.name() != "raw_json"
        && field
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
            Ok(Value::String(value.to_string()))
        } else {
            Err(invalid(path, "invalid JSON payload type"))
        };
    }
    match field.data_type() {
        DataType::Struct(fields) => normalize_object(fields, value, path),
        DataType::List(item) => {
            let content = matches!(field.name().as_str(), "message" | "content");
            if content && value.is_string() {
                return Ok(json!([{"type": "text", "text": value}]));
            }
            let items = value
                .as_array()
                .ok_or_else(|| invalid(path, "expected an array"))?;
            let mut output = Vec::with_capacity(items.len());
            for (index, value) in items.iter().enumerate() {
                let item_path = format!("{path}[{index}]");
                if content {
                    if value.get("type").and_then(Value::as_str) != Some("text") {
                        return Err(invalid(
                            format!("{item_path}.type"),
                            "only text content is supported",
                        ));
                    }
                    if !value.get("text").is_some_and(Value::is_string) {
                        return Err(invalid(
                            format!("{item_path}.text"),
                            "text content requires a string",
                        ));
                    }
                    if value.get("source").is_some_and(|source| !source.is_null()) {
                        return Err(invalid(
                            format!("{item_path}.source"),
                            "text content cannot contain a media source",
                        ));
                    }
                }
                output.push(normalize_field(item, value, &item_path)?);
            }
            Ok(Value::Array(output))
        }
        data_type => {
            let valid = match data_type {
                DataType::Utf8 => value.is_string(),
                DataType::Boolean => value.is_boolean(),
                DataType::Int64 => value.as_i64().is_some(),
                DataType::UInt64 => value.as_u64().is_some(),
                DataType::Float64 => value.as_f64().is_some_and(f64::is_finite),
                _ => false,
            };
            if valid {
                Ok(value.clone())
            } else {
                Err(invalid(
                    path,
                    format!("value cannot be represented as {data_type}"),
                ))
            }
        }
    }
}

/// Associates conversion failures with their input field location.
fn invalid(path: impl Into<String>, message: impl Into<String>) -> ConversionError {
    ConversionError::Field(ParseError::new(path, message))
}
