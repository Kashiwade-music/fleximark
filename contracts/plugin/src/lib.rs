//! Role: Current plugin exchange ABI, manifest, and guest bindings.
use std::collections::BTreeMap;

use fleximark_model::{
    Block, BlockKind, DocumentMetadata, DocumentUri, Inline, Node, NodeId, SourceProvenance,
};
use serde::{Deserialize, Serialize};

pub const PLUGIN_API_VERSION: u32 = 2;
fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn valid_plugin_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}
pub const PLUGIN_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const PLUGIN_DIRECTORY: &str = ".fleximark/plugins";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub schema_version: u32,
    pub plugin: PluginMetadata,
    pub artifact: PluginArtifact,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginArtifact {
    pub wasm_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginMetadata {
    pub id: String,
    pub api_version: u32,
    #[serde(default)]
    pub required: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginCapabilities {
    #[serde(default)]
    pub read_workspace: bool,
    #[serde(default)]
    pub write_workspace: bool,
    #[serde(default)]
    pub environment: bool,
    #[serde(default)]
    pub unsafe_html_output: bool,
}

impl PluginManifest {
    pub fn from_toml(source: &str) -> Result<Self, ManifestError> {
        let manifest: Self =
            toml::from_str(source).map_err(|error| ManifestError::Syntax(error.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.schema_version != PLUGIN_MANIFEST_SCHEMA_VERSION {
            return Err(ManifestError::SchemaVersion(self.schema_version));
        }
        if self.plugin.api_version != PLUGIN_API_VERSION {
            return Err(ManifestError::ApiVersion(self.plugin.api_version));
        }
        if !valid_plugin_id(&self.plugin.id) {
            return Err(ManifestError::PluginId(self.plugin.id.clone()));
        }
        if !is_sha256(&self.artifact.wasm_sha256) {
            return Err(ManifestError::ArtifactHash);
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ManifestError {
    Syntax(String),
    SchemaVersion(u32),
    ApiVersion(u32),
    PluginId(String),
    ArtifactHash,
}

pub use fleximark_plugin_api::{EditMapSegment, EditOrigin, Hook, PreprocessedSource};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CreationKey(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CandidateIdentity {
    Existing { id: NodeId },
    Created { key: CreationKey },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateDocument {
    pub schema_version: u32,
    pub document_version: u64,
    pub uri: DocumentUri,
    pub metadata: DocumentMetadata,
    pub blocks: Vec<CandidateBlock>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateBlock {
    pub identity: CandidateIdentity,
    pub provenance: SourceProvenance,
    pub kind: BlockKind,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<CandidateNode>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CandidateNode {
    Block(CandidateBlock),
    Inline(Inline),
}

impl CandidateDocument {
    pub fn from_document(document: &fleximark_model::Document) -> Self {
        Self {
            schema_version: document.schema_version,
            document_version: document.document_version,
            uri: document.uri.clone(),
            metadata: document.metadata.clone(),
            blocks: document.blocks.iter().map(candidate_from_block).collect(),
        }
    }
}

fn candidate_from_block(block: &Block) -> CandidateBlock {
    CandidateBlock {
        identity: CandidateIdentity::Existing {
            id: block.id.clone(),
        },
        provenance: block.provenance.clone(),
        kind: block.kind.clone(),
        attributes: block.attributes.clone(),
        children: block
            .children
            .iter()
            .map(|node| match node {
                Node::Block(block) => CandidateNode::Block(candidate_from_block(block)),
                Node::Inline(inline) => CandidateNode::Inline(inline.clone()),
            })
            .collect(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HookRequest {
    PreprocessSource {
        document_version: u64,
        text: String,
        edit_map: Vec<EditMapSegment>,
    },
    TransformDocument {
        document: fleximark_model::Document,
    },
    TransformBlock {
        document_version: u64,
        block: Block,
    },
    ExtendRenderModel {
        document: fleximark_model::Document,
        target: String,
    },
    UnsafeExportHtml {
        document_version: u64,
        html: String,
    },
}

impl HookRequest {
    pub fn hook(&self) -> Hook {
        match self {
            Self::PreprocessSource { .. } => Hook::PreprocessSource,
            Self::TransformDocument { .. } => Hook::TransformDocument,
            Self::TransformBlock { .. } => Hook::TransformBlock,
            Self::ExtendRenderModel { .. } => Hook::ExtendRenderModel,
            Self::UnsafeExportHtml { .. } => Hook::UnsafeExportHtml,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HookResponse {
    PreprocessedSource {
        candidate: PreprocessedSource,
    },
    Document {
        candidate: CandidateDocument,
    },
    Block {
        candidate: CandidateBlock,
    },
    RenderAnnotations {
        annotations: BTreeMap<String, String>,
    },
    UnsafeExportHtml {
        html: String,
    },
}

#[cfg(target_arch = "wasm32")]
pub mod component {
    //! Role: Expose generated guest bindings and typed hook exchange.
    wit_bindgen::generate!({
        path: "wit",
        world: "fleximark-plugin",
        pub_export_macro: true,
    });

    pub fn invoke(
        request: exports::fleximark::plugin::hooks::Invocation,
        handler: impl FnOnce(super::HookRequest) -> Result<super::HookResponse, String>,
    ) -> Result<exports::fleximark::plugin::hooks::Response, String> {
        if request.api_version != super::PLUGIN_API_VERSION {
            return Err(format!(
                "unsupported plugin API version {}",
                request.api_version
            ));
        }
        let hook_request: super::HookRequest = serde_json::from_str(&request.request_json)
            .map_err(|error| format!("invalid hook request JSON: {error}"))?;
        let expected_hook = serde_json::to_value(hook_request.hook())
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or_else(|| "hook name is not a string".to_owned())?;
        if expected_hook != request.hook {
            return Err("hook discriminator does not match request".to_owned());
        }
        let response = handler(hook_request)?;
        Ok(exports::fleximark::plugin::hooks::Response {
            api_version: super::PLUGIN_API_VERSION,
            response_json: serde_json::to_string(&response)
                .map_err(|error| format!("hook response is not serializable: {error}"))?,
        })
    }
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syntax(value) => write!(f, "plugin manifest TOML is invalid: {0}", value),
            Self::SchemaVersion(value) => write!(
                f,
                "plugin manifest schema version {0} is unsupported",
                value
            ),
            Self::ApiVersion(value) => write!(f, "plugin API version {0} is unsupported", value),
            Self::PluginId(value) => write!(f, "plugin id is invalid: {0}", value),
            Self::ArtifactHash => {
                f.write_str("plugin artifact hash must be a lowercase SHA-256 hex digest")
            }
        }
    }
}
impl std::error::Error for ManifestError {}

#[cfg(test)]
mod tests {
    //! Role: Verify invariants and rejection of invalid inputs.
    use super::*;

    #[test]
    fn manifest_is_deny_by_default_and_rejects_old_api() {
        let artifact = "[artifact]\nwasm_sha256='0000000000000000000000000000000000000000000000000000000000000000'\n";
        let manifest = PluginManifest::from_toml(&format!(
            "schema_version=1\n[plugin]\nid='example'\napi_version={PLUGIN_API_VERSION}\n{artifact}"
        ))
        .unwrap();
        assert_eq!(manifest.capabilities, PluginCapabilities::default());
        assert_eq!(
            PluginManifest::from_toml(&format!(
                "schema_version=1\n[plugin]\nid='legacy'\napi_version=1\n{artifact}"
            ))
            .unwrap_err(),
            ManifestError::ApiVersion(1)
        );
    }

    #[test]
    fn component_contract_is_versioned_and_has_one_typed_entrypoint() {
        let wit = include_str!("../wit/fleximark-plugin-v2.wit");
        assert!(wit.contains("package fleximark:plugin@2.0.0;"));
        assert!(wit.contains("record invocation"));
        assert!(wit.contains("record response"));
        assert_eq!(wit.matches("invoke: func").count(), 1);
    }
}
