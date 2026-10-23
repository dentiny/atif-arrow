use std::sync::Arc;

use arrow_schema::{DataType, Field, Fields, Schema, SchemaRef};

use crate::trajectory_schema;

/// Fields of an image/audio attachment. Payloads use ordinary Arrow binary;
/// storage consumers choose their own Blob representation and compression.
pub fn media_fields() -> Fields {
    vec![
        Field::new("source_locator", DataType::Utf8, false),
        Field::new("path", DataType::Utf8, false),
        Field::new("resolved_uri", DataType::Utf8, true),
        Field::new("media_type", DataType::Utf8, false),
        Field::new("payload", DataType::LargeBinary, true),
        Field::new("byte_size", DataType::UInt64, true),
        Field::new("resolution_status", DataType::Utf8, false),
    ]
    .into()
}

/// Dataset-owned attachments in encounter order; each locator is an RFC 6901
/// JSON pointer into the trajectory's original `raw_json` content part.
pub fn media_field() -> Field {
    Field::new(
        "media",
        DataType::List(Arc::new(Field::new(
            "item",
            DataType::Struct(media_fields()),
            false,
        ))),
        false,
    )
}

/// The existing trajectory contract plus a non-null `media` list. Pure parsing
/// still uses [`trajectory_schema`]; I/O readers can opt into this richer schema.
pub fn trajectory_schema_with_media() -> SchemaRef {
    let native = trajectory_schema();
    let mut fields = native.fields().to_vec();
    fields.push(Arc::new(media_field()));
    Arc::new(Schema::new_with_metadata(fields, native.metadata().clone()))
}
