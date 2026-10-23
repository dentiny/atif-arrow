use std::{io, sync::Arc};

use arrow_array::{
    builder::{LargeBinaryBuilder, ListBuilder, StringBuilder, StructBuilder, UInt64Builder},
    Array, ArrayRef, RecordBatch, StringArray, UInt64Array,
};
use arrow_schema::{DataType, Field};
use atif_arrow::{media_fields, trajectory_schema, trajectory_schema_with_media};
use base64::{engine::general_purpose::STANDARD, Engine};
use futures::TryStreamExt;
use opendal::Operator;
use serde_json::Value;
use url::Url;

use crate::{error::BoxError, ReadError};

/// Byte budgets count decoded payloads, including repeated references. They do
/// not bound input JSON, Arrow offsets, or allocation overhead.
#[derive(Clone, Copy, Debug)]
pub struct MediaOptions {
    /// False emits references with null payload/size and performs no media I/O.
    pub resolve_media: bool,
    pub max_media_bytes: u64,
    pub max_trajectory_media_bytes: u64,
    pub max_batch_media_bytes: u64,
}

impl Default for MediaOptions {
    fn default() -> Self {
        Self {
            resolve_media: true,
            max_media_bytes: 256 * 1024 * 1024,
            max_trajectory_media_bytes: 2 * 1024 * 1024 * 1024,
            max_batch_media_bytes: 2 * 1024 * 1024 * 1024,
        }
    }
}

/// Resolves media through exactly the caller-configured OpenDAL operator.
/// Relative paths use the input object's directory, independent of provenance.
/// This never constructs operators or forwards credentials to another service.
#[derive(Clone, Debug)]
pub struct MediaResolver {
    operator: Operator,
    document_path: String,
    options: MediaOptions,
    uri_prefix: Option<Url>,
}

impl MediaResolver {
    /// `document_path` is an object path under the operator's root, as in
    /// `TrajectoryReader::open`. Missing/unreadable media fail resolution.
    pub fn new(
        operator: &Operator,
        document_path: &str,
        options: MediaOptions,
    ) -> Result<Self, ReadError> {
        if options.max_media_bytes == 0
            || options.max_trajectory_media_bytes == 0
            || options.max_batch_media_bytes == 0
        {
            return Err(ReadError::InvalidMediaOptions(
                "byte limits must be positive".into(),
            ));
        }
        let document_path = normalize_path(document_path)
            .map_err(|error| ReadError::InvalidMediaOptions(error.to_string()))?;
        let info = operator.info();
        let uri_prefix = if info.scheme() == "fs" {
            Some(Url::from_directory_path(info.root()).map_err(|_| {
                ReadError::InvalidMediaOptions("filesystem root must be absolute".into())
            })?)
        } else {
            None
        };
        Ok(Self {
            operator: operator.clone(),
            document_path,
            options,
            uri_prefix,
        })
    }

    /// Explicitly binds an absolute URI prefix to this operator's root, e.g.
    /// `s3://bucket/prefix/`. Absolute references outside it are rejected.
    /// A prefix is a caller assertion, not an independently authenticated URL.
    pub fn with_uri_prefix(mut self, prefix: &str) -> Result<Self, ReadError> {
        let uri = Url::parse(prefix).map_err(|error| {
            ReadError::InvalidMediaOptions(format!("invalid URI prefix: {error}"))
        })?;
        if uri.cannot_be_a_base()
            || !uri.path().ends_with('/')
            || !uri.username().is_empty()
            || uri.password().is_some()
            || uri.query().is_some()
            || uri.fragment().is_some()
        {
            return Err(ReadError::InvalidMediaOptions(
                "URI prefix must be a directory URI without credentials, query, or fragment".into(),
            ));
        }
        self.uri_prefix = Some(uri);
        Ok(self)
    }

    fn path_from_uri(&self, uri: &Url) -> Result<String, BoxError> {
        let prefix = self.uri_prefix.as_ref().ok_or_else(|| {
            message("absolute media URIs require an explicit with_uri_prefix binding")
        })?;
        if uri.scheme() != prefix.scheme()
            || uri.host_str() != prefix.host_str()
            || uri.port_or_known_default() != prefix.port_or_known_default()
            || !uri.username().is_empty()
            || uri.password().is_some()
            || uri.query().is_some()
            || uri.fragment().is_some()
        {
            return Err(message("media URI is outside the configured storage scope"));
        }
        let path = String::from_utf8(percent_decode(uri.path(), usize::MAX)?)?;
        let root = String::from_utf8(percent_decode(prefix.path(), usize::MAX)?)?;
        let path = path
            .strip_prefix(&root)
            .ok_or_else(|| message("media URI is outside the configured storage root"))?;
        normalize_path(path)
    }

