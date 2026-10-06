use std::sync::Arc;

use http::HeaderValue;
use opendal_core::raw::*;
use opendal_core::*;

use crate::config::HarborHubConfig;
use crate::core::HarborHubCore;
use crate::error::parse_error;
use crate::reader::HarborHubReader;

/// Read-only OpenDAL builder for Harbor Hub's `results` bucket.
#[derive(Debug, Default)]
pub struct HarborHub {
    pub(crate) config: HarborHubConfig,
}

impl HarborHub {
    /// Set the Supabase project URL, not the Hub website URL.
    pub fn endpoint(mut self, endpoint: &str) -> Self {
        self.config.endpoint = Some(endpoint.into());
        self
    }

    /// Restrict object paths to this prefix within the results bucket.
    pub fn root(mut self, root: &str) -> Self {
        self.config.root = Some(root.into());
        self
    }

    /// Set the publishable key for a custom Supabase project.
    pub fn publishable_key(mut self, key: &str) -> Self {
        self.config.publishable_key = Some(key.into());
        self
    }
}

impl Builder for HarborHub {
    type Config = HarborHubConfig;

    fn build(self) -> Result<impl Service> {
        let endpoint = self
            .config
            .endpoint
            .as_deref()
            .unwrap_or("https://ofhuhcpkvzjlejydnvyd.supabase.co");
        let url = url::Url::parse(endpoint).map_err(|source| {
            Error::new(ErrorKind::ConfigInvalid, "invalid Supabase endpoint").set_source(source)
        })?;
        if !(url.scheme() == "https" || url.scheme() == "http")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::new(
                ErrorKind::ConfigInvalid,
                "endpoint must use HTTP or HTTPS without credentials, query, or fragment",
            ));
        }
        let publishable_key = self
            .config
            .publishable_key
            .as_deref()
            .unwrap_or("sb_publishable_Z-vuQbpvpG-PStjbh4yE0Q_e-d3MTIH");
        if publishable_key.is_empty() {
            return Err(Error::new(
                ErrorKind::ConfigInvalid,
                "publishable_key is empty",
            ));
        }
        let publishable_key = HeaderValue::from_str(publishable_key).map_err(|source| {
            Error::new(ErrorKind::ConfigInvalid, "invalid publishable_key").set_source(source)
        })?;
        let root = normalize_root(self.config.root.as_deref().unwrap_or_default());
        Ok(HarborHubBackend {
            core: Arc::new(HarborHubCore {
                info: ServiceInfo::new("harborhub", &root, "results"),
                endpoint: url.as_str().trim_end_matches('/').into(),
                root,
                publishable_key,
            }),
        })
    }
}

/// OpenDAL service implementation for streaming result reads.
#[derive(Debug)]
struct HarborHubBackend {
    core: Arc<HarborHubCore>,
}

impl Service for HarborHubBackend {
    type Reader = oio::StreamReader<HarborHubReader>;
    type Writer = ();
    type Lister = ();
    type Deleter = ();
    type Copier = ();
    type Composer = ();

    fn info(&self) -> ServiceInfo {
        self.core.info.clone()
    }

    fn capability(&self) -> Capability {
        Capability {
            read: true,
            stat: true,
            shared: true,
            ..Default::default()
        }
    }

    fn read(&self, ctx: &OperationContext, path: &str, _args: OpRead) -> Result<Self::Reader> {
        Ok(oio::StreamReader::new(HarborHubReader {
            core: self.core.clone(),
            ctx: ctx.clone(),
            path: path.into(),
        }))
    }

    async fn stat(&self, ctx: &OperationContext, path: &str, _args: OpStat) -> Result<RpStat> {
        if path == "/" {
            return Ok(RpStat::new(MetadataBuilder::dir().build()));
        }
        let req = self.core.object_request(http::Method::HEAD, path)?;
        let resp = ctx.http_transport().send(req).await?;
        if resp.status() == http::StatusCode::OK {
            parse_into_metadata(path, resp.headers()).map(RpStat::new)
        } else {
            Err(parse_error(resp))
        }
    }

    async fn create_dir(
        &self,
        _: &OperationContext,
        _: &str,
        _: OpCreateDir,
    ) -> Result<RpCreateDir> {
        Err(Error::new(
            ErrorKind::Unsupported,
            "Harbor Hub is read-only",
        ))
    }

    fn write(&self, _: &OperationContext, _: &str, _: OpWrite) -> Result<Self::Writer> {
        Err(Error::new(
            ErrorKind::Unsupported,
            "Harbor Hub is read-only",
        ))
    }

    fn list(&self, _: &OperationContext, _: &str, _: OpList) -> Result<Self::Lister> {
        Err(Error::new(
            ErrorKind::Unsupported,
            "Harbor Hub listing is not implemented",
        ))
    }

    fn delete(&self, _: &OperationContext) -> Result<Self::Deleter> {
        Err(Error::new(
            ErrorKind::Unsupported,
            "Harbor Hub is read-only",
        ))
    }

    fn copy(&self, _: &OperationContext, _: &str, _: &str, _: OpCopy) -> Result<Self::Copier> {
        Err(Error::new(
            ErrorKind::Unsupported,
            "Harbor Hub is read-only",
        ))
    }

    async fn rename(
        &self,
        _: &OperationContext,
        _: &str,
        _: &str,
        _: OpRename,
    ) -> Result<RpRename> {
        Err(Error::new(
            ErrorKind::Unsupported,
            "Harbor Hub is read-only",
        ))
    }

    async fn presign(&self, _: &OperationContext, _: &str, _: OpPresign) -> Result<RpPresign> {
        Err(Error::new(
            ErrorKind::Unsupported,
            "Harbor Hub presigning is not implemented",
        ))
    }
}
