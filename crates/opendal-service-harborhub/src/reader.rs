use std::sync::Arc;

use http::{header, Method, Response, StatusCode};
use opendal_core::raw::{oio, parse_into_metadata, RpRead};
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
