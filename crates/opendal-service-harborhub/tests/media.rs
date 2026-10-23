use std::sync::{Arc, Mutex};

use arrow_array::{Array, LargeBinaryArray, ListArray, StringArray, StructArray};
use atif_io::{InputFormat, MediaOptions, MediaResolver, TrajectoryReader};
use futures::{channel::oneshot, FutureExt};
use http::{Method, Request, Response};
use opendal_core::{
    Buffer, HttpBody, HttpTransport, HttpTransporter, OperationContext, Operator, Result,
};
use opendal_service_harborhub::HarborHub;

const DOCUMENT: &str = r#"{"schema_version":"ATIF-v1.8","agent":{"name":"test","version":"1"},"steps":[{"step_id":1,"source":"user","message":[{"type":"image","source":{"media_type":"image/png","path":"images/one.png"}}]}]}"#;
const PREFIX: &str = "/storage/v1/object/results/trials/test/agent/";

/// Uses the actual Harbor backend with in-process HTTP responses. The gate lets
/// a media GET remain pending while the caller cancels and resumes next_batch.
struct Transport {
    gate: Arc<Mutex<Option<oneshot::Receiver<()>>>>,
    requests: Arc<Mutex<Vec<String>>>,
    reported_size: usize,
}

impl HttpTransport for Transport {
    async fn fetch(&self, request: Request<Buffer>) -> Result<Response<HttpBody>> {
        assert_eq!(request.headers()["apikey"], "test-publishable-key");
        assert!(request.headers().get("authorization").is_none());
        assert!(request.uri().query().is_none());
        let name = request.uri().path().strip_prefix(PREFIX).unwrap();
        self.requests
            .lock()
            .unwrap()
            .push(format!("{} {name}", request.method()));
        let data = match name {
            "trajectory.json" => DOCUMENT.as_bytes(),
            "images/one.png" => b"\0png\xff".as_slice(),
            other => panic!("unexpected object: {other}"),
        };
        let length = if name == "images/one.png" {
            self.reported_size
        } else {
            data.len()
        };
        let body = if request.method() == Method::HEAD {
            &b""[..]
        } else {
            assert_eq!(request.method(), Method::GET);
            if name == "images/one.png" {
                let gate = self.gate.lock().unwrap().take();
                if let Some(gate) = gate {
                    gate.await.unwrap();
                }
            }
            data
        };
        let chunks = [
            Ok(Buffer::from(body[..body.len() / 2].to_vec())),
            Ok(Buffer::from(body[body.len() / 2..].to_vec())),
        ];
        Ok(Response::builder()
            .status(200)
            .header("content-length", length)
            .body(HttpBody::new(futures::stream::iter(chunks), None))
            .unwrap())
    }
}

fn operator(
    gate: Option<oneshot::Receiver<()>>,
    reported_size: usize,
) -> (Operator, Arc<Mutex<Vec<String>>>) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let transport = Transport {
        gate: Arc::new(Mutex::new(gate)),
        requests: requests.clone(),
        reported_size,
    };
    let operator = Operator::new(
        HarborHub::default()
            .endpoint("https://hub.example.com")
            .publishable_key("test-publishable-key")
            .root("trials/test"),
    )
    .unwrap()
    .with_context(OperationContext::new().with_http_transport(HttpTransporter::new(transport)));
    (operator, requests)
}

#[test]
fn reads_hub_media_and_resumes_a_cancelled_request_without_dropping_or_repeating_it() {
    futures::executor::block_on(async {
        let (sender, receiver) = oneshot::channel();
        let (operator, requests) = operator(Some(receiver), 5);
        let resolver =
            MediaResolver::new(&operator, "agent/trajectory.json", MediaOptions::default())
                .unwrap();
        let mut reader = TrajectoryReader::open(
            &operator,
            "agent/trajectory.json",
            InputFormat::Json,
            1,
            Some("harbor://display/trial".into()),
        )
        .await
        .unwrap()
        .with_media(resolver);
        assert!(reader.next_batch().now_or_never().is_none());
        assert!(requests
            .lock()
            .unwrap()
            .iter()
            .any(|request| request == "GET images/one.png"));
        sender.send(()).unwrap();
        let batch = reader.next_batch().await.unwrap().unwrap();
        let list = batch
            .column_by_name("media")
            .unwrap()
            .as_any()
            .downcast_ref::<ListArray>()
            .unwrap();
        let value = list.value(0);
        let media = value.as_any().downcast_ref::<StructArray>().unwrap();
        let payload = media
            .column_by_name("payload")
            .unwrap()
            .as_any()
            .downcast_ref::<LargeBinaryArray>()
            .unwrap();
        assert_eq!(payload.value(0), b"\0png\xff");
        let uri = media
            .column_by_name("resolved_uri")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(uri.value(0), "agent/images/one.png");
        assert_eq!(
            requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| *request == "GET images/one.png")
                .count(),
            1
        );
        assert!(reader.next_batch().await.unwrap().is_none());
    });
}

#[test]
fn enforces_byte_limits_while_streaming_even_if_stat_underreports_the_size() {
    futures::executor::block_on(async {
        let (operator, _) = operator(None, 2);
        let resolver = MediaResolver::new(
            &operator,
            "agent/trajectory.json",
            MediaOptions {
                max_media_bytes: 3,
                ..MediaOptions::default()
            },
        )
        .unwrap();
        let mut reader = TrajectoryReader::open(
            &operator,
            "agent/trajectory.json",
            InputFormat::Json,
            1,
            None,
        )
        .await
        .unwrap()
        .with_media(resolver);
        let error = reader.next_batch().await.unwrap_err();
        assert!(error.to_string().contains("/steps/0/message/0"));
        assert!(
            error.to_string().contains("byte limit")
                || error.to_string().contains("content length"),
            "{error}"
        );
        assert!(reader.next_batch().await.unwrap().is_none());
    });
}
