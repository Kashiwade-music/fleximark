use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path};

use fleximark_model::{
    Block, BlockKind, DocumentMetadata, DocumentUri, Inline, Node, NodeId, SourceProvenance,
    SourceRange,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const PLUGIN_API_VERSION: u32 = 1;
pub const PLUGIN_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const CONFIG_SCHEMA_VERSION: u32 = 1;
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

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("plugin manifest TOML is invalid: {0}")]
    Syntax(String),
    #[error("plugin manifest schema version {0} is unsupported")]
    SchemaVersion(u32),
    #[error("plugin API version {0} is unsupported")]
    ApiVersion(u32),
    #[error("plugin id is invalid: {0}")]
    PluginId(String),
    #[error("plugin artifact hash must be a lowercase SHA-256 hex digest")]
    ArtifactHash,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hook {
    PreprocessSource,
    TransformDocument,
    TransformBlock,
    ExtendRenderModel,
    UnsafeExportHtml,
}

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
pub struct PreprocessedSource {
    pub text: String,
    pub segments: Vec<EditMapSegment>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditMapSegment {
    pub output_start: u64,
    pub output_end: u64,
    pub origin: EditOrigin,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EditOrigin {
    Original {
        ranges: Vec<SourceRange>,
        primary_range_index: u32,
    },
    Derived {
        ranges: Vec<SourceRange>,
        primary_range_index: u32,
    },
    Generated {
        anchor: Option<SourceRange>,
    },
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlexiMarkConfig {
    pub schema_version: u32,
    #[serde(default)]
    pub notes: NotesConfig,
    #[serde(default)]
    pub assets: AssetsConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub plugins: Vec<ConfiguredPlugin>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotesConfig {
    #[serde(default)]
    pub file_name_prefix: String,
    #[serde(default)]
    pub file_name_suffix: String,
    #[serde(default)]
    pub categories: BTreeMap<String, String>,
    #[serde(default)]
    pub templates: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetsConfig {
    #[serde(default)]
    pub roots: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawHtmlRenderPolicy {
    #[default]
    Sanitize,
    Escape,
    Reject,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityConfig {
    #[serde(default)]
    pub raw_html_preview: RawHtmlRenderPolicy,
    #[serde(default)]
    pub raw_html_export: RawHtmlRenderPolicy,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            raw_html_preview: RawHtmlRenderPolicy::Sanitize,
            raw_html_export: RawHtmlRenderPolicy::Sanitize,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfiguredPlugin {
    pub id: String,
    pub wasm: String,
    pub manifest: String,
    pub signature: String,
    pub manifest_sha256: String,
    pub signer_public_key: String,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub grants: PluginCapabilities,
}

fn enabled() -> bool {
    true
}

impl FlexiMarkConfig {
    pub fn from_toml(source: &str) -> Result<Self, ConfigError> {
        let config: Self =
            toml::from_str(source).map_err(|error| ConfigError::Syntax(error.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ConfigError::Schema(self.schema_version));
        }
        let mut ids = HashSet::new();
        for plugin in &self.plugins {
            if !valid_plugin_id(&plugin.id) {
                return Err(ConfigError::PluginId(plugin.id.clone()));
            }
            if !ids.insert(&plugin.id) {
                return Err(ConfigError::DuplicatePlugin(plugin.id.clone()));
            }
            let path = Path::new(&plugin.wasm);
            if !valid_relative_path(path, &plugin.wasm, "wasm") {
                return Err(ConfigError::PluginPath(plugin.wasm.clone()));
            }
            if !valid_relative_path(Path::new(&plugin.manifest), &plugin.manifest, "toml")
                || !valid_relative_path(Path::new(&plugin.signature), &plugin.signature, "sig")
            {
                return Err(ConfigError::PluginMetadataPath(plugin.id.clone()));
            }
            if !is_sha256(&plugin.manifest_sha256) {
                return Err(ConfigError::ManifestHash(plugin.id.clone()));
            }
            if plugin.signer_public_key.len() != 64
                || !plugin
                    .signer_public_key
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
            {
                return Err(ConfigError::SignerKey(plugin.id.clone()));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("configuration TOML is invalid: {0}")]
    Syntax(String),
    #[error("configuration schema version {0} is unsupported")]
    Schema(u32),
    #[error("plugin is configured more than once: {0}")]
    DuplicatePlugin(String),
    #[error("configured plugin id is invalid: {0}")]
    PluginId(String),
    #[error("plugin WASM path must be relative, traversal-free, and end in .wasm: {0}")]
    PluginPath(String),
    #[error("plugin manifest/signature paths are invalid for: {0}")]
    PluginMetadataPath(String),
    #[error("plugin manifest hash must be lowercase SHA-256 for: {0}")]
    ManifestHash(String),
    #[error("plugin signer key must be a 32-byte hexadecimal Ed25519 key for: {0}")]
    SignerKey(String),
}

fn valid_relative_path(path: &Path, text: &str, extension: &str) -> bool {
    !path.is_absolute()
        && !text.contains(['\\', ':'])
        && path.extension().and_then(|value| value.to_str()) == Some(extension)
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_deny_by_default_and_rejects_old_api() {
        let artifact = "[artifact]\nwasm_sha256='0000000000000000000000000000000000000000000000000000000000000000'\n";
        let manifest = PluginManifest::from_toml(&format!(
            "schema_version=1\n[plugin]\nid='example'\napi_version=1\n{artifact}"
        ))
        .unwrap();
        assert_eq!(manifest.capabilities, PluginCapabilities::default());
        assert_eq!(
            PluginManifest::from_toml(&format!(
                "schema_version=1\n[plugin]\nid='legacy'\napi_version=0\n{artifact}"
            ))
            .unwrap_err(),
            ManifestError::ApiVersion(0)
        );
    }

    #[test]
    fn component_contract_is_versioned_and_has_one_typed_entrypoint() {
        let wit = include_str!("../wit/fleximark-plugin-v1.wit");
        assert!(wit.contains("package fleximark:plugin@1.0.0;"));
        assert!(wit.contains("record invocation"));
        assert!(wit.contains("record response"));
        assert_eq!(wit.matches("invoke: func").count(), 1);
    }

    #[test]
    fn canonical_config_rejects_duplicates_and_traversal() {
        let metadata = "manifest='a.toml'\nsignature='a.sig'\nmanifest_sha256='0000000000000000000000000000000000000000000000000000000000000000'\nsigner_public_key='0000000000000000000000000000000000000000000000000000000000000000'\n";
        let duplicate = format!(
            "schema_version=1\n[[plugins]]\nid='a'\nwasm='a.wasm'\n{metadata}[[plugins]]\nid='a'\nwasm='b.wasm'\n{metadata}"
        );
        assert_eq!(
            FlexiMarkConfig::from_toml(&duplicate).unwrap_err(),
            ConfigError::DuplicatePlugin("a".into())
        );
        let traversal =
            format!("schema_version=1\n[[plugins]]\nid='a'\nwasm='../a.wasm'\n{metadata}");
        assert_eq!(
            FlexiMarkConfig::from_toml(&traversal).unwrap_err(),
            ConfigError::PluginPath("../a.wasm".into())
        );
    }

    #[test]
    fn config_defaults_are_secure_and_unknown_fields_are_rejected() {
        let config = FlexiMarkConfig::from_toml("schema_version = 1\n").unwrap();
        assert_eq!(
            config.security.raw_html_preview,
            RawHtmlRenderPolicy::Escape
        );
        assert_eq!(config.security.raw_html_export, RawHtmlRenderPolicy::Reject);
        assert!(FlexiMarkConfig::from_toml("schema_version = 1\nlegacy = true\n").is_err());
    }
}
