use opendal::{FuturesAsyncReader, Operator};

#[cfg(feature = "gzip")]
use crate::GzipReader;
use crate::{InputFormat, ReadError, TrajectoryReader};

impl TrajectoryReader<FuturesAsyncReader> {
    /// Opens an object through a caller-configured async OpenDAL operator.
    ///
    /// The path is relative to the operator's root. If no source URI is supplied,
    /// that path is used for provenance. Object access may be deferred until reading.
    pub async fn open(
        operator: &Operator,
        path: &str,
        format: InputFormat,
        batch_size: usize,
        source_uri: Option<String>,
    ) -> Result<Self, ReadError> {
        let (input, source_uri) = open_object(operator, path, batch_size, source_uri).await?;
        Self::new(input, format, batch_size, Some(source_uri))
    }
}

#[cfg(feature = "gzip")]
impl TrajectoryReader<GzipReader<FuturesAsyncReader>> {
    /// Like [`TrajectoryReader::open`], but decompresses a GZIP object while reading.
    ///
    /// See [`GzipReader`] for how members are decompressed.
    pub async fn open_gzip(
        operator: &Operator,
        path: &str,
        format: InputFormat,
        batch_size: usize,
        source_uri: Option<String>,
    ) -> Result<Self, ReadError> {
        let (input, source_uri) = open_object(operator, path, batch_size, source_uri).await?;
        Self::new(GzipReader::new(input), format, batch_size, Some(source_uri))
    }
}

async fn open_object(
    operator: &Operator,
    path: &str,
    batch_size: usize,
    source_uri: Option<String>,
) -> Result<(FuturesAsyncReader, String), ReadError> {
    if batch_size == 0 {
        return Err(ReadError::InvalidBatchSize);
    }
    let source_uri = source_uri.unwrap_or_else(|| path.to_owned());
    // Use OpenDAL's AsyncBufRead adapter directly without a second buffering layer.
    let input = async {
        operator
            .reader(path)
            .await?
            .into_futures_async_read(..)
            .await
    }
    .await
    .map_err(|source| ReadError::Open {
        source_uri: source_uri.clone(),
        source,
    })?;
    Ok((input, source_uri))
}
