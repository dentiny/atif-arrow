use std::sync::{Arc, Mutex};

use atif_io::{InputFormat, TrajectoryReader};
use http::{Method, Request, Response, StatusCode};
use opendal_core::{
    Buffer, ErrorKind, HttpBody, HttpTransport, HttpTransporter, OperationContext, Operator, Result,
};
use opendal_service_harborhub::HarborHub;

const DOCUMENT: &str = r#"{"schema_version":"ATIF-v1.8","agent":{"name":"test","version":"1"},"steps":[{"step_id":1,"source":"user","message":"hello"}]}"#;

/// Scripted HTTP transport verifies Harbor requests without contacting a real account.
#[derive(Clone, Default)]
struct HubTransport {
    requests: Arc<Mutex<usize>>,
    object_error: Option<(StatusCode, &'static str)>,
    ignore_range: bool,
    head_bad_request: bool,
    missing_content_range: bool,
    content_range: Option<&'static str>,
}

impl HttpTransport for HubTransport {
    async fn fetch(&self, req: Request<Buffer>) -> Result<Response<HttpBody>> {
        assert_eq!(req.headers()["apikey"], "publishable-test-key");
        let mut requests = self.requests.lock().unwrap();
        let mut response = Response::builder();
        assert!(req.headers().get("authorization").is_none());
        let body = {
            assert_eq!(
                req.uri().path(),
                "/storage/v1/object/results/trials/%E8%AF%95%E9%AA%8C%3F%23%20%25/trajectory.json"
            );
            assert!(req.uri().query().is_none());
            *requests += 1;
            if req.method() == Method::HEAD && self.head_bad_request {
                response = response.status(StatusCode::BAD_REQUEST);
                String::new()
            } else if let Some((status, body)) = self.object_error {
                response = response.status(status);
                body.into()
            } else if req.method() == Method::HEAD {
                response = response
                    .header("content-length", DOCUMENT.len())
                    .header("content-type", "application/json");
                String::new()
            } else {
                assert_eq!(req.method(), Method::GET);
                if let Some(range) = req.headers().get("range").filter(|_| !self.ignore_range) {
                    let (start, end) = range
                        .to_str()
                        .unwrap()
                        .strip_prefix("bytes=")
                        .unwrap()
                        .split_once('-')
                        .unwrap();
                    let start: usize = start.parse().unwrap();
                    let end = if end.is_empty() {
                        DOCUMENT.len() - 1
                    } else {
                        end.parse::<usize>().unwrap().min(DOCUMENT.len() - 1)
                    };
                    response = response
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header("content-length", end - start + 1);
                    if !self.missing_content_range {
                        let normal_range = format!("bytes {start}-{end}/{}", DOCUMENT.len());
                        response = response
                            .header("content-range", self.content_range.unwrap_or(&normal_range));
                    }
                    DOCUMENT[start..=end].into()
                } else {
                    response = response.header("content-length", DOCUMENT.len());
                    DOCUMENT.into()
                }
            }
        };
        // Separate response chunks exercise streaming rather than a buffered object response.
        let midpoint = body.len() / 2;
        let chunks = vec![
            Ok(Buffer::from(body[..midpoint].to_owned())),
            Ok(Buffer::from(body[midpoint..].to_owned())),
        ];
        Ok(response
            .body(HttpBody::new(futures::stream::iter(chunks), None))
            .unwrap())
    }
}

/// Build the production backend with an isolated test transport and encoded root.
fn operator(transport: &HubTransport) -> Operator {
    let builder = HarborHub::default()
        .endpoint("http://127.0.0.1:1234/")
        .publishable_key("publishable-test-key")
        .root("trials/试验?# %");
    Operator::new(builder).unwrap().with_context(
        OperationContext::new().with_http_transport(HttpTransporter::new(transport.clone())),
    )
}

#[test]
fn streams_objects_and_ranges_into_existing_arrow_reader() {
    futures::executor::block_on(async {
        let transport = HubTransport::default();
        let operator = operator(&transport);
        let metadata = operator.stat("trajectory.json").await.unwrap();
        assert_eq!(metadata.content_length(), DOCUMENT.len() as u64);
        assert_eq!(metadata.content_type(), Some("application/json"));
        assert_eq!(
            operator.read("trajectory.json").await.unwrap().to_bytes(),
            DOCUMENT.as_bytes()
        );
        assert_eq!(
            operator
                .read_with("trajectory.json")
                .range(2..7)
                .await
                .unwrap()
                .to_bytes(),
            &DOCUMENT.as_bytes()[2..7]
        );
        let mut reader = TrajectoryReader::open(
            &operator,
            "trajectory.json",
            InputFormat::Json,
            8,
            Some("harborhub://trials/example/trajectory.json".into()),
        )
        .await
        .unwrap();
        assert_eq!(reader.next_batch().await.unwrap().unwrap().num_rows(), 1);
        assert!(reader.next_batch().await.unwrap().is_none());
        assert_eq!(*transport.requests.lock().unwrap(), 4);
        assert!(!operator.info().capability().list);
        assert_eq!(
            operator
                .write("trajectory.json", "x")
                .await
                .unwrap_err()
                .kind(),
            ErrorKind::Unsupported
        );
    });
}

#[test]
fn maps_storage_errors_and_rejects_ignored_ranges() {
    futures::executor::block_on(async {
        for (status, body, kind, temporary) in [
            (StatusCode::NOT_FOUND, "", ErrorKind::NotFound, false),
            (
                StatusCode::BAD_REQUEST,
                r#"{"statusCode":"404"}"#,
                ErrorKind::NotFound,
                false,
            ),
            (
                StatusCode::BAD_REQUEST,
                r#"{"code":"InvalidJWT"}"#,
                ErrorKind::PermissionDenied,
                false,
            ),
            (
                StatusCode::FORBIDDEN,
                "",
                ErrorKind::PermissionDenied,
                false,
            ),
            (
                StatusCode::RANGE_NOT_SATISFIABLE,
                "",
                ErrorKind::RangeNotSatisfied,
                false,
            ),
            (
                StatusCode::TOO_MANY_REQUESTS,
                "",
                ErrorKind::Unexpected,
                true,
            ),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "",
                ErrorKind::Unexpected,
                true,
            ),
        ] {
            let transport = HubTransport {
                object_error: Some((status, body)),
                ..Default::default()
            };
            let error = operator(&transport)
                .read("trajectory.json")
                .await
                .unwrap_err();
            assert_eq!(error.kind(), kind);
            assert_eq!(error.is_temporary(), temporary);
        }
        let transport = HubTransport {
            ignore_range: true,
            ..Default::default()
        };
        assert_eq!(
            operator(&transport)
                .read_with("trajectory.json")
                .range(2..7)
                .await
                .unwrap_err()
                .kind(),
            ErrorKind::Unsupported
        );
    });
}

