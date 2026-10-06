use opendal_core::Configurator;
use serde::{Deserialize, Serialize};

use crate::HarborHub;

/// Configuration for reading objects in Harbor Hub's `results` bucket.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct HarborHubConfig {
    /// Supabase project URL; defaults to the public Harbor Hub project.
    pub endpoint: Option<String>,
    /// Object prefix within the results bucket; defaults to `/`.
    pub root: Option<String>,
    /// Supabase publishable key; defaults to the public Harbor Hub key.
    pub publishable_key: Option<String>,
}

impl Configurator for HarborHubConfig {
    type Builder = HarborHub;

    fn into_builder(self) -> Self::Builder {
        HarborHub { config: self }
    }
}
