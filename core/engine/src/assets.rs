//! Role: Validate resolved render assets, styles, and immutable configuration.
use std::collections::{BTreeMap, HashSet};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use fleximark_model::{AssetRef, Sha256Hex};
use fleximark_render_html::RenderContext;
use serde::{Deserialize, Serialize};

use crate::error::EngineError;
use crate::identity::{content_hash, content_hash_bytes};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
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
            renderer_version: "html-v1".to_owned(),
            sanitizer_version: "builtin-sanitizer-v2".to_owned(),
            assets: Vec::new(),
            asset_diagnostics: Vec::new(),
        }
    }
}

impl RenderConfig {
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
        let mut sources = HashSet::new();
        let mut publications = BTreeMap::<String, RenderAsset>::new();
        let mut resources = BTreeMap::new();
        let mut unique_assets = Vec::new();
        for asset in assets {
            asset.validate()?;
            if !sources.insert(asset.source.clone()) {
                return Err(EngineError::Asset(
                    "resolved asset sources must be unique".to_owned(),
                ));
            }
            resources.insert(asset.source.clone(), asset.published.reference.clone());
            if let Some(existing) = publications.get(&asset.published.reference) {
                if existing != &asset.published {
                    return Err(EngineError::Asset(
                        "resolved assets reuse a reference for different content".to_owned(),
                    ));
                }
                continue;
            }
            publications.insert(asset.published.reference.clone(), asset.published.clone());
            unique_assets.push(asset);
        }
        self.context.resolved_resources = resources;
        self.assets = unique_assets;
        self.asset_diagnostics = diagnostics;
        Ok(self)
    }

    pub fn assets(&self) -> impl ExactSizeIterator<Item = &RenderAsset> {
        self.assets.iter().map(|asset| &asset.published)
    }

    pub fn asset_diagnostics(&self) -> &[AssetDiagnostic] {
        &self.asset_diagnostics
    }

    pub fn validate(&self) -> Result<(), EngineError> {
        Sha256Hex::try_from(self.config_hash.as_str()).map_err(|_| {
            EngineError::Configuration("render config contains a non-canonical hash".to_owned())
        })?;
        if self
            .style
            .as_ref()
            .is_some_and(|style| style.fingerprint != content_hash(&style.css))
        {
            return Err(EngineError::Configuration(
                "render style fingerprint does not match its CSS content".to_owned(),
            ));
        }
        if self.renderer_version.is_empty() || self.sanitizer_version.is_empty() {
            return Err(EngineError::Configuration(
                "renderer and sanitizer versions are required".to_owned(),
            ));
        }
        let published_references = self
            .assets
            .iter()
            .map(|asset| asset.published.reference.as_str())
            .collect::<HashSet<_>>();
        if self.context.resolved_resources.is_empty() != self.assets.is_empty()
            || self
                .context
                .resolved_resources
                .iter()
                .any(|(source, reference)| {
                    source.is_empty() || !published_references.contains(reference.as_str())
                })
        {
            return Err(EngineError::Asset(
                "renderer resource map does not match the resolved assets".to_owned(),
            ));
        }
        for asset in &self.assets {
            asset.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderAsset {
    pub reference: String,
    pub media_type: String,
    pub content_hash: String,
    pub byte_length: u64,
    pub data: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedRenderAsset {
    source: String,
    pub(super) published: RenderAsset,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetDiagnostic {
    pub source: String,
    pub kind: AssetDiagnosticKind,
    pub message: String,
    pub source_range: Option<fleximark_model::SourceRange>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssetDiagnosticKind {
    InvalidReference,
    Missing,
    OutsideRoot,
    Symlink,
    NotAFile,
    Unreadable,
    Unsupported,
}

impl AssetDiagnosticKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidReference => "asset-invalid-reference",
            Self::Missing => "asset-missing",
            Self::OutsideRoot => "asset-outside-root",
            Self::Symlink => "asset-symlink",
            Self::NotAFile => "asset-not-a-file",
            Self::Unreadable => "asset-unreadable",
            Self::Unsupported => "asset-unsupported",
        }
    }
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
        if source.is_empty() {
            return Err(EngineError::Asset(
                "resolved asset source must be non-empty".to_owned(),
            ));
        }
        let content_hash = content_hash_bytes(bytes);
        let hash = Sha256Hex::try_from(content_hash.as_str())
            .map_err(|error| EngineError::Asset(error.to_string()))?;
        let reference = AssetRef::from_hash(hash).to_string();
        let asset = Self {
            source,
            published: RenderAsset {
                reference,
                media_type,
                content_hash,
                byte_length: (bytes.len() as u64),
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
        let hash = Sha256Hex::try_from(self.published.content_hash.as_str())
            .map_err(|error| EngineError::Asset(error.to_string()))?;
        let reference = AssetRef::try_from(self.published.reference.as_str())
            .map_err(|error| EngineError::Asset(error.to_string()))?;
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
            || decoded.len() as u64 != self.published.byte_length
            || content_hash_bytes(&decoded) != self.published.content_hash
            || reference != AssetRef::from_hash(hash)
        {
            return Err(EngineError::Asset(
                "resolved asset metadata does not match its content".to_owned(),
            ));
        }
        Ok(())
    }
}
