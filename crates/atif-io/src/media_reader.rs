use arrow_array::RecordBatch;
use futures::{future::BoxFuture, io::AsyncBufRead, stream, Stream};

use crate::{MediaResolver, ReadError, TrajectoryReader};

/// An opt-in trajectory reader that emits actual media bytes in Arrow.
/// Pending media work survives cancellation of `next_batch`; errors stop input.
pub struct MediaReader<R> {
    reader: TrajectoryReader<R>,
    resolver: MediaResolver,
    pending: Option<BoxFuture<'static, Result<RecordBatch, ReadError>>>,
    done: bool,
}

impl<R: AsyncBufRead + Unpin> MediaReader<R> {
    pub(crate) fn new(reader: TrajectoryReader<R>, resolver: MediaResolver) -> Self {
        Self {
            reader,
            resolver,
            pending: None,
            done: false,
        }
    }

    /// The complete output schema, available before the first media read.
    pub fn schema(&self) -> arrow_schema::SchemaRef {
        atif_arrow::trajectory_schema_with_media()
    }

    /// Returns the next complete batch. A failed media read never yields a
    /// partially populated batch, and is never silently replaced with a path.
    pub async fn next_batch(&mut self) -> Result<Option<RecordBatch>, ReadError> {
        if self.done {
            return Ok(None);
        }
        if self.pending.is_none() {
            match self.reader.next_batch().await {
                Ok(Some(batch)) => {
                    let resolver = self.resolver.clone();
                    self.pending =
                        Some(Box::pin(async move { resolver.resolve_batch(batch).await }));
                }
                Ok(None) => {
                    self.done = true;
                    return Ok(None);
                }
                Err(error) => {
                    self.done = true;
                    return Err(error);
                }
            }
        }
        let result = self.pending.as_mut().unwrap().await;
        self.pending = None;
        self.done = result.is_err();
        result.map(Some)
    }

    /// An ordered Arrow stream ending at EOF or after its first error.
    pub fn into_stream(self) -> impl Stream<Item = Result<RecordBatch, ReadError>> {
        stream::try_unfold(self, |mut reader| async move {
            Ok(reader.next_batch().await?.map(|batch| (batch, reader)))
        })
    }
}
