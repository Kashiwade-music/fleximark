use std::collections::{BTreeMap, HashSet};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use fleximark_plugin_host::PluginHost;
use fleximark_render_html::RenderContext;
use fleximark_wire::JsSafeU64;
use serde::{Deserialize, Serialize};

use crate::error::EngineError;
use crate::identity::{content_hash, content_hash_bytes};

pub(super) const MAX_RENDER_ASSET_BYTES: usize = 1024 * 1024;
const MAX_RENDER_ASSETS_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct RenderStyle {
    css: String,
    fingerprint: String,
}

impl RenderStyle {
    pub fn from_validated_css(css: String) -> Self {
        let fingerprint = content_hash(&css);
        Self { css, fingerprint }
    }

    pub fn css(&self) -> &str {
        &self.css
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

impl<'de> Deserialize<'de> for RenderStyle {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct WireStyle {
            css: String,
            fingerprint: String,
        }

        let wire = WireStyle::deserialize(deserializer)?;
        let style = Self::from_validated_css(wire.css);
        if style.fingerprint != wire.fingerprint {
            return Err(serde::de::Error::custom(
                "render style fingerprint does not match its CSS content",
            ));
        }
        Ok(style)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderConfig {
    pub context: RenderContext,
    pub style: Option<RenderStyle>,
    pub config_hash: String,
    pub config_generation: u64,
    pub plugin_set_hash: String,
    pub plugin_generation: u64,
    pub renderer_version: String,
    pub sanitizer_version: String,
    pub(super) assets: Vec<ResolvedRenderAsset>,
    pub(super) asset_diagnostics: Vec<AssetDiagnostic>,
}

impl Default for RenderConfig {
    fn default() -> Self {
        let empty = content_hash("");
        Self {
            context: RenderContext::default(),
            style: None,
            config_hash: empty.clone(),
            config_generation: 0,
            plugin_set_hash: empty,
            plugin_generation: 0,
            renderer_version: "html-v1".to_owned(),
            sanitizer_version: "builtin-sanitizer-v1".to_owned(),
            assets: Vec::new(),
            asset_diagnostics: Vec::new(),
        }
    }
}

impl RenderConfig {
    pub fn for_plugins(
        context: RenderContext,
        style: Option<RenderStyle>,
        plugins: &PluginHost,
    ) -> Self {
        Self {
            context,
            style,
            config_hash: plugins.config_hash().to_owned(),
            config_generation: plugins.generation(),
            plugin_set_hash: plugins.plugin_set_hash(),
            plugin_generation: plugins.generation(),
            renderer_version: "html-v1".to_owned(),
            sanitizer_version: "builtin-sanitizer-v1".to_owned(),
            assets: Vec::new(),
            asset_diagnostics: Vec::new(),
        }
    }

    /// Installs assets that were resolved and read by the trusted service boundary.
    pub fn with_resolved_assets(
        mut self,
        resolved: impl Into<ResolvedAssets>,
    ) -> Result<Self, EngineError> {
        let resolved = resolved.into();
        let ResolvedAssets {
            assets,
            diagnostics,
        } = resolved;
        let total = assets.iter().try_fold(0_usize, |total, asset| {
            total
                .checked_add(asset.published.byte_length.get() as usize)
                .filter(|total| *total <= MAX_RENDER_ASSETS_BYTES)
                .ok_or_else(|| EngineError::Asset("resolved assets exceed 8 MiB".to_owned()))
        })?;
        let mut sources = HashSet::new();
        let mut references = HashSet::new();
        for asset in &assets {
            asset.validate()?;
            if !sources.insert(asset.source.clone())
                || !references.insert(asset.published.reference.clone())
            {
                return Err(EngineError::Asset(
                    "resolved asset sources and references must be unique".to_owned(),
                ));
            }
        }
        debug_assert!(total <= MAX_RENDER_ASSETS_BYTES);
        self.context.resolved_resources = assets
            .iter()
            .map(|asset| (asset.source.clone(), asset.published.reference.clone()))
            .collect();
        self.assets = assets;
        self.asset_diagnostics = diagnostics;
        Ok(self)
    }

    pub fn assets(&self) -> impl ExactSizeIterator<Item = &RenderAsset> {
        self.assets.iter().map(|asset| &asset.published)
    }

    pub fn asset_diagnostics(&self) -> &[AssetDiagnostic] {
        &self.asset_diagnostics
    }

    pub fn validate_for(&self, plugins: &PluginHost) -> Result<(), EngineError> {
        if self.config_hash != plugins.config_hash()
            || self.config_generation != plugins.generation()
            || self.plugin_set_hash != plugins.plugin_set_hash()
            || self.plugin_generation != plugins.generation()
        {
            return Err(EngineError::Plugin(
                "render config and plugin set generations do not match".to_owned(),
            ));
        }
        for hash in [&self.config_hash, &self.plugin_set_hash] {
            if hash.len() != 64
                || !hash
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
            {
                return Err(EngineError::Plugin(
                    "render config contains a non-canonical hash".to_owned(),
                ));
            }
        }
        if self
            .style
            .as_ref()
            .is_some_and(|style| style.fingerprint != content_hash(&style.css))
        {
            return Err(EngineError::Plugin(
                "render style fingerprint does not match its CSS content".to_owned(),
            ));
        }
        if self.renderer_version.is_empty() || self.sanitizer_version.is_empty() {
            return Err(EngineError::Plugin(
                "renderer and sanitizer versions are required".to_owned(),
            ));
        }
        let expected_resources = self
            .assets
            .iter()
            .map(|asset| (asset.source.clone(), asset.published.reference.clone()))
            .collect::<BTreeMap<_, _>>();
        if self.context.resolved_resources != expected_resources {
            return Err(EngineError::Asset(
                "renderer resource map was not produced by resolved assets".to_owned(),
            ));
        }
        for asset in &self.assets {
            asset.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderAsset {
    pub reference: String,
    pub media_type: String,
    pub content_hash: String,
    pub byte_length: JsSafeU64,
    pub data: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedRenderAsset {
    source: String,
    pub(super) published: RenderAsset,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetDiagnostic {
    pub source: String,
    pub message: String,
    pub source_range: Option<fleximark_model::SourceRange>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolvedAssets {
    pub assets: Vec<ResolvedRenderAsset>,
    pub diagnostics: Vec<AssetDiagnostic>,
}

impl From<Vec<ResolvedRenderAsset>> for ResolvedAssets {
    fn from(assets: Vec<ResolvedRenderAsset>) -> Self {
        Self {
            assets,
            diagnostics: Vec::new(),
        }
    }
}

impl ResolvedRenderAsset {
    pub fn from_validated_bytes(
        source: String,
        media_type: String,
        bytes: &[u8],
    ) -> Result<Self, EngineError> {
        if source.is_empty() || bytes.len() > MAX_RENDER_ASSET_BYTES {
            return Err(EngineError::Asset(
                "resolved asset source must be non-empty and content must not exceed 1 MiB"
                    .to_owned(),
            ));
        }
        let content_hash = content_hash_bytes(bytes);
        let asset = Self {
            source,
            published: RenderAsset {
                reference: format!("fleximark-asset:{content_hash}"),
                media_type,
                content_hash,
                byte_length: JsSafeU64::new(bytes.len() as u64)
                    .expect("asset size limit is JavaScript-safe"),
                data: BASE64.encode(bytes),
            },
        };
        asset.validate()?;
        Ok(asset)
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn published(&self) -> &RenderAsset {
        &self.published
    }

    fn validate(&self) -> Result<(), EngineError> {
        let media_type_valid = !self.published.media_type.is_empty()
            && self.published.media_type.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'/' | b'-' | b'+' | b'.')
            });
        let decoded = BASE64
            .decode(&self.published.data)
            .map_err(|_| EngineError::Asset("asset data is not canonical base64".to_owned()))?;
        if !media_type_valid
            || decoded.len() != self.published.byte_length.get() as usize
            || decoded.len() > MAX_RENDER_ASSET_BYTES
            || content_hash_bytes(&decoded) != self.published.content_hash
            || self.published.reference
                != format!("fleximark-asset:{}", self.published.content_hash)
        {
            return Err(EngineError::Asset(
                "resolved asset metadata does not match its content".to_owned(),
            ));
        }
        Ok(())
    }
}
