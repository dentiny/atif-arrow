use http::{Response, StatusCode};
use opendal_core::{Buffer, Error, ErrorKind};

/// Map both HTTP statuses and Supabase's legacy JSON error codes into OpenDAL errors.
pub(crate) fn parse_error(resp: Response<Buffer>) -> Error {
    let status = resp.status();
    let body = resp.body().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    let code = payload.get("code").and_then(|v| v.as_str());
    let kind = match code {
        Some("NoSuchKey" | "NoSuchBucket") => ErrorKind::NotFound,
        Some("AccessDenied" | "InvalidJWT" | "ExpiredToken") => ErrorKind::PermissionDenied,
        Some("InvalidRange") => ErrorKind::RangeNotSatisfied,
        _ if payload.get("statusCode").and_then(|v| v.as_str()) == Some("404") => {
            ErrorKind::NotFound
        }
        _ => match status {
            StatusCode::NOT_FOUND => ErrorKind::NotFound,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ErrorKind::PermissionDenied,
            StatusCode::RANGE_NOT_SATISFIABLE => ErrorKind::RangeNotSatisfied,
            _ => ErrorKind::Unexpected,
        },
    };
    let mut error =
        Error::new(kind, "Harbor Hub request failed").with_context("http_status", status.as_str());
    if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS {
        error = error.set_temporary();
    }
    error
}
