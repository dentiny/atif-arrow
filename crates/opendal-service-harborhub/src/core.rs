use http::{HeaderValue, Method, Request};
use opendal_core::raw::{
    build_rooted_abs_path, new_request_build_error, percent_encode_path, ServiceInfo,
};
use opendal_core::{Buffer, Result};

/// Shared configuration for results-bucket HTTP requests.
#[derive(Debug)]
pub(crate) struct HarborHubCore {
    pub info: ServiceInfo,
    pub endpoint: String,
    pub root: String,
    pub publishable_key: HeaderValue,
}

impl HarborHubCore {
    /// Build an anonymous request using the project's public gateway key and encoded object path.
    pub fn object_request(&self, method: Method, path: &str) -> Result<Request<Buffer>> {
        let path = build_rooted_abs_path(&self.root, path);
        let url = format!(
            "{}/storage/v1/object/results/{}",
            self.endpoint,
            percent_encode_path(path.trim_start_matches('/'))
        );
        Request::builder()
            .method(method)
            .uri(url)
            .header("apikey", &self.publishable_key)
            .body(Buffer::new())
            .map_err(new_request_build_error)
    }
}
