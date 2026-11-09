use std::sync::Arc;

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
    if field.name() == "text" {
        assert_eq!(field.data_type(), &DataType::LargeBinary);
        assert!(field.is_nullable());
    }
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

#[test]
fn schema_is_shared_across_threads_and_copy_on_write_stays_isolated() {
    let original = trajectory_schema();
    let handles: Vec<_> = (0..8)
        .map(|_| std::thread::spawn(trajectory_schema))
        .collect();
    for handle in handles {
        assert!(Arc::ptr_eq(&original, &handle.join().unwrap()));
    }
    let mut customized = trajectory_schema();
    Arc::make_mut(&mut customized)
        .metadata
        .insert("caller".into(), "custom".into());
    assert_eq!(customized.metadata()["caller"], "custom");
    assert!(!trajectory_schema().metadata().contains_key("caller"));
    assert_eq!(original.fields(), customized.fields());
}
