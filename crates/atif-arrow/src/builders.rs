use arrow_array::{
    builder::{
        ArrayBuilder, BooleanBuilder, Float64Builder, Int64Builder, LargeBinaryBuilder,
        ListBuilder, StringBuilder, StructBuilder, UInt64Builder,
    },
    OffsetSizeTrait,
};
use arrow_schema::{ArrowError, DataType};
use serde_json::Value;

use crate::ConversionError;

/// Checks all offsets before mutating any arrays, then appends the normalized row.
pub(crate) fn append_row(
    builder: &mut StructBuilder,
    value: &Value,
) -> Result<(), ConversionError> {
    let data_type = DataType::Struct(builder.fields().clone());
    check_offsets(builder, &data_type, &[value])?;
    append_object(builder, value);
    Ok(())
}

/// Null structs still append child slots so nested arrays stay aligned.
fn append_object(builder: &mut StructBuilder, value: &Value) {
    let fields = builder.fields().clone();
    for (field, child) in fields.iter().zip(builder.field_builders_mut()) {
        append_value(child.as_mut(), field.data_type(), &value[field.name()]);
    }
    builder.append(!value.is_null());
}

/// A list can contribute many values to the same child array. Check their total
/// growth together, including existing batch offsets, before any append can panic.
fn check_offsets(
    builder: &mut dyn ArrayBuilder,
    data_type: &DataType,
    values: &[&Value],
) -> Result<(), ConversionError> {
    match data_type {
        DataType::Utf8 => check_offset::<i32>(
            typed::<StringBuilder>(builder).values_slice().len(),
            values
                .iter()
                .filter_map(|value| value.as_str())
                .map(str::len),
        ),
        DataType::LargeBinary => check_offset::<i64>(
            typed::<LargeBinaryBuilder>(builder).values_slice().len(),
            values
                .iter()
                .filter_map(|value| value.as_str())
                .map(str::len),
        ),
        DataType::Struct(fields) => {
            for (field, child) in fields
                .iter()
                .zip(typed::<StructBuilder>(builder).field_builders_mut())
            {
                let children: Vec<_> = values.iter().map(|value| &value[field.name()]).collect();
                check_offsets(child.as_mut(), field.data_type(), &children)?;
            }
            Ok(())
        }
        DataType::List(item) => {
            let builder = typed::<ListBuilder<Box<dyn ArrayBuilder>>>(builder);
            let children: Vec<_> = values
                .iter()
                .filter_map(|value| value.as_array())
                .flatten()
                .collect();
            check_offset::<i32>(builder.values_ref().len(), [children.len()].into_iter())?;
            check_offsets(builder.values().as_mut(), item.data_type(), &children)
        }
        _ => Ok(()),
    }
}

fn check_offset<O: OffsetSizeTrait>(
    current: usize,
    mut lengths: impl Iterator<Item = usize>,
) -> Result<(), ConversionError> {
    let end = lengths.try_fold(current, |offset, length| offset.checked_add(length));
    if end.is_none_or(|end| end > O::MAX_OFFSET) {
        return Err(ArrowError::InvalidArgumentError(
            "Arrow offset overflow; reduce the batch size or input size".into(),
        )
        .into());
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::check_offset;

    #[test]
    fn checks_cumulative_offsets_at_their_limits_without_large_allocations() {
        let limit = i32::MAX as usize;
        assert!(check_offset::<i32>(limit - 3, [1, 2].into_iter()).is_ok());
        assert!(check_offset::<i32>(limit - 3, [2, 2].into_iter()).is_err());
        assert!(check_offset::<i32>(limit, [0].into_iter()).is_ok());
        assert!(check_offset::<i64>(limit, [1].into_iter()).is_ok());
        assert!(check_offset::<i64>(i64::MAX as usize, [1].into_iter()).is_err());
        assert!(check_offset::<i64>(usize::MAX - 1, [1, 1].into_iter()).is_err());
    }
}