#[test]
fn validates_configuration() {
    for endpoint in [
        "",
        "file:///tmp",
        "https://user:pass@example.com",
        "https://example.com?query=x",
        "https://example.com#fragment",
    ] {
        assert_eq!(
            Operator::new(HarborHub::default().endpoint(endpoint))
                .unwrap_err()
                .kind(),
            ErrorKind::ConfigInvalid
        );
    }
    assert_eq!(
        Operator::new(HarborHub::default().publishable_key("bad\nheader"))
            .unwrap_err()
            .kind(),
        ErrorKind::ConfigInvalid
    );
}

#[test]
fn stat_resolves_legacy_head_errors_without_assuming_not_found() {
    futures::executor::block_on(async {
        let transport = HubTransport {
            head_bad_request: true,
            object_error: Some((
                StatusCode::BAD_REQUEST,
                r#"{"code":"NoSuchKey","statusCode":"404"}"#,
            )),
            ..Default::default()
        };
        let op = operator(&transport);
        assert_eq!(
            op.stat("trajectory.json").await.unwrap_err().kind(),
            ErrorKind::NotFound
        );
        assert!(!op.exists("trajectory.json").await.unwrap());
        assert_eq!(*transport.requests.lock().unwrap(), 4);

        let denied = HubTransport {
            head_bad_request: true,
            object_error: Some((StatusCode::BAD_REQUEST, r#"{"code":"AccessDenied"}"#)),
            ..Default::default()
        };
        assert_eq!(
            operator(&denied)
                .stat("trajectory.json")
                .await
                .unwrap_err()
                .kind(),
            ErrorKind::PermissionDenied
        );

        // A 400 from HEAD does not by itself establish whether an object exists.
        let readable = HubTransport {
            head_bad_request: true,
            ..Default::default()
        };
        assert_eq!(
            operator(&readable)
                .stat("trajectory.json")
                .await
                .unwrap()
                .content_length(),
            DOCUMENT.len() as u64
        );
    });
}

#[test]
fn rejects_missing_or_mismatched_content_ranges() {
    futures::executor::block_on(async {
        for content_range in [
            None,
            Some("bytes 0-4/130"),
            Some("bytes 2-5/130"),
            Some("bytes 2-7/130"),
            Some("bytes 2-6/*"),
            Some("bytes */130"),
            Some("bytes 2-6/4"),
            Some("bytes 2-18446744073709551615/18446744073709551615"),
        ] {
            let transport = HubTransport {
                missing_content_range: content_range.is_none(),
                content_range,
                ..Default::default()
            };
            let error = operator(&transport)
                .read_with("trajectory.json")
                .range(2..7)
                .await
                .unwrap_err();
            assert_eq!(error.kind(), ErrorKind::Unexpected);
        }
        // Open-ended ranges remain valid through the object's end.
        let transport = HubTransport::default();
        let buffer = operator(&transport)
            .read_with("trajectory.json")
            .range(2..)
            .await
            .unwrap();
        assert_eq!(buffer.to_bytes(), &DOCUMENT.as_bytes()[2..]);
    });
}
