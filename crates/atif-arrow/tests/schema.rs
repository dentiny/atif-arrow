use arrow_schema::{DataType, Field};
use atif_arrow::{trajectory_schema, trajectory_schema_with_media};

#[test]
fn schema_avoids_dynamic_types_and_marks_json_payloads() {
    for field in trajectory_schema().fields() {
        check_field(field);
    }
}

#[test]
fn media_schema_keeps_native_fields_and_uses_storage_neutral_large_binary() {
    let native = trajectory_schema();
    let schema = trajectory_schema_with_media();
    assert_eq!(native.fields().len(), 13);
    assert_eq!(schema.fields().len(), 14);
    assert_eq!(&schema.fields()[..13], native.fields().as_ref());
    assert_eq!(schema.metadata(), native.metadata());
    let media = schema.field(13);
    assert!(!media.is_nullable());
    assert_eq!(media.name(), "media");
    let DataType::List(item) = media.data_type() else {
        panic!("expected list")
    };
    assert!(!item.is_nullable());
    let DataType::Struct(fields) = item.data_type() else {
        panic!("expected struct")
    };
    let payload = fields
        .iter()
        .find(|field| field.name() == "payload")
        .unwrap();
    assert_eq!(payload.data_type(), &DataType::LargeBinary);
    assert!(payload.is_nullable());
    assert!(payload.metadata().is_empty());
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
        | DataType::Int64
        | DataType::UInt64
        | DataType::Float64
        | DataType::Boolean => {}
        other => panic!("unsupported schema type: {other:?}"),
    }
}
