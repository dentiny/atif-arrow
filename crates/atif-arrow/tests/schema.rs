use arrow_schema::{DataType, Field};
use atif_arrow::{trajectory_schema, SCHEMA_VERSION};

#[test]
fn schema_is_available_without_input_and_has_a_versioned_layout() {
    let schema = trajectory_schema();
    assert_eq!(schema.fields().len(), 13);
    assert_eq!(schema, trajectory_schema());
    assert_eq!(
        schema.metadata()["atif-arrow.schema_version"],
        SCHEMA_VERSION
    );
    assert_eq!(
        schema.metadata()["atif-arrow.row_granularity"],
        "trajectory"
    );
    assert!(!schema.field_with_name("raw_json").unwrap().is_nullable());
    assert!(schema.field_with_name("session_id").unwrap().is_nullable());
}

#[test]
fn nested_fields_keep_content_order_and_training_data_types() {
    let schema = trajectory_schema();
    let step = list_item(schema.field_with_name("steps").unwrap());
    assert!(!step.is_nullable());
    let message = child(step, "message");
    assert!(!message.is_nullable());
    let part = list_item(message);
    assert_eq!(child(part, "type").data_type(), &DataType::Utf8);
    let media = child(part, "source");
    assert!(media.is_nullable());
    assert!(!child(media, "path").is_nullable());
    assert_eq!(child(media, "duration_sec").data_type(), &DataType::Float64);
    let metrics = child(step, "metrics");
    assert!(metrics.is_nullable());
    assert_eq!(
        list_item(child(metrics, "completion_token_ids")).data_type(),
        &DataType::Int64
    );
    assert_eq!(
        list_item(child(metrics, "logprobs")).data_type(),
        &DataType::Float64
    );
    assert_eq!(
        child(step, "is_copied_context").data_type(),
        &DataType::Boolean
    );
}

#[test]
fn schema_avoids_dynamic_types_and_marks_json_payloads() {
    for field in trajectory_schema().fields() {
        check_field(field);
    }
}

fn check_field(field: &Field) {
    if field.name().ends_with("_json") {
        assert_eq!(field.data_type(), &DataType::Utf8);
        assert_eq!(field.metadata()["atif-arrow.encoding"], "json");
    }
    match field.data_type() {
        DataType::Struct(fields) => fields.iter().for_each(|field| check_field(field)),
        DataType::List(item) => {
            assert!(!item.is_nullable());
            check_field(item);
        }
        DataType::Utf8
        | DataType::Int64
        | DataType::UInt64
        | DataType::Float64
        | DataType::Boolean => {}
        other => panic!("unsupported schema type: {other:?}"),
    }
}

fn child<'a>(field: &'a Field, name: &str) -> &'a Field {
    let DataType::Struct(fields) = field.data_type() else {
        panic!("expected struct")
    };
    fields.iter().find(|field| field.name() == name).unwrap()
}

fn list_item(field: &Field) -> &Field {
    let DataType::List(item) = field.data_type() else {
        panic!("expected list")
    };
    item
}
