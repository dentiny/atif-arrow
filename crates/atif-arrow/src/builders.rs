use arrow_array::builder::{
    ArrayBuilder, BooleanBuilder, Float64Builder, Int64Builder, LargeBinaryBuilder, ListBuilder,
    StringBuilder, StructBuilder, UInt64Builder,
};
use arrow_schema::DataType;
use serde_json::Value;

/// Appends a fully normalized object to builders created from the same schema.
/// Null structs still append child slots so nested arrays stay aligned.
pub(crate) fn append_object(builder: &mut StructBuilder, value: &Value) {
    let fields = builder.fields().clone();
    for (field, child) in fields.iter().zip(builder.field_builders_mut()) {
        append_value(child.as_mut(), field.data_type(), &value[field.name()]);
    }
    builder.append(!value.is_null());
}

/// Writes validated values directly into their final Arrow types. Content text
/// bypasses Arrow JSON's hex decoding and goes straight into a LargeBinary buffer.
fn append_value(builder: &mut dyn ArrayBuilder, data_type: &DataType, value: &Value) {
    match data_type {
        DataType::Utf8 => typed::<StringBuilder>(builder).append_option(value.as_str()),
        DataType::LargeBinary => {
            typed::<LargeBinaryBuilder>(builder).append_option(value.as_str().map(str::as_bytes));
        }
        DataType::Boolean => typed::<BooleanBuilder>(builder).append_option(value.as_bool()),
        DataType::Int64 => typed::<Int64Builder>(builder).append_option(value.as_i64()),
        DataType::UInt64 => typed::<UInt64Builder>(builder).append_option(value.as_u64()),
        DataType::Float64 => typed::<Float64Builder>(builder).append_option(value.as_f64()),
        DataType::Struct(_) => append_object(typed::<StructBuilder>(builder), value),
        DataType::List(item) => {
            let builder = typed::<ListBuilder<Box<dyn ArrayBuilder>>>(builder);
            if let Some(items) = value.as_array() {
                for value in items {
                    append_value(builder.values().as_mut(), item.data_type(), value);
                }
            }
            builder.append(!value.is_null());
        }
        other => unreachable!("unsupported trajectory schema type: {other}"),
    }
}

/// Arrow's schema-driven factory returns erased builders, including list children.
/// Their concrete types are guaranteed by the same schema used for appending.
fn typed<T: ArrayBuilder>(builder: &mut dyn ArrayBuilder) -> &mut T {
    builder
        .as_any_mut()
        .downcast_mut()
        .expect("builder type must match the trajectory schema")
}
