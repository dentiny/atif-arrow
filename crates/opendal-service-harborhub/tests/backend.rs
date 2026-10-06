use atif_io::{InputFormat, TrajectoryReader};
use http::{Method, Request, Response};
use opendal_core::{
    Buffer, ErrorKind, HttpBody, HttpTransport, HttpTransporter, OperationContext, Operator, Result,
};
use opendal_service_harborhub::HarborHub;

const DOCUMENT: &str = r#"{"schema_version":"ATIF-v1.8","agent":{"name":"test","version":"1"},"steps":[{"step_id":1,"source":"user","message":"hello"}]}"#;

/// Verifies requests and streams the response supplied by each test.
struct HubTransport<F>(F);

impl<F> HttpTransport for HubTransport<F>
where
    F: Fn(Request<Buffer>) -> Response<Buffer> + Send + Sync + Unpin + 'static,
{
    async fn fetch(&self, req: Request<Buffer>) -> Result<Response<HttpBody>> {
        assert_eq!(req.headers()["apikey"], "publishable-test-key");
        assert!(req.headers().get("authorization").is_none());
        assert_eq!(
            req.uri().path(),
            "/storage/v1/object/results/trials/%E8%AF%95%E9%AA%8C%3F%23%20%25/trajectory.json"
        );
        assert!(req.uri().query().is_none());
        let (parts, body) = (self.0)(req).into_parts();
        let midpoint = body.len() / 2;
        // Buffer slices share their bytes; separate chunks exercise streaming.
        let chunks = [Ok(body.slice(..midpoint)), Ok(body.slice(midpoint..))];
        Ok(Response::from_parts(
            parts,
            HttpBody::new(futures::stream::iter(chunks), None),
        ))
    }
}

/// Builds the production backend with an isolated response handler and encoded root.
fn operator(
    reply: impl Fn(Request<Buffer>) -> Response<Buffer> + Send + Sync + Unpin + 'static,
) -> Operator {
    let builder = HarborHub::default()
        .endpoint("http://127.0.0.1:1234/")
        .publishable_key("publishable-test-key")
        .root("trials/试验?# %");
    Operator::new(builder).unwrap().with_context(
        OperationContext::new().with_http_transport(HttpTransporter::new(HubTransport(reply))),
    )
}

/// Creates a response whose body length and media type match its headers.
fn response(status: u16, body: &'static str) -> Response<Buffer> {
    Response::builder()
        .status(status)
        .header("content-length", body.len())
        .header("content-type", "application/json")
        .body(Buffer::from(body))
        .unwrap()
}

#[test]
fn streams_objects_and_ranges_into_existing_arrow_reader() {
    futures::executor::block_on(async {
        let metadata = operator(|req| {
            assert_eq!(req.method(), Method::HEAD);
            let mut reply = response(200, "");
            reply
                .headers_mut()
                .insert("content-length", DOCUMENT.len().into());
            reply
        })
        .stat("trajectory.json")
        .await
        .unwrap();
        assert_eq!(metadata.content_length(), DOCUMENT.len() as u64);
        assert_eq!(metadata.content_type(), Some("application/json"));

        let full = operator(|req| {
            assert_eq!(req.method(), Method::GET);
            assert!(req.headers().get("range").is_none());
            response(200, DOCUMENT)
        });
        assert_eq!(
            full.read("trajectory.json").await.unwrap().to_bytes(),
            DOCUMENT.as_bytes()
        );
        let mut reader = TrajectoryReader::open(
            &full,
            "trajectory.json",
            InputFormat::Json,
            8,
            Some("harborhub://trials/example/trajectory.json".into()),
        )
        .await
        .unwrap();
        assert_eq!(reader.next_batch().await.unwrap().unwrap().num_rows(), 1);
        assert!(reader.next_batch().await.unwrap().is_none());
        assert!(!full.info().capability().list);
        assert_eq!(
            full.write("trajectory.json", "x").await.unwrap_err().kind(),
            ErrorKind::Unsupported
        );

        let partial = operator(|req| {
            assert_eq!(req.method(), Method::GET);
            assert_eq!(req.headers()["range"], "bytes=2-6");
            let mut reply = response(206, &DOCUMENT[2..7]);
            reply.headers_mut().insert(
                "content-range",
                format!("bytes 2-6/{}", DOCUMENT.len()).parse().unwrap(),
            );
            reply
        });
        assert_eq!(
            partial
                .read_with("trajectory.json")
                .range(2..7)
                .await
                .unwrap()
                .to_bytes(),
            &DOCUMENT.as_bytes()[2..7]
        );
    });
}

#[test]
fn maps_storage_errors_and_rejects_ignored_ranges() {
    futures::executor::block_on(async {
        for (status, body, kind, temporary) in [
            (404, "", ErrorKind::NotFound, false),
            (
                400,
                r#"{"code":"InvalidJWT"}"#,
                ErrorKind::PermissionDenied,
                false,
            ),
            (403, "", ErrorKind::PermissionDenied, false),
            (416, "", ErrorKind::RangeNotSatisfied, false),
            (429, "", ErrorKind::Unexpected, true),
            (500, "", ErrorKind::Unexpected, true),
        ] {
            let error = operator(move |_| response(status, body))
                .read("trajectory.json")
                .await
                .unwrap_err();
            assert_eq!(error.kind(), kind);
            assert_eq!(error.is_temporary(), temporary);
        }
        let error = operator(|_| response(200, DOCUMENT))
            .read_with("trajectory.json")
            .range(2..7)
            .await
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Unsupported);
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
            let error = operator(move |_| {
                let mut reply = response(206, &DOCUMENT[2..7]);
                if let Some(value) = content_range {
                    reply
                        .headers_mut()
                        .insert("content-range", value.parse().unwrap());
                }
                reply
            })
            .read_with("trajectory.json")
            .range(2..7)
            .await
            .unwrap_err();
            assert_eq!(error.kind(), ErrorKind::Unexpected);
        }
        // Open-ended ranges remain valid through the object's end.
        let buffer = operator(|req| {
            assert_eq!(req.headers()["range"], "bytes=2-");
            let mut reply = response(206, &DOCUMENT[2..]);
            reply.headers_mut().insert(
                "content-range",
                format!("bytes 2-{}/{}", DOCUMENT.len() - 1, DOCUMENT.len())
                    .parse()
                    .unwrap(),
            );
            reply
        })
        .read_with("trajectory.json")
        .range(2..)
        .await
        .unwrap();
        assert_eq!(buffer.to_bytes(), &DOCUMENT.as_bytes()[2..]);
    });
}
