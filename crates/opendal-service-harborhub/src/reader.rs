use std::sync::Arc;

use http::{header, HeaderMap, Method, Response, StatusCode};
use opendal_core::raw::{oio, parse_content_range, parse_into_metadata, RpRead};
use opendal_core::{BytesRange, Error, ErrorKind, OperationContext, Result};

use crate::core::HarborHubCore;
use crate::error::parse_error;

/// Opens a streaming HTTP response for each requested object range.
pub(crate) struct HarborHubReader {
    pub core: Arc<HarborHubCore>,
    pub ctx: OperationContext,
    pub path: String,
}

impl oio::StreamRead for HarborHubReader {
    async fn open(&self, range: BytesRange) -> Result<(RpRead, Box<dyn oio::ReadStreamDyn>)> {
        let mut req = self.core.object_request(Method::GET, &self.path)?;
        if !range.is_full() {
            req.headers_mut().insert(
                header::RANGE,
                range.to_header().parse().expect("valid byte range"),
            );
        }
        let resp = self.ctx.http_transport().fetch(req).await?;
        match resp.status() {
            StatusCode::OK if !range.is_full() => Err(Error::new(
                ErrorKind::Unsupported,
                "server ignored the requested byte range",
            )),
            StatusCode::OK | StatusCode::PARTIAL_CONTENT => {
                if resp.status() == StatusCode::PARTIAL_CONTENT {
                    validate_content_range(resp.headers(), range)?;
                }
                let metadata = parse_into_metadata(&self.path, resp.headers())?;
                Ok((RpRead::new(metadata), Box::new(resp.into_body())))
            }
            _ => {
                let (parts, mut body) = resp.into_parts();
                let buffer = body.to_buffer().await?;
                Err(parse_error(Response::from_parts(parts, buffer)))
            }
        }
    }
}

/// Require a partial response to cover exactly the requested range, clipped at EOF.
fn validate_content_range(headers: &HeaderMap, requested: BytesRange) -> Result<()> {
    let content_range = parse_content_range(headers)?.ok_or_else(|| {
        Error::new(
            ErrorKind::Unexpected,
            "206 response is missing Content-Range",
        )
    })?;
    let (Some(actual), Some(total)) = (content_range.range_inclusive(), content_range.size())
    else {
        return Err(Error::new(
            ErrorKind::Unexpected,
            "206 response has incomplete Content-Range",
        ));
    };
    let (start, end) = match requested {
        BytesRange::Range { offset, size } => (
            offset,
            size.map(|size| offset.saturating_add(size))
                .unwrap_or(total)
                .min(total),
        ),
        BytesRange::Suffix { size } => (total.saturating_sub(size), total),
    };
    if start >= end || *actual.start() != start || *actual.end() != end - 1 {
        return Err(Error::new(
            ErrorKind::Unexpected,
            "Content-Range does not match the requested range",
        ));
    }
    Ok(())
}
