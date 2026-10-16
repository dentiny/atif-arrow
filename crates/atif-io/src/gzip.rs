use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use async_compression::futures::bufread::GzipDecoder;
use futures::io::{AsyncBufRead, AsyncRead, BufReader};

/// An async reader which decompresses a multi-member GZIP stream.
///
/// Decompression is incremental, so memory use stays bounded regardless of member size.
/// Concatenated members, such as `cat a.jsonl.gz b.jsonl.gz`, read as one stream.
pub struct GzipReader<R>(BufReader<GzipDecoder<R>>);

impl<R: AsyncBufRead> GzipReader<R> {
    /// Create a reader which decompresses `inner`.
    pub fn new(inner: R) -> Self {
        let mut decoder = GzipDecoder::new(inner);
        decoder.multiple_members(true);
        GzipReader(BufReader::with_capacity(64 * 1024, decoder))
    }
}

impl<R: AsyncBufRead + Unpin> AsyncBufRead for GzipReader<R> {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        Pin::new(&mut self.get_mut().0).poll_fill_buf(cx)
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        Pin::new(&mut self.get_mut().0).consume(amt)
    }
}

impl<R: AsyncBufRead + Unpin> AsyncRead for GzipReader<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_read(cx, buf)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{ErrorKind, Write};

    use flate2::{write::GzEncoder, Compression};
    use futures::io::{AsyncReadExt, BufReader, Cursor};

    use super::GzipReader;

    fn member(data: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    async fn read_in_small_chunks(compressed: &[u8]) -> std::io::Result<Vec<u8>> {
        let inner = BufReader::with_capacity(7, Cursor::new(compressed));
        let mut output = Vec::new();
        GzipReader::new(inner).read_to_end(&mut output).await?;
        Ok(output)
    }

    #[tokio::test]
    async fn decodes_members_split_across_reads() {
        let large: Vec<u8> = (0..300_000u32).map(|i| (i * 31 % 251) as u8).collect();
        let mut compressed = member(b"first");
        compressed.extend(member(&large));
        compressed.extend(member(b""));
        compressed.extend(member(b"last"));

        let mut expected = b"first".to_vec();
        expected.extend(&large);
        expected.extend(b"last");
        assert_eq!(read_in_small_chunks(&compressed).await.unwrap(), expected);
    }

    #[tokio::test]
    async fn truncated_member_is_an_error() {
        let compressed = member(b"hello");
        let error = read_in_small_chunks(&compressed[..compressed.len() - 1])
            .await
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnexpectedEof);
    }
}
