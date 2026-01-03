#[derive(Clone, Copy, Debug)]
pub struct UpstreamRoute(pub &'static str);

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestAttribution {
    pub provider: String,
    pub resolved_model: Option<String>,
    pub account_id: Option<String>,
    pub account_email_masked: Option<String>,
}