    async fn filesystem_path(&self, path: &std::path::Path) -> Result<String, BoxError> {
        // Resolve symlinks and platform aliases (e.g. /tmp versus /private/tmp)
        // without blocking the async reader. The OpenDAL Fs root is canonical.
        let canonical = tokio::fs::canonicalize(path).await?;
        let root = self.operator.info().root();
        let relative = canonical
            .strip_prefix(&root)
            .map_err(|_| message("media file is outside the configured storage root"))?;
        normalize_path(
            relative
                .to_str()
                .ok_or_else(|| message("media path is not UTF-8"))?,
        )
    }

    async fn storage_path(&self, reference: &str) -> Result<String, BoxError> {
        let filesystem = self.operator.info().scheme() == "fs";
        let path = if let Ok(uri) = Url::parse(reference) {
            if filesystem
                && uri.scheme() == "file"
                && uri.query().is_none()
                && uri.fragment().is_none()
            {
                let path = uri
                    .to_file_path()
                    .map_err(|_| message("invalid media file URI"))?;
                return self.filesystem_path(&path).await;
            }
            self.path_from_uri(&uri)?
        } else if reference.starts_with('/') {
            if filesystem {
                return self.filesystem_path(std::path::Path::new(reference)).await;
            }
            normalize_path(reference)?
        } else {
            let directory = self
                .document_path
                .rsplit_once('/')
                .map_or("", |(parent, _)| parent);
            normalize_path(&format!("{directory}/{reference}"))?
        };
        if filesystem {
            return self
                .filesystem_path(&std::path::Path::new(&self.operator.info().root()).join(path))
                .await;
        }
        Ok(path)
    }

    async fn read(&self, path: &str, limit: u64) -> Result<Vec<u8>, BoxError> {
        let limit = usize::try_from(limit).unwrap_or(usize::MAX);
        if path.starts_with("data:") {
            return decode_data_uri(path, limit);
        }
        let size = if self.operator.info().capability().stat {
            let metadata = self.operator.stat(path).await?;
            if metadata.is_dir() {
                return Err(message("media path names a directory"));
            }
            let size = metadata.content_length();
            if size > limit as u64 {
                return Err(limit_error());
            }
            Some(size)
        } else {
            None
        };
        let mut input = self
            .operator
            .reader(path)
            .await?
            .into_bytes_stream(..)
            .await?;
        let mut bytes = Vec::new();
        while let Some(chunk) = input.try_next().await? {
            if chunk.len() > limit.saturating_sub(bytes.len()) {
                return Err(limit_error());
            }
            bytes.extend_from_slice(&chunk);
        }
        if size.is_some_and(|size| size != bytes.len() as u64) {
            return Err(message("media size changed during read"));
        }
        Ok(bytes)
    }

    /// Adds `media[]` to a native, unprojected ATIF batch. The caller supplies one
    /// resolver per source object; source_uri is provenance, not an I/O authority.
    /// All rows resolve successfully or the whole batch fails.
    pub async fn resolve_batch(&self, batch: RecordBatch) -> Result<RecordBatch, ReadError> {
        if batch.schema() != trajectory_schema() {
            return Err(ReadError::InvalidMediaOptions(
                "resolve_batch requires the native, unprojected trajectory schema".into(),
            ));
        }
        let raw = batch
            .column_by_name("raw_json")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let sources = batch
            .column_by_name("source_uri")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let indices = batch
            .column_by_name("source_record_index")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let fields = media_fields();
        let mut builder =
            ListBuilder::new(StructBuilder::from_fields(fields.clone(), 0)).with_field(Arc::new(
                Field::new("item", DataType::Struct(fields), false),
            ));
        let mut batch_bytes = 0;
        for row in 0..batch.num_rows() {
            let source_uri = (!sources.is_null(row)).then(|| sources.value(row).to_owned());
            let index = indices.value(row);
            let document: Value = serde_json::from_str(raw.value(row))
                .map_err(|error| ReadError::record(&source_uri, index, error.into()))?;
            let mut references = Vec::new();
            document_refs(&document, "", &mut references);
            let mut trajectory_bytes = 0;
            for reference in references {
                let resolve = async {
                    let path = if reference.path.starts_with("data:") {
                        reference.path.to_owned()
                    } else {
                        self.storage_path(reference.path).await?
                    };
                    let limit = self
                        .options
                        .max_media_bytes
                        .min(self.options.max_trajectory_media_bytes - trajectory_bytes)
                        .min(self.options.max_batch_media_bytes - batch_bytes);
                    let payload = self.read(&path, limit).await?;
                    Ok::<_, BoxError>((path, payload))
                };
                let (resolved, payload) = if self.options.resolve_media {
                    let (path, payload) = resolve.await.map_err(|source| ReadError::Media {
                        source_uri: source_uri.clone(),
                        record_index: index,
                        source_locator: reference.locator.clone(),
                        source,
                    })?;
                    trajectory_bytes += payload.len() as u64;
                    batch_bytes += payload.len() as u64;
                    (Some(path), Some(payload))
                } else {
                    (None, None)
                };
                let item = builder.values();
                item.field_builder::<StringBuilder>(0)
                    .unwrap()
                    .append_value(reference.locator);
                item.field_builder::<StringBuilder>(1)
                    .unwrap()
                    .append_value(reference.path);
                item.field_builder::<StringBuilder>(2)
                    .unwrap()
                    .append_option(
                        resolved
                            .as_deref()
                            .filter(|path| !path.starts_with("data:")),
                    );
                item.field_builder::<StringBuilder>(3)
                    .unwrap()
                    .append_value(reference.media_type);
                item.field_builder::<LargeBinaryBuilder>(4)
                    .unwrap()
                    .append_option(payload.as_deref());
                item.field_builder::<UInt64Builder>(5)
                    .unwrap()
                    .append_option(payload.as_ref().map(|bytes| bytes.len() as u64));
                item.field_builder::<StringBuilder>(6)
                    .unwrap()
                    .append_value(if payload.is_some() {
                        "embedded"
                    } else {
                        "not_resolved"
                    });
                item.append(true);
            }
            builder.append(true);
        }
        let mut columns = batch.columns().to_vec();
        columns.push(Arc::new(builder.finish()) as ArrayRef);
        RecordBatch::try_new(trajectory_schema_with_media(), columns)
            .map_err(|error| ReadError::record(&None, 1, error.into()))
    }
}

