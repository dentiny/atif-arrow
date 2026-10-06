use opendal::{FuturesAsyncReader, Operator};

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
        Self::new(input, format, batch_size, Some(source_uri))
    }
}
