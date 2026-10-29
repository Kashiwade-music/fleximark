//! Role: Define preview values shared by command and document contracts.
use crate::rendering::RenderFrame;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum PreviewTarget {
    EmbeddedHtml,
    ExternalBrowser,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreatePreviewResult {
    pub frame: RenderFrame,
    pub preview_session_id: String,
    #[cfg_attr(feature = "contract-schema", schemars(transform = crate::schema::non_null))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}
