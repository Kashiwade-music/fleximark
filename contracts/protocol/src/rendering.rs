//! Role: Define external render and navigation payloads.
use fleximark_wire::JsSafeU64;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct NodeId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct PreviewSessionId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum PositionEncoding {
    Utf8,
    Utf16,
    Utf32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourcePosition {
    pub line: JsSafeU64,
    pub character: JsSafeU64,
    pub encoding: PositionEncoding,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceRange {
    pub byte_start: JsSafeU64,
    pub byte_end: JsSafeU64,
    pub start: SourcePosition,
    pub end: SourcePosition,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NavigationEntry {
    #[cfg_attr(feature = "contract-schema", schemars(length(min = 1)))]
    pub node_id: NodeId,
    pub source_range: SourceRange,
    pub depth: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderStyle {
    pub css: String,
    #[cfg_attr(
        feature = "contract-schema",
        schemars(regex(pattern = "^[0-9a-f]{64}$"))
    )]
    pub fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderAsset {
    #[cfg_attr(
        feature = "contract-schema",
        schemars(regex(pattern = "^fleximark-asset:[0-9a-f]{64}$"))
    )]
    pub reference: String,
    #[cfg_attr(feature = "contract-schema", schemars(extend("enum" = ["image/png", "image/jpeg", "image/gif", "image/webp", "image/avif", "audio/mpeg", "audio/ogg", "audio/wav"])))]
    pub media_type: String,
    #[cfg_attr(
        feature = "contract-schema",
        schemars(regex(pattern = "^[0-9a-f]{64}$"))
    )]
    pub content_hash: String,
    pub byte_length: JsSafeU64,
    #[cfg_attr(feature = "contract-schema", schemars(extend("contentEncoding" = "base64")))]
    pub data: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderBlock {
    pub id: NodeId,
    pub html: String,
    #[cfg_attr(feature = "contract-schema", schemars(extend("minItems" = 1, "uniqueItems" = true, "items" = {"type":"string","minLength":1})))]
    pub node_ids: Vec<NodeId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderFrame {
    pub preview_session_id: PreviewSessionId,
    pub document_version: JsSafeU64,
    #[cfg_attr(feature = "contract-schema", schemars(extend("minimum" = 1)))]
    pub render_revision: JsSafeU64,
    pub renderer_fingerprint: String,
    pub style: Option<RenderStyle>,
    pub assets: Vec<RenderAsset>,
    pub blocks: Vec<RenderBlock>,
    pub navigation: Vec<NavigationEntry>,
}