fn message(text: &str) -> BoxError {
    io::Error::new(io::ErrorKind::InvalidData, text).into()
}
fn limit_error() -> BoxError {
    message("media exceeds configured per-media, per-trajectory, or per-batch byte limit")
}

fn normalize_path(path: &str) -> Result<String, BoxError> {
    if path.contains(['\\', '\0'])
        || path.starts_with("//")
        || Url::parse(path).is_ok()
        || path.ends_with('/')
    {
        return Err(message(
            "expected an object file path within the operator root",
        ));
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(message("media path escapes the operator root"));
                }
            }
            part => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Err(message("media path is empty"));
    }
    Ok(parts.join("/"))
}

fn percent_decode(input: &str, limit: usize) -> Result<Vec<u8>, BoxError> {
    let input = input.as_bytes();
    let mut decoded = Vec::new();
    let mut index = 0;
    while index < input.len() {
        if decoded.len() == limit {
            return Err(limit_error());
        }
        let value = if input[index] == b'%' {
            let hex = input
                .get(index + 1..index + 3)
                .ok_or_else(|| message("invalid percent escape"))?;
            let digit = |value: u8| (value as char).to_digit(16).map(|v| v as u8);
            let high = digit(hex[0]).ok_or_else(|| message("invalid percent escape"))?;
            let low = digit(hex[1]).ok_or_else(|| message("invalid percent escape"))?;
            index += 3;
            high * 16 + low
        } else {
            let value = input[index];
            index += 1;
            value
        };
        decoded.push(value);
    }
    Ok(decoded)
}

fn decode_data_uri(uri: &str, limit: usize) -> Result<Vec<u8>, BoxError> {
    let (header, encoded) = uri
        .split_once(',')
        .ok_or_else(|| message("invalid media data URI"))?;
    if header.ends_with(";base64") {
        let encoded_limit = limit.saturating_add(2).saturating_div(3).saturating_mul(4);
        let encoded = percent_decode(encoded, encoded_limit)?;
        let bytes = STANDARD.decode(encoded)?;
        if bytes.len() > limit {
            return Err(limit_error());
        }
        Ok(bytes)
    } else {
        percent_decode(encoded, limit)
    }
}

struct MediaRef<'a> {
    locator: String,
    path: &'a str,
    media_type: &'a str,
}

fn content_refs<'a>(content: &'a Value, prefix: &str, refs: &mut Vec<MediaRef<'a>>) {
    for (index, part) in content.as_array().into_iter().flatten().enumerate() {
        if !matches!(part["type"].as_str(), Some("image" | "audio")) {
            continue;
        }
        if let (Some(path), Some(media_type)) = (
            part["source"]["path"].as_str(),
            part["source"]["media_type"].as_str(),
        ) {
            refs.push(MediaRef {
                locator: format!("{prefix}/{index}"),
                path,
                media_type,
            });
        }
    }
}

fn document_refs<'a>(document: &'a Value, prefix: &str, refs: &mut Vec<MediaRef<'a>>) {
    for (index, step) in document["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let step_path = format!("{prefix}/steps/{index}");
        content_refs(&step["message"], &format!("{step_path}/message"), refs);
        for (index, result) in step["observation"]["results"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            content_refs(
                &result["content"],
                &format!("{step_path}/observation/results/{index}/content"),
                refs,
            );
        }
    }
    for (index, child) in document["subagent_trajectories"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        document_refs(
            child,
            &format!("{prefix}/subagent_trajectories/{index}"),
            refs,
        );
    }
}
