use arrow_schema::{DataType, Field};
use atif_arrow::trajectory_schema;

#[test]
fn schema_avoids_dynamic_types_and_marks_json_payloads() {
    for field in trajectory_schema().fields() {
        check_field(field);
    }
}

#[test]
fn content_text_is_large_binary_but_media_references_remain_strings() {
    let schema = trajectory_schema();
    assert_eq!(schema.fields().len(), 13);
    let DataType::List(step) = schema.field_with_name("steps").unwrap().data_type() else {
        panic!("expected steps list")
    };
    let DataType::Struct(fields) = step.data_type() else {
        panic!("expected step struct")
    };
    let message = fields
        .iter()
        .find(|field| field.name() == "message")
        .unwrap();
    let DataType::List(part) = message.data_type() else {
        panic!("expected content list")
    };
    let DataType::Struct(parts) = part.data_type() else {
        panic!("expected content struct")
    };
    assert_eq!(parts[1].name(), "text");
    assert_eq!(parts[1].data_type(), &DataType::LargeBinary);
    assert!(parts[1].is_nullable());
    let DataType::Struct(source) = parts[2].data_type() else {
        panic!("expected media source")
    };
    assert_eq!(source[0].data_type(), &DataType::Utf8);
    assert_eq!(source[1].data_type(), &DataType::Utf8);
}

/// Checks storage types and JSON annotations throughout the nested schema.
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
        | DataType::LargeBinary
        | DataType::Int64
        | DataType::UInt64
        | DataType::Float64
        | DataType::Boolean => {}
        other => panic!("unsupported schema type: {other:?}"),
    }
}
