use arrow_schema::{DataType, Field};
use atif_arrow::trajectory_schema;

#[test]
fn schema_avoids_dynamic_types_and_marks_json_payloads() {
    for field in trajectory_schema().fields() {
        check_field(field);
    }
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
