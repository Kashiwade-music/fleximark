use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use fleximark_model::{
    AnchorAffinity, Block, BlockKind, Document, DocumentUri, GeneratedAnchor, Inline, InlineKind,
    NavigationEntry, Node, NodeId, PositionEncoding, SourcePosition, SourceProvenance, TransformId,
};
use fleximark_parser::{ParseError, parse};
use fleximark_plugin_host::{
    CancellationToken, PluginDiagnostic, PluginHost, PluginRun, UnsafeExportOutput,
};
use fleximark_plugin_sdk::{EditOrigin, PreprocessedSource};
use fleximark_render_html::{
    HtmlRenderer, HtmlTarget, RawHtmlPolicy, RenderContext, RenderError, RenderedBlock,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

const ROOT_NODE_ID: &str = "document-root";
const MAX_RENDER_ASSET_BYTES: usize = 1024 * 1024;
const MAX_RENDER_ASSETS_BYTES: usize = 8 * 1024 * 1024;
static SESSION_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocumentSessionId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PreviewSessionId(pub String);

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
    pub plugin_set_hash: String,
    pub plugin_generation: u64,
    pub renderer_version: String,
    pub sanitizer_version: String,
    assets: Vec<ResolvedRenderAsset>,
    asset_diagnostics: Vec<AssetDiagnostic>,
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
                .checked_add(asset.published.byte_length as usize)
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
    published: RenderAsset,
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
                byte_length: bytes.len() as u64,
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
            || decoded.len() != self.published.byte_length as usize
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderSnapshot {
    pub preview_session_id: PreviewSessionId,
    pub document_version: u64,
    pub result_render_revision: u64,
    pub renderer_fingerprint: String,
    pub style: Option<RenderStyle>,
    pub assets: Vec<RenderAsset>,
    pub node_ids: Vec<NodeId>,
    pub navigation: Vec<NavigationEntry>,
    pub html: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderPatch {
    pub preview_session_id: PreviewSessionId,
    pub document_version: u64,
    pub base_render_revision: u64,
    pub result_render_revision: u64,
    pub base_renderer_fingerprint: String,
    pub result_renderer_fingerprint: String,
    pub style: Option<RenderStyle>,
    pub navigation: Vec<NavigationEntry>,
    pub operations: Vec<PatchOperation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchPrecondition {
    pub node_exists: bool,
    pub current_parent_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PatchOperation {
    Insert {
        node_id: NodeId,
        parent_id: String,
        before_id: Option<NodeId>,
        after_id: Option<NodeId>,
        at_end: bool,
        content: String,
        content_node_ids: Vec<NodeId>,
    },
    Remove {
        node_id: NodeId,
        parent_id: String,
        precondition: PatchPrecondition,
    },
    Replace {
        node_id: NodeId,
        parent_id: String,
        content: String,
        content_node_ids: Vec<NodeId>,
        precondition: PatchPrecondition,
    },
    Move {
        node_id: NodeId,
        parent_id: String,
        before_id: Option<NodeId>,
        after_id: Option<NodeId>,
        at_end: bool,
        precondition: PatchPrecondition,
    },
    SetAttributes {
        node_id: NodeId,
        parent_id: String,
        attributes: BTreeMap<String, Option<String>>,
        precondition: PatchPrecondition,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RenderPublication {
    Full(RenderSnapshot),
    Patch(RenderPatch),
}

#[derive(Debug)]
pub struct PluginRenderPublication {
    pub publication: RenderPublication,
    pub annotations: BTreeMap<String, String>,
    pub diagnostics: Vec<PluginDiagnostic>,
}

pub struct PreparedExport {
    safe_html: String,
    style: Option<RenderStyle>,
    assets: Vec<RenderAsset>,
}

pub struct ResolvedExport {
    html: String,
}

impl PreparedExport {
    pub fn safe_html(&self) -> &str {
        &self.safe_html
    }

    pub fn style(&self) -> Option<&RenderStyle> {
        self.style.as_ref()
    }

    pub fn assets(&self) -> &[RenderAsset] {
        &self.assets
    }

    pub fn compose_portable<E>(
        self,
        common_runtime: &str,
        composer: impl FnOnce(&str, Option<&RenderStyle>, &[RenderAsset], &str) -> Result<String, E>,
    ) -> Result<ResolvedExport, E> {
        composer(
            &self.safe_html,
            self.style.as_ref(),
            &self.assets,
            common_runtime,
        )
        .map(|html| ResolvedExport { html })
    }
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error(transparent)]
    Parse(#[from] ParseError),
    #[error(transparent)]
    Render(#[from] RenderError),
    #[error("document version {received} is not newer than {current}")]
    StaleVersion { current: u64, received: u64 },
    #[error("the document session is out of sync and requires full text")]
    ContentModified,
    #[error("checkpoint hash does not match the authoritative source")]
    CheckpointMismatch,
    #[error("plugin pipeline failed: {0}")]
    Plugin(String),
    #[error("unsafe plugin HTML is available only after a safe portable render")]
    UnsafeExportPolicy,
    #[error("invalid resolved render asset: {0}")]
    Asset(String),
}

pub struct DocumentSession {
    id: DocumentSessionId,
    source: String,
    position_encoding: PositionEncoding,
    document: Document,
    out_of_sync: bool,
    render_config: RenderConfig,
    plugins: Option<Arc<PluginHost>>,
    plugin_diagnostics: Vec<PluginDiagnostic>,
    previews: HashMap<PreviewSessionId, PreviewCache>,
}

#[derive(Clone)]
struct PreviewCache {
    revision: u64,
    fingerprint: String,
    blocks: Vec<RenderedBlock>,
}

impl DocumentSession {
    pub fn open(
        uri: DocumentUri,
        version: u64,
        source: String,
        position_encoding: PositionEncoding,
    ) -> Result<Self, EngineError> {
        let mut document = parse(uri, version, &source)?;
        reconcile_node_ids(None, &mut document);
        document.validate(&source).map_err(ParseError::from)?;
        Ok(Self {
            id: new_session_id(&document.uri),
            source,
            position_encoding,
            document,
            out_of_sync: false,
            render_config: RenderConfig::default(),
            plugins: None,
            plugin_diagnostics: Vec::new(),
            previews: HashMap::new(),
        })
    }

    fn open_with_plugins(
        uri: DocumentUri,
        version: u64,
        source: String,
        position_encoding: PositionEncoding,
        host: Arc<PluginHost>,
        cancellation: &CancellationToken,
    ) -> Result<(Self, Vec<PluginDiagnostic>), EngineError> {
        let preprocessed = host
            .preprocess_source(version, &source, cancellation)
            .map_err(|error| EngineError::Plugin(error.to_string()))?;
        let mut document = parse(uri, version, &preprocessed.value.text)?;
        if !preprocessed.value.segments.is_empty() {
            remap_document_provenance(&mut document, &preprocessed.value, &source)?;
        }
        reconcile_node_ids(None, &mut document);
        document.validate(&source).map_err(ParseError::from)?;
        let mut session = Self {
            id: new_session_id(&document.uri),
            source,
            position_encoding,
            document,
            out_of_sync: false,
            render_config: RenderConfig::default(),
            plugins: Some(Arc::clone(&host)),
            plugin_diagnostics: Vec::new(),
            previews: HashMap::new(),
        };
        let mut diagnostics = preprocessed.diagnostics;
        diagnostics.extend(session.apply_plugins(&host, cancellation)?);
        session.plugin_diagnostics = diagnostics.clone();
        Ok((session, diagnostics))
    }

    pub fn open_configured(
        uri: DocumentUri,
        version: u64,
        source: String,
        position_encoding: PositionEncoding,
        render_config: RenderConfig,
        plugins: Arc<PluginHost>,
        cancellation: &CancellationToken,
    ) -> Result<Self, EngineError> {
        render_config.validate_for(&plugins)?;
        let (mut session, diagnostics) = Self::open_with_plugins(
            uri,
            version,
            source,
            position_encoding,
            Arc::clone(&plugins),
            cancellation,
        )?;
        session.render_config = render_config;
        session.plugins = Some(plugins);
        session.plugin_diagnostics = diagnostics;
        Ok(session)
    }

    pub fn id(&self) -> &DocumentSessionId {
        &self.id
    }
    pub fn document(&self) -> &Document {
        &self.document
    }
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn position_encoding(&self) -> PositionEncoding {
        self.position_encoding
    }
    pub fn is_out_of_sync(&self) -> bool {
        self.out_of_sync
    }
    pub fn content_hash(&self) -> String {
        content_hash(&self.source)
    }
    pub fn render_config(&self) -> &RenderConfig {
        &self.render_config
    }
    pub fn plugin_diagnostics(&self) -> &[PluginDiagnostic] {
        &self.plugin_diagnostics
    }
    pub fn asset_diagnostics(&self) -> &[AssetDiagnostic] {
        self.render_config.asset_diagnostics()
    }

    pub fn node_at_source_offset(&self, byte_offset: u64) -> Option<NavigationEntry> {
        let offset = usize::try_from(byte_offset).ok()?;
        if !self.source.is_char_boundary(offset) {
            return None;
        }
        self.document
            .navigation()
            .into_iter()
            .filter(|entry| {
                (entry.source_range.byte_start <= byte_offset
                    && byte_offset < entry.source_range.byte_end)
                    || (entry.source_range.byte_start == byte_offset
                        && entry.source_range.byte_end == byte_offset)
            })
            .min_by(|left, right| {
                let left_span = left.source_range.byte_end - left.source_range.byte_start;
                let right_span = right.source_range.byte_end - right.source_range.byte_start;
                left_span
                    .cmp(&right_span)
                    .then_with(|| right.depth.cmp(&left.depth))
                    .then_with(|| left.node_id.0.cmp(&right.node_id.0))
            })
    }

    pub fn source_range_for_node(&self, node_id: &NodeId) -> Option<NavigationEntry> {
        self.document
            .navigation()
            .into_iter()
            .find(|entry| &entry.node_id == node_id)
    }

    pub fn reconfigure(
        &mut self,
        render_config: RenderConfig,
        plugins: Arc<PluginHost>,
        cancellation: &CancellationToken,
    ) -> Result<(), EngineError> {
        render_config.validate_for(&plugins)?;
        let diagnostics = self.install_source_with_plugins(
            self.document.document_version,
            self.source.clone(),
            &plugins,
            cancellation,
        )?;
        self.render_config = render_config;
        self.plugins = Some(plugins);
        self.plugin_diagnostics = diagnostics;
        Ok(())
    }

    pub fn adopt_reconfiguration(&mut self, configured: Self) -> Result<(), EngineError> {
        if self.document.uri != configured.document.uri
            || self.document.document_version != configured.document.document_version
            || self.source != configured.source
            || self.position_encoding != configured.position_encoding
        {
            return Err(EngineError::Plugin(
                "reconfigured session does not match the authoritative source".to_owned(),
            ));
        }
        self.document = configured.document;
        self.render_config = configured.render_config;
        self.plugins = configured.plugins;
        self.plugin_diagnostics = configured.plugin_diagnostics;
        self.out_of_sync = configured.out_of_sync;
        Ok(())
    }

    /// Replaces publication-only configuration without re-running IR plugin hooks.
    pub fn reconfigure_render(&mut self, render_config: RenderConfig) -> Result<(), EngineError> {
        let Some(plugins) = self.plugins.as_ref() else {
            return Err(EngineError::Plugin(
                "render-only reconfiguration requires the session plugin set".to_owned(),
            ));
        };
        render_config.validate_for(plugins)?;
        self.render_config = render_config;
        Ok(())
    }

    pub fn change_full_text(&mut self, version: u64, source: String) -> Result<(), EngineError> {
        if version <= self.document.document_version {
            self.out_of_sync = true;
            return Err(EngineError::StaleVersion {
                current: self.document.document_version,
                received: version,
            });
        }
        if self.out_of_sync {
            return Err(EngineError::ContentModified);
        }
        if let Some(plugins) = self.plugins.clone() {
            self.plugin_diagnostics = self.install_source_with_plugins(
                version,
                source,
                &plugins,
                &CancellationToken::default(),
            )?;
            Ok(())
        } else {
            self.install_source(version, source)
        }
    }

    pub fn resynchronize(&mut self, version: u64, source: String) -> Result<(), EngineError> {
        if version < self.document.document_version
            || (!self.out_of_sync && version == self.document.document_version)
        {
            return Err(EngineError::StaleVersion {
                current: self.document.document_version,
                received: version,
            });
        }
        if let Some(plugins) = self.plugins.clone() {
            self.plugin_diagnostics = self.install_source_with_plugins(
                version,
                source,
                &plugins,
                &CancellationToken::default(),
            )?;
        } else {
            self.install_source(version, source)?;
        }
        self.out_of_sync = false;
        Ok(())
    }

    pub fn checkpoint(&mut self, version: u64, expected_hash: &str) -> Result<(), EngineError> {
        if version != self.document.document_version || expected_hash != self.content_hash() {
            self.out_of_sync = true;
            return Err(EngineError::CheckpointMismatch);
        }
        Ok(())
    }

    fn apply_plugins(
        &mut self,
        host: &PluginHost,
        cancellation: &CancellationToken,
    ) -> Result<Vec<PluginDiagnostic>, EngineError> {
        if self.out_of_sync {
            return Err(EngineError::ContentModified);
        }
        let block_run = host
            .transform_blocks(&self.source, &self.document, cancellation)
            .map_err(|error| EngineError::Plugin(error.to_string()))?;
        let document_run = host
            .transform_document(&self.source, &block_run.value, cancellation)
            .map_err(|error| EngineError::Plugin(error.to_string()))?;
        document_run
            .value
            .validate(&self.source)
            .map_err(|error| EngineError::Plugin(error.to_string()))?;
        self.document = document_run.value;
        let mut diagnostics = block_run.diagnostics;
        diagnostics.extend(document_run.diagnostics);
        Ok(diagnostics)
    }

    pub fn render(
        &mut self,
        preview_session_id: PreviewSessionId,
        context: &RenderContext,
    ) -> Result<RenderPublication, EngineError> {
        self.render_internal(preview_session_id, context, &BTreeMap::new())
    }

    fn render_internal(
        &mut self,
        preview_session_id: PreviewSessionId,
        context: &RenderContext,
        annotations: &BTreeMap<String, String>,
    ) -> Result<RenderPublication, EngineError> {
        if self.out_of_sync {
            return Err(EngineError::ContentModified);
        }
        let fingerprint = renderer_fingerprint(context, &self.render_config, annotations);
        let blocks = HtmlRenderer.render_blocks(&self.document, context)?;
        let previous = self.previews.get(&preview_session_id).cloned();
        let revision = previous.as_ref().map_or(1, |cache| cache.revision + 1);
        let publication = match previous {
            None => full_snapshot(
                &preview_session_id,
                &self.document,
                revision,
                &fingerprint,
                &self.render_config,
                &blocks,
                annotations,
            ),
            Some(cache) if cache.fingerprint != fingerprint => full_snapshot(
                &preview_session_id,
                &self.document,
                revision,
                &fingerprint,
                &self.render_config,
                &blocks,
                annotations,
            ),
            Some(cache) => RenderPublication::Patch(RenderPatch {
                preview_session_id: preview_session_id.clone(),
                document_version: self.document.document_version,
                base_render_revision: cache.revision,
                result_render_revision: revision,
                base_renderer_fingerprint: fingerprint.clone(),
                result_renderer_fingerprint: fingerprint.clone(),
                style: self.render_config.style.clone(),
                navigation: blocks
                    .iter()
                    .flat_map(|block| block.navigation.iter().cloned())
                    .collect(),
                operations: diff_blocks(&cache.blocks, &blocks),
            }),
        };
        self.previews.insert(
            preview_session_id,
            PreviewCache {
                revision,
                fingerprint,
                blocks,
            },
        );
        Ok(publication)
    }

    pub fn render_full(
        &mut self,
        preview_session_id: PreviewSessionId,
        context: &RenderContext,
    ) -> Result<RenderSnapshot, EngineError> {
        self.render_full_internal(preview_session_id, context, &BTreeMap::new())
    }

    pub fn dispose_preview(&mut self, preview_session_id: &PreviewSessionId) -> bool {
        self.previews.remove(preview_session_id).is_some()
    }

    #[cfg(test)]
    fn preview_count(&self) -> usize {
        self.previews.len()
    }

    fn render_full_internal(
        &mut self,
        preview_session_id: PreviewSessionId,
        context: &RenderContext,
        annotations: &BTreeMap<String, String>,
    ) -> Result<RenderSnapshot, EngineError> {
        if self.out_of_sync {
            return Err(EngineError::ContentModified);
        }
        let fingerprint = renderer_fingerprint(context, &self.render_config, annotations);
        let blocks = HtmlRenderer.render_blocks(&self.document, context)?;
        let revision = self
            .previews
            .get(&preview_session_id)
            .map_or(1, |cache| cache.revision + 1);
        let RenderPublication::Full(snapshot) = full_snapshot(
            &preview_session_id,
            &self.document,
            revision,
            &fingerprint,
            &self.render_config,
            &blocks,
            annotations,
        ) else {
            unreachable!("full_snapshot always creates a full publication")
        };
        self.previews.insert(
            preview_session_id,
            PreviewCache {
                revision,
                fingerprint,
                blocks,
            },
        );
        Ok(snapshot)
    }

    fn render_with_plugins(
        &mut self,
        preview_session_id: PreviewSessionId,
        context: &RenderContext,
        host: &PluginHost,
        cancellation: &CancellationToken,
    ) -> Result<PluginRenderPublication, EngineError> {
        let target = match context.target {
            HtmlTarget::Preview => "preview",
            HtmlTarget::Portable => "portable",
        };
        let extension = host
            .extend_render_model(&self.document, target, cancellation)
            .map_err(|error| EngineError::Plugin(error.to_string()))?;
        let publication = self.render_internal(preview_session_id, context, &extension.value)?;
        Ok(PluginRenderPublication {
            publication,
            annotations: extension.value,
            diagnostics: extension.diagnostics,
        })
    }

    pub fn render_configured(
        &mut self,
        preview_session_id: PreviewSessionId,
        cancellation: &CancellationToken,
    ) -> Result<PluginRenderPublication, EngineError> {
        let context = self.render_config.context.clone();
        let Some(host) = self.plugins.clone() else {
            return Ok(PluginRenderPublication {
                publication: self.render_internal(
                    preview_session_id,
                    &context,
                    &BTreeMap::new(),
                )?,
                annotations: BTreeMap::new(),
                diagnostics: Vec::new(),
            });
        };
        self.render_with_plugins(preview_session_id, &context, &host, cancellation)
    }

    pub fn render_full_configured(
        &mut self,
        preview_session_id: PreviewSessionId,
        cancellation: &CancellationToken,
    ) -> Result<PluginRenderPublication, EngineError> {
        let context = self.render_config.context.clone();
        let extension = match self.plugins.clone() {
            Some(host) => host
                .extend_render_model(
                    &self.document,
                    match context.target {
                        HtmlTarget::Preview => "preview",
                        HtmlTarget::Portable => "portable",
                    },
                    cancellation,
                )
                .map_err(|error| EngineError::Plugin(error.to_string()))?,
            None => PluginRun {
                value: BTreeMap::new(),
                diagnostics: Vec::new(),
            },
        };
        let snapshot = self.render_full_internal(preview_session_id, &context, &extension.value)?;
        Ok(PluginRenderPublication {
            publication: RenderPublication::Full(snapshot),
            annotations: extension.value,
            diagnostics: extension.diagnostics,
        })
    }

    pub fn prepare_safe_export(
        &self,
        context: &RenderContext,
    ) -> Result<PreparedExport, EngineError> {
        if context.target != HtmlTarget::Portable {
            return Err(EngineError::UnsafeExportPolicy);
        }
        Ok(PreparedExport {
            safe_html: HtmlRenderer.render(&self.document, context)?,
            style: self.render_config.style.clone(),
            assets: self.render_config.assets().cloned().collect(),
        })
    }

    pub fn apply_unsafe_export_html(
        &self,
        export: ResolvedExport,
        cancellation: &CancellationToken,
    ) -> Result<PluginRun<UnsafeExportOutput>, EngineError> {
        match &self.plugins {
            Some(host) => host
                .unsafe_export_html(self.document.document_version, export.html, cancellation)
                .map_err(|error| EngineError::Plugin(error.to_string())),
            None => Ok(PluginRun {
                value: UnsafeExportOutput {
                    html: export.html,
                    unsafe_output_used: false,
                },
                diagnostics: Vec::new(),
            }),
        }
    }

    fn install_source(&mut self, version: u64, source: String) -> Result<(), EngineError> {
        let mut document = parse(self.document.uri.clone(), version, &source)?;
        reconcile_node_ids(Some(&self.document), &mut document);
        document.validate(&source).map_err(ParseError::from)?;
        self.source = source;
        self.document = document;
        Ok(())
    }

    fn install_source_with_plugins(
        &mut self,
        version: u64,
        source: String,
        host: &PluginHost,
        cancellation: &CancellationToken,
    ) -> Result<Vec<PluginDiagnostic>, EngineError> {
        let preprocessed = host
            .preprocess_source(version, &source, cancellation)
            .map_err(|error| EngineError::Plugin(error.to_string()))?;
        let mut document = parse(self.document.uri.clone(), version, &preprocessed.value.text)?;
        if !preprocessed.value.segments.is_empty() {
            remap_document_provenance(&mut document, &preprocessed.value, &source)?;
        }
        reconcile_node_ids(Some(&self.document), &mut document);
        document.validate(&source).map_err(ParseError::from)?;
        let block_run = host
            .transform_blocks(&source, &document, cancellation)
            .map_err(|error| EngineError::Plugin(error.to_string()))?;
        let document_run = host
            .transform_document(&source, &block_run.value, cancellation)
            .map_err(|error| EngineError::Plugin(error.to_string()))?;
        document_run
            .value
            .validate(&source)
            .map_err(|error| EngineError::Plugin(error.to_string()))?;
        self.source = source;
        self.document = document_run.value;
        let mut diagnostics = preprocessed.diagnostics;
        diagnostics.extend(block_run.diagnostics);
        diagnostics.extend(document_run.diagnostics);
        Ok(diagnostics)
    }
}

fn full_snapshot(
    preview: &PreviewSessionId,
    document: &Document,
    revision: u64,
    fingerprint: &str,
    config: &RenderConfig,
    blocks: &[RenderedBlock],
    annotations: &BTreeMap<String, String>,
) -> RenderPublication {
    let annotation_html = if annotations.is_empty() {
        String::new()
    } else {
        let json = serde_json::to_string(annotations)
            .expect("render annotations are serializable")
            .replace('&', "\\u0026")
            .replace('<', "\\u003c")
            .replace('>', "\\u003e")
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029");
        format!(
            "<script type=\"application/json\" data-fleximark-render-annotations>{json}</script>"
        )
    };
    RenderPublication::Full(RenderSnapshot {
        preview_session_id: preview.clone(),
        document_version: document.document_version,
        result_render_revision: revision,
        renderer_fingerprint: fingerprint.to_owned(),
        style: config.style.clone(),
        assets: config
            .assets
            .iter()
            .map(|asset| asset.published.clone())
            .collect(),
        node_ids: std::iter::once(NodeId(ROOT_NODE_ID.to_owned()))
            .chain(
                blocks
                    .iter()
                    .flat_map(|block| block.node_ids.iter().cloned()),
            )
            .collect(),
        navigation: blocks
            .iter()
            .flat_map(|block| block.navigation.iter().cloned())
            .collect(),
        html: format!(
            "<main data-fleximark-node-id=\"{ROOT_NODE_ID}\">{annotation_html}{}</main>",
            blocks
                .iter()
                .map(|block| block.html.as_str())
                .collect::<String>()
        ),
    })
}

fn remap_document_provenance(
    document: &mut Document,
    preprocessed: &PreprocessedSource,
    original_source: &str,
) -> Result<(), EngineError> {
    fn remap_inline(
        inline: &mut Inline,
        preprocessed: &PreprocessedSource,
        original_source: &str,
    ) -> Result<(), EngineError> {
        inline.provenance = remap_provenance(&inline.provenance, preprocessed, original_source)?;
        let children = match &mut inline.kind {
            InlineKind::Emphasis { children }
            | InlineKind::Strong { children }
            | InlineKind::Strikethrough { children }
            | InlineKind::Link { children, .. }
            | InlineKind::Image { children, .. } => children,
            _ => return Ok(()),
        };
        for child in children {
            remap_inline(child, preprocessed, original_source)?;
        }
        Ok(())
    }

    fn remap_blocks(
        blocks: &mut [Block],
        preprocessed: &PreprocessedSource,
        original_source: &str,
    ) -> Result<(), EngineError> {
        for block in blocks {
            block.provenance = remap_provenance(&block.provenance, preprocessed, original_source)?;
            for child in &mut block.children {
                match child {
                    Node::Block(block) => {
                        remap_blocks(std::slice::from_mut(block), preprocessed, original_source)?
                    }
                    Node::Inline(inline) => remap_inline(inline, preprocessed, original_source)?,
                }
            }
        }
        Ok(())
    }

    remap_blocks(&mut document.blocks, preprocessed, original_source)
}

fn remap_provenance(
    provenance: &SourceProvenance,
    preprocessed: &PreprocessedSource,
    original_source: &str,
) -> Result<SourceProvenance, EngineError> {
    let range = provenance
        .primary_range()
        .ok_or_else(|| EngineError::Plugin("parser provenance has no range to map".to_owned()))?;
    let mut ranges = Vec::new();
    let mut primary_range = None;
    let mut generated_anchor = None;
    let source_range = |byte_start: u64, byte_end: u64| {
        let start = usize::try_from(byte_start)
            .map_err(|_| EngineError::Plugin("preprocess origin offset overflowed".to_owned()))?;
        let end = usize::try_from(byte_end)
            .map_err(|_| EngineError::Plugin("preprocess origin offset overflowed".to_owned()))?;
        if !original_source.is_char_boundary(start) || !original_source.is_char_boundary(end) {
            return Err(EngineError::Plugin(
                "preprocess edit map cannot be clipped at UTF-8 boundaries".to_owned(),
            ));
        }
        let position = |offset: usize| {
            let before = &original_source[..offset];
            let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
            SourcePosition {
                line: before.bytes().filter(|byte| *byte == b'\n').count() as u64,
                character: (offset - line_start) as u64,
                encoding: PositionEncoding::Utf8,
            }
        };
        Ok(fleximark_model::SourceRange {
            byte_start,
            byte_end,
            start: position(start),
            end: position(end),
        })
    };
    for segment in &preprocessed.segments {
        let empty = range.byte_start == range.byte_end;
        let contains_empty = segment.output_start <= range.byte_start
            && (range.byte_start < segment.output_end
                || (range.byte_start == preprocessed.text.len() as u64
                    && segment.output_end == range.byte_start));
        if (!empty
            && (segment.output_start >= range.byte_end || segment.output_end <= range.byte_start))
            || (empty && !contains_empty)
        {
            continue;
        }
        match &segment.origin {
            EditOrigin::Original {
                ranges: origin,
                primary_range_index,
            }
            | EditOrigin::Derived {
                ranges: origin,
                primary_range_index,
            } => {
                let overlap_start = segment.output_start.max(range.byte_start);
                let overlap_end = segment.output_end.min(range.byte_end);
                let relative_start = overlap_start - segment.output_start;
                let relative_end = overlap_end - segment.output_start;
                let origin_length = origin
                    .iter()
                    .try_fold(0_u64, |length, range| {
                        length.checked_add(range.byte_end - range.byte_start)
                    })
                    .ok_or_else(|| {
                        EngineError::Plugin("preprocess origin length overflowed".to_owned())
                    })?;
                let exact = origin_length == segment.output_end - segment.output_start;
                if exact {
                    let mut origin_offset = 0_u64;
                    for (index, origin_range) in origin.iter().enumerate() {
                        let length = origin_range.byte_end - origin_range.byte_start;
                        if empty
                            && (relative_start < origin_offset + length
                                || (relative_start == origin_length && index + 1 == origin.len()))
                        {
                            let offset = origin_range.byte_start + relative_start - origin_offset;
                            let mapped = source_range(offset, offset)?;
                            if index == *primary_range_index as usize && primary_range.is_none() {
                                primary_range = Some(mapped.clone());
                            }
                            ranges.push(mapped);
                            break;
                        }
                        let start = relative_start.max(origin_offset);
                        let end = relative_end.min(origin_offset + length);
                        origin_offset += length;
                        if start >= end {
                            continue;
                        }
                        let byte_start = origin_range.byte_start + start - (origin_offset - length);
                        let byte_end = origin_range.byte_start + end - (origin_offset - length);
                        let mapped = source_range(byte_start, byte_end)?;
                        if index == *primary_range_index as usize && primary_range.is_none() {
                            primary_range = Some(mapped.clone());
                        }
                        ranges.push(mapped);
                    }
                } else {
                    for (index, origin_range) in origin.iter().enumerate() {
                        if index == *primary_range_index as usize && primary_range.is_none() {
                            primary_range = Some(origin_range.clone());
                        }
                        ranges.push(origin_range.clone());
                    }
                }
            }
            EditOrigin::Generated { anchor } => {
                generated_anchor = generated_anchor.or_else(|| anchor.clone());
            }
        }
    }
    if !ranges.is_empty() {
        ranges.sort_by_key(|range| (range.byte_start, range.byte_end));
        ranges.dedup();
        if ranges
            .windows(2)
            .any(|pair| pair[1].byte_start < pair[0].byte_end)
        {
            return Err(EngineError::Plugin(
                "mapped preprocess provenance overlaps in the original snapshot".to_owned(),
            ));
        }
        let primary_range_index = primary_range
            .and_then(|primary| ranges.iter().position(|range| range == &primary))
            .unwrap_or(0) as u32;
        return Ok(SourceProvenance::Derived {
            ranges,
            primary_range_index,
            transform: TransformId("plugin-preprocess-v1".to_owned()),
        });
    }
    if generated_anchor.is_some() {
        return Ok(SourceProvenance::Generated {
            anchor: generated_anchor.map(|range| GeneratedAnchor {
                range,
                affinity: AnchorAffinity::After,
            }),
            transform: TransformId("plugin-preprocess-v1".to_owned()),
        });
    }
    Err(EngineError::Plugin(
        "preprocess edit map does not cover parser provenance".to_owned(),
    ))
}

fn diff_blocks(previous: &[RenderedBlock], current: &[RenderedBlock]) -> Vec<PatchOperation> {
    let current_ids = current
        .iter()
        .map(|block| block.id.clone())
        .collect::<HashSet<_>>();
    let previous_html = previous
        .iter()
        .map(|block| (&block.id, block.html.as_str()))
        .collect::<HashMap<_, _>>();
    let mut working = previous
        .iter()
        .map(|block| block.id.clone())
        .collect::<Vec<_>>();
    let mut operations = Vec::new();
    let precondition = || PatchPrecondition {
        node_exists: true,
        current_parent_id: ROOT_NODE_ID.to_owned(),
    };

    for block in previous {
        if !current_ids.contains(&block.id) {
            operations.push(PatchOperation::Remove {
                node_id: block.id.clone(),
                parent_id: ROOT_NODE_ID.to_owned(),
                precondition: precondition(),
            });
            working.retain(|id| id != &block.id);
        }
    }
    for (index, block) in current.iter().enumerate() {
        if let Some(position) = working.iter().position(|id| id == &block.id) {
            if position != index {
                working.remove(position);
                let before_id = working.get(index).cloned();
                operations.push(PatchOperation::Move {
                    node_id: block.id.clone(),
                    parent_id: ROOT_NODE_ID.to_owned(),
                    before_id,
                    after_id: None,
                    at_end: index == working.len(),
                    precondition: precondition(),
                });
                working.insert(index, block.id.clone());
            }
            if let Some(previous) = previous_html.get(&block.id) {
                if *previous != block.html {
                    if let Some(attributes) = presentation_attribute_delta(previous, &block.html) {
                        operations.push(PatchOperation::SetAttributes {
                            node_id: block.id.clone(),
                            parent_id: ROOT_NODE_ID.to_owned(),
                            attributes,
                            precondition: precondition(),
                        });
                    } else {
                        operations.push(PatchOperation::Replace {
                            node_id: block.id.clone(),
                            parent_id: ROOT_NODE_ID.to_owned(),
                            content: block.html.clone(),
                            content_node_ids: block.node_ids.clone(),
                            precondition: precondition(),
                        });
                    }
                }
            }
        } else {
            let before_id = working.get(index).cloned();
            operations.push(PatchOperation::Insert {
                node_id: block.id.clone(),
                parent_id: ROOT_NODE_ID.to_owned(),
                before_id,
                after_id: None,
                at_end: index == working.len(),
                content: block.html.clone(),
                content_node_ids: block.node_ids.clone(),
            });
            working.insert(index, block.id.clone());
        }
    }
    operations
}

fn presentation_attribute_delta(
    previous: &str,
    current: &str,
) -> Option<BTreeMap<String, Option<String>>> {
    fn root(value: &str) -> Option<(&str, BTreeMap<&str, &str>, &str)> {
        let end = value.find('>')?;
        let opening = value.get(1..end)?;
        let mut parts = opening.split_ascii_whitespace();
        let tag = parts.next()?;
        let mut attributes = BTreeMap::new();
        for part in parts {
            let (name, quoted) = part.split_once('=')?;
            attributes.insert(name, quoted.strip_prefix('"')?.strip_suffix('"')?);
        }
        Some((tag, attributes, &value[end + 1..]))
    }

    let (previous_tag, mut previous_attributes, previous_content) = root(previous)?;
    let (current_tag, mut current_attributes, current_content) = root(current)?;
    if previous_tag != current_tag || previous_content != current_content {
        return None;
    }
    let identity = "data-fleximark-node-id";
    if previous_attributes.remove(identity) != current_attributes.remove(identity) {
        return None;
    }
    let allowed = [
        "role",
        "aria-checked",
        "open",
        "class",
        "data-line-numbers",
        "data-admonition-kind",
    ];
    let mut delta = BTreeMap::new();
    for name in previous_attributes
        .keys()
        .chain(current_attributes.keys())
        .copied()
        .collect::<HashSet<_>>()
    {
        if previous_attributes.get(name) != current_attributes.get(name) {
            if !allowed.contains(&name) {
                return None;
            }
            delta.insert(
                name.to_owned(),
                current_attributes
                    .get(name)
                    .map(|value| (*value).to_owned()),
            );
        }
    }
    (!delta.is_empty()).then_some(delta)
}

#[derive(Clone)]
struct IdentityRecord {
    semantic: String,
    structure: std::mem::Discriminant<BlockKind>,
    parent: String,
    provenance: String,
    id: NodeId,
}

fn reconcile_node_ids(previous: Option<&Document>, current: &mut Document) {
    let mut old = Vec::new();
    if let Some(previous) = previous {
        collect_identity(&previous.blocks, "root", &mut old);
    }
    let mut new = Vec::new();
    collect_identity(&current.blocks, "root", &mut new);
    let mut assigned = vec![None; new.len()];
    let mut used_old = HashSet::new();

    assign_unique(&old, &new, &mut assigned, &mut used_old, |record| {
        format!("{}\0{}", record.semantic, record.provenance)
    });
    assign_unique(&old, &new, &mut assigned, &mut used_old, |record| {
        format!("{}\0{}", record.semantic, record.parent)
    });
    assign_unique(&old, &new, &mut assigned, &mut used_old, |record| {
        format!("{}\0{}", record.provenance, record.parent)
    });
    assign_unique(&old, &new, &mut assigned, &mut used_old, |record| {
        record.semantic.clone()
    });
    for (new_index, candidate) in new.iter().enumerate() {
        if assigned[new_index].is_some() {
            continue;
        }
        let available_old = old
            .iter()
            .enumerate()
            .filter(|(old_index, previous)| {
                !used_old.contains(old_index)
                    && previous.structure == candidate.structure
                    && previous.parent == candidate.parent
            })
            .count();
        let available_new = new
            .iter()
            .enumerate()
            .filter(|(index, next)| {
                assigned[*index].is_none()
                    && next.structure == candidate.structure
                    && next.parent == candidate.parent
            })
            .count();
        if available_old != available_new {
            continue;
        }
        if let Some((old_index, previous)) = old.iter().enumerate().find(|(old_index, previous)| {
            !used_old.contains(old_index)
                && previous.structure == candidate.structure
                && previous.parent == candidate.parent
        }) {
            assigned[new_index] = Some(previous.id.clone());
            used_old.insert(old_index);
        }
    }

    let mut occupied = old
        .iter()
        .map(|record| record.id.clone())
        .collect::<HashSet<_>>();
    for (index, slot) in assigned.iter_mut().enumerate() {
        if slot.is_some() {
            continue;
        }
        let mut salt = 0_u64;
        loop {
            let material = format!(
                "{}\0{}\0{index}\0{}\0{}\0{salt}",
                current.uri.0, current.document_version, new[index].semantic, new[index].provenance
            );
            let id = NodeId(format!(
                "block-{}",
                &blake3::hash(material.as_bytes()).to_hex()[..20]
            ));
            if occupied.insert(id.clone()) {
                *slot = Some(id);
                break;
            }
            salt += 1;
        }
    }
    let mut ids = assigned.into_iter().map(Option::unwrap);
    apply_ids(&mut current.blocks, &mut ids);
}

fn assign_unique(
    old: &[IdentityRecord],
    new: &[IdentityRecord],
    assigned: &mut [Option<NodeId>],
    used_old: &mut HashSet<usize>,
    key: impl Fn(&IdentityRecord) -> String,
) {
    let mut old_groups: HashMap<String, Vec<usize>> = HashMap::new();
    let mut new_groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, record) in old
        .iter()
        .enumerate()
        .filter(|(index, _)| !used_old.contains(index))
    {
        old_groups.entry(key(record)).or_default().push(index);
    }
    for (index, record) in new
        .iter()
        .enumerate()
        .filter(|(index, _)| assigned[*index].is_none())
    {
        new_groups.entry(key(record)).or_default().push(index);
    }
    for (key, old_indexes) in old_groups {
        let Some(new_indexes) = new_groups.get(&key) else {
            continue;
        };
        if old_indexes.len() == 1 && new_indexes.len() == 1 {
            let old_index = old_indexes[0];
            assigned[new_indexes[0]] = Some(old[old_index].id.clone());
            used_old.insert(old_index);
        }
    }
}

fn collect_identity(blocks: &[Block], parent: &str, output: &mut Vec<IdentityRecord>) {
    for block in blocks {
        let semantic = semantic_key(block);
        output.push(IdentityRecord {
            semantic: semantic.clone(),
            structure: std::mem::discriminant(&block.kind),
            parent: parent.to_owned(),
            provenance: provenance_key(&block.provenance),
            id: block.id.clone(),
        });
        for child in &block.children {
            if let Node::Block(child) = child {
                collect_identity(std::slice::from_ref(child), &semantic, output);
            }
        }
    }
}

fn semantic_key(block: &Block) -> String {
    let mut material = format!("{:?}\0{:?}", block.kind, block.attributes);
    for child in &block.children {
        match child {
            Node::Block(block) => material.push_str(&semantic_key(block)),
            Node::Inline(inline) => append_inline_semantics(inline, &mut material),
        }
        material.push('\0');
    }
    blake3::hash(material.as_bytes()).to_hex().to_string()
}

fn append_inline_semantics(inline: &Inline, output: &mut String) {
    match &inline.kind {
        InlineKind::Text { value } => {
            output.push_str("text:");
            output.push_str(value);
        }
        InlineKind::Code { value } => {
            output.push_str("code:");
            output.push_str(value);
        }
        InlineKind::Math { source } => {
            output.push_str("math:");
            output.push_str(source);
        }
        InlineKind::SoftBreak => output.push_str("soft-break"),
        InlineKind::HardBreak => output.push_str("hard-break"),
        InlineKind::RawHtml { html } => {
            output.push_str("raw:");
            output.push_str(html);
        }
        InlineKind::Emphasis { children } => append_nested("em", children, output),
        InlineKind::Strong { children } => append_nested("strong", children, output),
        InlineKind::Strikethrough { children } => append_nested("strike", children, output),
        InlineKind::Link {
            destination,
            title,
            children,
        } => {
            output.push_str("link:");
            output.push_str(destination);
            output.push('\0');
            output.push_str(title);
            append_nested("", children, output);
        }
        InlineKind::Image {
            source,
            title,
            children,
        } => {
            output.push_str("image:");
            output.push_str(source);
            output.push('\0');
            output.push_str(title);
            append_nested("", children, output);
        }
    }
}

fn append_nested(label: &str, children: &[Inline], output: &mut String) {
    output.push_str(label);
    output.push('[');
    for child in children {
        append_inline_semantics(child, output);
        output.push('\0');
    }
    output.push(']');
}

fn provenance_key(provenance: &SourceProvenance) -> String {
    serde_json::to_string(provenance).expect("source provenance is serializable")
}

fn apply_ids(blocks: &mut [Block], ids: &mut impl Iterator<Item = NodeId>) {
    for block in blocks {
        block.id = ids.next().expect("identity record count matches blocks");
        for child in &mut block.children {
            if let Node::Block(child) = child {
                apply_ids(std::slice::from_mut(child), ids);
            }
        }
    }
}

fn renderer_fingerprint(
    context: &RenderContext,
    config: &RenderConfig,
    annotations: &BTreeMap<String, String>,
) -> String {
    let target = match context.target {
        HtmlTarget::Preview => "preview",
        HtmlTarget::Portable => "portable",
    };
    let raw = match context.raw_html {
        RawHtmlPolicy::Escape => "escape",
        RawHtmlPolicy::Reject => "reject",
    };
    let style_fingerprint = config
        .style
        .as_ref()
        .map_or("none", RenderStyle::fingerprint);
    let annotations = serde_json::to_string(annotations).expect("render annotations serialize");
    let assets = serde_json::to_string(
        &config
            .assets
            .iter()
            .map(|asset| &asset.published)
            .collect::<Vec<_>>(),
    )
    .expect("resolved assets serialize");
    let asset_diagnostics =
        serde_json::to_string(&config.asset_diagnostics).expect("asset diagnostics serialize");
    let value = format!(
        "{}\0{}\0{target}\0{raw}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{annotations}\0{assets}\0{asset_diagnostics}",
        config.renderer_version,
        config.sanitizer_version,
        context.allow_remote_resources,
        context.allow_data_resources,
        style_fingerprint,
        config.config_hash,
        config.config_generation,
        config.plugin_set_hash,
        config.plugin_generation,
    );
    format!("sha256:{:x}", Sha256::digest(value.as_bytes()))
}

fn content_hash(source: &str) -> String {
    content_hash_bytes(source.as_bytes())
}

fn content_hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn new_session_id(uri: &DocumentUri) -> DocumentSessionId {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = SESSION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let value = format!("{}\0{time}\0{sequence}", uri.0);
    DocumentSessionId(format!(
        "document-{}",
        &blake3::hash(value.as_bytes()).to_hex()[..24]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf8_range(
        source: &str,
        byte_start: usize,
        byte_end: usize,
    ) -> fleximark_model::SourceRange {
        let position = |offset: usize| {
            let before = &source[..offset];
            let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
            SourcePosition {
                line: before.bytes().filter(|byte| *byte == b'\n').count() as u64,
                character: (offset - line_start) as u64,
                encoding: PositionEncoding::Utf8,
            }
        };
        fleximark_model::SourceRange {
            byte_start: byte_start as u64,
            byte_end: byte_end as u64,
            start: position(byte_start),
            end: position(byte_end),
        }
    }

    fn open(source: &str) -> DocumentSession {
        DocumentSession::open(
            DocumentUri("file:///test.md".into()),
            1,
            source.to_owned(),
            PositionEncoding::Utf16,
        )
        .unwrap()
    }

    #[test]
    fn reconciles_moved_unique_nodes_but_not_ambiguous_duplicates() {
        let mut session = open("# A\n\n# B\n");
        let old = session
            .document
            .blocks
            .iter()
            .map(|block| block.id.clone())
            .collect::<Vec<_>>();
        session
            .change_full_text(2, "# Intro\n\n# A\n\n# B\n".into())
            .unwrap();
        assert_eq!(session.document.blocks[1].id, old[0]);
        assert_eq!(session.document.blocks[2].id, old[1]);

        let mut duplicates = open("same\n");
        let old_id = duplicates.document.blocks[0].id.clone();
        duplicates
            .change_full_text(2, "# lead\n\nsame\n\nsame\n".into())
            .unwrap();
        assert!(
            duplicates.document.blocks[1..]
                .iter()
                .all(|block| block.id != old_id)
        );
        assert_ne!(
            duplicates.document.blocks[1].id,
            duplicates.document.blocks[2].id
        );
    }

    #[test]
    fn rejects_stale_changes_until_full_text_resynchronization() {
        let mut session = open("old\n");
        assert!(matches!(
            session.change_full_text(1, "stale\n".into()),
            Err(EngineError::StaleVersion { .. })
        ));
        assert!(session.is_out_of_sync());
        assert!(matches!(
            session.render(PreviewSessionId("p".into()), &RenderContext::default()),
            Err(EngineError::ContentModified)
        ));
        session.resynchronize(1, "fresh\n".into()).unwrap();
        assert!(!session.is_out_of_sync());
        assert_eq!(session.document().document_version, 1);
        assert_eq!(session.source(), "fresh\n");
    }

    #[test]
    fn publishes_patch_only_with_equal_fingerprints_and_full_on_policy_change() {
        let mut session = open("# A\n");
        session.render_config.style = Some(RenderStyle::from_validated_css(
            "main { color: canvastext; }".to_owned(),
        ));
        let preview = PreviewSessionId("preview-1".into());
        let first = session
            .render(preview.clone(), &RenderContext::default())
            .unwrap();
        let RenderPublication::Full(snapshot) = first else {
            panic!("first render must be full")
        };
        let original_id = session.document.blocks[0].id.clone();
        assert!(
            snapshot
                .html
                .starts_with("<main data-fleximark-node-id=\"document-root\">")
        );
        assert_eq!(snapshot.node_ids[0], NodeId("document-root".into()));
        assert_eq!(snapshot.node_ids[1], session.document.blocks[0].id);
        assert_eq!(snapshot.style, session.render_config.style);
        assert!(!snapshot.html.contains("color: canvastext"));
        assert_eq!(
            snapshot.navigation[0].node_id,
            session.document.blocks[0].id
        );
        session.change_full_text(2, "# A changed\n".into()).unwrap();
        let second = session
            .render(preview.clone(), &RenderContext::default())
            .unwrap();
        let RenderPublication::Patch(patch) = second else {
            panic!("expected patch")
        };
        assert_eq!(session.document.blocks[0].id, original_id);
        assert!(
            matches!(patch.operations.as_slice(), [PatchOperation::Replace { node_id, .. }] if node_id == &original_id)
        );
        assert_eq!(
            patch.base_renderer_fingerprint,
            patch.result_renderer_fingerprint
        );
        assert_eq!(patch.style, session.render_config.style);
        assert_eq!(
            (patch.base_render_revision, patch.result_render_revision),
            (1, 2)
        );
        assert!(!patch.operations.is_empty());
        let wire = serde_json::to_value(&patch).unwrap();
        assert_eq!(wire["baseRenderRevision"], 1);
        assert!(wire["navigation"][0]["sourceRange"]["byteStart"].is_number());
        assert!(wire["operations"][0].get("nodeId").is_some());
        assert!(
            wire["operations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|operation| operation.get("contentNodeIds").is_some())
        );

        let strict = RenderContext {
            raw_html: RawHtmlPolicy::Reject,
            ..RenderContext::default()
        };
        let third = session.render(preview, &strict).unwrap();
        let RenderPublication::Full(snapshot) = third else {
            panic!("fingerprint change must be full")
        };
        assert_eq!(snapshot.result_render_revision, 3);
    }

    #[test]
    fn emits_allowlisted_attribute_delta_and_serializes_it_in_camel_case() {
        let mut session = open(":::info[Note]\nbody\n:::\n");
        let preview = PreviewSessionId("attributes".into());
        session
            .render(preview.clone(), &RenderContext::default())
            .unwrap();
        let original_id = session.document.blocks[0].id.clone();
        session
            .change_full_text(2, ":::tip[Note]\nbody\n:::\n".to_owned())
            .unwrap();
        assert_eq!(session.document.blocks[0].id, original_id);
        let RenderPublication::Patch(patch) =
            session.render(preview, &RenderContext::default()).unwrap()
        else {
            panic!("presentation-only change should patch")
        };
        assert!(matches!(
            patch.operations.as_slice(),
            [PatchOperation::SetAttributes { attributes, .. }]
                if attributes.get("data-admonition-kind") == Some(&Some("tip".to_owned()))
        ));
        let wire = serde_json::to_value(&patch.operations[0]).unwrap();
        assert_eq!(wire["type"], "setAttributes");
        assert_eq!(wire["attributes"]["data-admonition-kind"], "tip");
    }

    #[test]
    fn resolved_assets_are_bounded_published_and_fingerprinted_without_source_paths() {
        let mut session = open("![diagram](private/diagram.png)\n");
        let first_asset = ResolvedRenderAsset::from_validated_bytes(
            "private/diagram.png".to_owned(),
            "image/png".to_owned(),
            b"png-one",
        )
        .unwrap();
        session.render_config = RenderConfig::default()
            .with_resolved_assets(vec![first_asset])
            .unwrap();
        let preview = PreviewSessionId("asset-preview".into());
        let context = session.render_config.context.clone();
        let RenderPublication::Full(first) = session.render(preview.clone(), &context).unwrap()
        else {
            panic!("first asset publication must be full")
        };
        assert_eq!(first.assets.len(), 1);
        assert_eq!(first.assets[0].data, BASE64.encode(b"png-one"));
        assert!(first.html.contains(&first.assets[0].reference));
        let wire = serde_json::to_string(&first).unwrap();
        assert!(!wire.contains("private/diagram.png"));

        session
            .change_full_text(2, "![changed](private/diagram.png)\n".to_owned())
            .unwrap();
        let context = session.render_config.context.clone();
        let RenderPublication::Patch(patch) = session.render(preview.clone(), &context).unwrap()
        else {
            panic!("unchanged asset set may patch")
        };
        assert!(
            serde_json::to_value(&patch)
                .unwrap()
                .get("assets")
                .is_none()
        );

        session.render_config = RenderConfig::default()
            .with_resolved_assets(vec![
                ResolvedRenderAsset::from_validated_bytes(
                    "private/diagram.png".to_owned(),
                    "image/png".to_owned(),
                    b"png-two",
                )
                .unwrap(),
            ])
            .unwrap();
        let context = session.render_config.context.clone();
        let RenderPublication::Full(second) = session.render(preview, &context).unwrap() else {
            panic!("asset content change must force full publication")
        };
        assert_ne!(first.renderer_fingerprint, second.renderer_fingerprint);
        assert_ne!(first.assets[0].content_hash, second.assets[0].content_hash);

        assert!(
            ResolvedRenderAsset::from_validated_bytes(
                "too-large.bin".to_owned(),
                "application/octet-stream".to_owned(),
                &vec![0; MAX_RENDER_ASSET_BYTES + 1],
            )
            .is_err()
        );
    }

    #[test]
    fn large_assets_are_published_once_and_not_repeated_in_multi_operation_patch() {
        let original = (0..31)
            .map(|index| format!("paragraph {index}"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let changed = (0..31)
            .map(|index| format!("changed paragraph {index}"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let assets: Vec<_> = (0..8)
            .map(|index| {
                let mut bytes = vec![0_u8; MAX_RENDER_ASSET_BYTES];
                bytes[0] = index;
                ResolvedRenderAsset::from_validated_bytes(
                    format!("asset-{index}.png"),
                    "image/png".to_owned(),
                    &bytes,
                )
                .unwrap()
            })
            .collect();
        let mut session = open(&original);
        session.render_config = RenderConfig::default()
            .with_resolved_assets(assets)
            .unwrap();
        let preview = PreviewSessionId("large-assets".into());
        let context = session.render_config.context.clone();
        let RenderPublication::Full(snapshot) = session.render(preview.clone(), &context).unwrap()
        else {
            panic!("initial publication must be full")
        };
        assert_eq!(snapshot.assets.len(), 8);

        session.change_full_text(2, changed).unwrap();
        let RenderPublication::Patch(patch) = session.render(preview, &context).unwrap() else {
            panic!("unchanged assets may patch")
        };
        assert_eq!(patch.operations.len(), 31);
        let wire = serde_json::to_vec(&patch).unwrap();
        assert!(wire.len() < 64 * 1024);
        assert!(!String::from_utf8(wire).unwrap().contains("\"assets\""));
    }

    #[test]
    fn render_only_reconfiguration_preserves_ir_and_plugin_diagnostics() {
        let host = Arc::new(
            PluginHost::configured(
                fleximark_plugin_host::ExecutionLimits::default(),
                fleximark_plugin_host::HostPolicy {
                    workspace_trusted: true,
                    workspace_root: ".".to_owned(),
                },
                content_hash("config"),
                3,
            )
            .unwrap(),
        );
        let initial = RenderConfig::for_plugins(RenderContext::default(), None, &host);
        let mut session = DocumentSession::open_configured(
            DocumentUri("file:///render-only.md".into()),
            1,
            "before\n".to_owned(),
            PositionEncoding::Utf8,
            initial,
            Arc::clone(&host),
            &CancellationToken::default(),
        )
        .unwrap();
        let document = session.document.clone();
        let diagnostics = session.plugin_diagnostics.clone();
        let updated = RenderConfig::for_plugins(RenderContext::default(), None, &host)
            .with_resolved_assets(vec![
                ResolvedRenderAsset::from_validated_bytes(
                    "unused.png".to_owned(),
                    "image/png".to_owned(),
                    b"image",
                )
                .unwrap(),
            ])
            .unwrap();

        session.reconfigure_render(updated).unwrap();

        assert_eq!(session.document, document);
        assert_eq!(session.source, "before\n");
        assert_eq!(session.plugin_diagnostics, diagnostics);
        assert_eq!(session.render_config.assets().len(), 1);
    }

    #[test]
    fn staged_reconfiguration_preserves_preview_revision_and_forces_full() {
        let mut session = open("hello\n");
        let preview = PreviewSessionId("reconfigure".into());
        session
            .render(preview.clone(), &RenderContext::default())
            .unwrap();
        let mut configured = open("hello\n");
        configured.render_config.style = Some(RenderStyle::from_validated_css(
            "p { color: green; }".to_owned(),
        ));
        session.adopt_reconfiguration(configured).unwrap();
        let RenderPublication::Full(snapshot) =
            session.render(preview, &RenderContext::default()).unwrap()
        else {
            panic!("configuration fingerprint change must be full")
        };
        assert_eq!(snapshot.result_render_revision, 2);
    }

    #[test]
    fn checkpoint_is_bound_to_version_and_utf8_content() {
        let mut session = open("🦀\n");
        let hash = session.content_hash();
        assert_eq!(
            hash,
            "5d40fbf44301a6d80c06a5a5fb6aa8cdbb0c987d3aa07740e2bb941df3d7b862"
        );
        session.checkpoint(1, &hash).unwrap();
        assert!(matches!(
            session.checkpoint(1, "wrong"),
            Err(EngineError::CheckpointMismatch)
        ));
    }

    #[test]
    fn render_metadata_and_annotations_are_part_of_the_publication() {
        let mut session = open("# A\n");
        let preview = PreviewSessionId("configured".into());
        let first = session
            .render(preview.clone(), &RenderContext::default())
            .unwrap();
        let RenderPublication::Full(first) = first else {
            panic!("first publication must be full")
        };

        session.render_config.style = Some(RenderStyle::from_validated_css(
            "main { color: rebeccapurple; }".to_owned(),
        ));
        let second = session.render(preview, &RenderContext::default()).unwrap();
        let RenderPublication::Full(second) = second else {
            panic!("metadata changes must force a full publication")
        };
        assert_ne!(first.renderer_fingerprint, second.renderer_fingerprint);
        assert_eq!(second.style, session.render_config.style);
        let wire = serde_json::to_value(&second).unwrap();
        assert_eq!(wire["style"]["css"], "main { color: rebeccapurple; }");
        assert!(wire["style"]["fingerprint"].is_string());

        let annotations =
            BTreeMap::from([("plugin".to_owned(), "</script><img src=x>".to_owned())]);
        let annotated = session
            .render_internal(
                PreviewSessionId("annotations".into()),
                &RenderContext::default(),
                &annotations,
            )
            .unwrap();
        let RenderPublication::Full(annotated) = annotated else {
            panic!("first annotated publication must be full")
        };
        assert!(annotated.html.contains("data-fleximark-render-annotations"));
        assert!(annotated.html.contains("\\u003c/script\\u003e"));
        assert!(!annotated.html.contains("</script><img"));
    }

    #[test]
    fn unsafe_plugin_html_is_reachable_only_from_explicit_portable_export() {
        let mut session = open("hello\n");
        session.render_config.style = Some(RenderStyle::from_validated_css(
            "body { color: green; }".to_owned(),
        ));
        assert!(matches!(
            session.prepare_safe_export(&RenderContext::default()),
            Err(EngineError::UnsafeExportPolicy)
        ));
        let portable = RenderContext {
            target: HtmlTarget::Portable,
            raw_html: RawHtmlPolicy::Reject,
            allow_remote_resources: false,
            allow_data_resources: false,
            resolved_resources: BTreeMap::new(),
        };
        let prepared = session.prepare_safe_export(&portable).unwrap();
        assert!(prepared.safe_html().contains("hello"));
        let denied = session
            .prepare_safe_export(&portable)
            .unwrap()
            .compose_portable("common-runtime", |_, _, _, _| {
                Err::<String, _>("asset containment failed")
            });
        assert!(matches!(denied, Err("asset containment failed")));
        let resolved = prepared
            .compose_portable("common-runtime", |html, style, assets, runtime| {
                assert_eq!(style.unwrap().css(), "body { color: green; }");
                assert!(assets.is_empty());
                assert_eq!(runtime, "common-runtime");
                Ok::<_, EngineError>(format!(
                    "<style>{}</style>{}<script>{runtime}</script>",
                    style.unwrap().css(),
                    html.replace("hello", "resolved")
                ))
            })
            .unwrap();
        let exported = session
            .apply_unsafe_export_html(resolved, &CancellationToken::default())
            .unwrap();
        assert!(exported.value.html.contains("resolved"));
        assert!(!exported.value.unsafe_output_used);
        assert!(exported.diagnostics.is_empty());

        let raw = open("<b>unsafe</b>\n");
        let escaped = RenderContext {
            target: HtmlTarget::Portable,
            raw_html: RawHtmlPolicy::Escape,
            ..RenderContext::default()
        };
        assert!(
            raw.prepare_safe_export(&escaped)
                .unwrap()
                .safe_html()
                .contains("&lt;b&gt;unsafe&lt;/b&gt;")
        );
        assert!(matches!(
            raw.prepare_safe_export(&portable),
            Err(EngineError::Render(RenderError::RawHtmlRejected))
        ));
    }

    #[test]
    fn render_style_rejects_a_fingerprint_that_does_not_match_its_css() {
        let mut unstyled = open("plain\n");
        let RenderPublication::Full(publication) = unstyled
            .render(
                PreviewSessionId("unstyled".into()),
                &RenderContext::default(),
            )
            .unwrap()
        else {
            panic!("first publication must be full")
        };
        assert!(serde_json::to_value(publication).unwrap()["style"].is_null());

        let style = RenderStyle::from_validated_css("body { color: red; }".to_owned());
        let wire = serde_json::to_value(&style).unwrap();
        assert_eq!(wire["css"], "body { color: red; }");
        assert_eq!(
            serde_json::from_value::<RenderStyle>(wire.clone()).unwrap(),
            style
        );
        let mut forged = wire;
        forged["fingerprint"] = "0".repeat(64).into();
        assert!(serde_json::from_value::<RenderStyle>(forged).is_err());
    }

    #[test]
    fn preprocess_mapping_clips_unicode_ranges_across_insertions_and_deletions() {
        let original = "αβ--tail";
        let output = "★αβtail";
        let map = PreprocessedSource {
            text: output.to_owned(),
            segments: vec![
                fleximark_plugin_sdk::EditMapSegment {
                    output_start: 0,
                    output_end: 3,
                    origin: EditOrigin::Generated {
                        anchor: Some(utf8_range(original, 4, 4)),
                    },
                },
                fleximark_plugin_sdk::EditMapSegment {
                    output_start: 3,
                    output_end: 11,
                    origin: EditOrigin::Original {
                        ranges: vec![utf8_range(original, 0, 4), utf8_range(original, 6, 10)],
                        primary_range_index: 1,
                    },
                },
            ],
        };
        let mapped = remap_provenance(
            &SourceProvenance::original(utf8_range(output, 5, 10)),
            &map,
            original,
        )
        .unwrap();
        let SourceProvenance::Derived {
            ranges,
            primary_range_index,
            ..
        } = mapped
        else {
            panic!("source-backed output must remain navigable")
        };
        assert_eq!(
            ranges,
            vec![utf8_range(original, 2, 4), utf8_range(original, 6, 9)]
        );
        assert_eq!(primary_range_index, 1);

        let generated = remap_provenance(
            &SourceProvenance::original(utf8_range(output, 0, 3)),
            &map,
            original,
        )
        .unwrap();
        assert_eq!(
            generated,
            SourceProvenance::Generated {
                anchor: Some(GeneratedAnchor {
                    range: utf8_range(original, 4, 4),
                    affinity: AnchorAffinity::After,
                }),
                transform: TransformId("plugin-preprocess-v1".to_owned()),
            }
        );
    }

    #[test]
    fn identity_edit_mapping_preserves_every_unicode_half_open_slice() {
        let source = "a🦀éz";
        let boundaries = (0..=source.len())
            .filter(|offset| source.is_char_boundary(*offset))
            .collect::<Vec<_>>();
        let map = PreprocessedSource {
            text: source.to_owned(),
            segments: vec![fleximark_plugin_sdk::EditMapSegment {
                output_start: 0,
                output_end: source.len() as u64,
                origin: EditOrigin::Original {
                    ranges: vec![utf8_range(source, 0, source.len())],
                    primary_range_index: 0,
                },
            }],
        };
        for &start in &boundaries {
            for &end in boundaries.iter().filter(|end| **end >= start) {
                let mapped = remap_provenance(
                    &SourceProvenance::original(utf8_range(source, start, end)),
                    &map,
                    source,
                )
                .unwrap();
                let SourceProvenance::Derived { ranges, .. } = mapped else {
                    panic!("identity mapping must be derived from original bytes")
                };
                let bytes = ranges
                    .iter()
                    .flat_map(|range| {
                        source.as_bytes()[range.byte_start as usize..range.byte_end as usize]
                            .iter()
                            .copied()
                    })
                    .collect::<Vec<_>>();
                assert_eq!(bytes, source.as_bytes()[start..end]);
            }
        }
    }

    #[test]
    fn navigation_selects_the_smallest_deepest_unicode_block_and_round_trips() {
        let session = open("- Héllo\n");
        let selected = session.node_at_source_offset(3).unwrap();
        assert!(session.node_at_source_offset(4).is_none());
        assert!(selected.depth >= 2);
        assert_eq!(selected.source_range.start.encoding, PositionEncoding::Utf8);
        assert!(selected.source_range.byte_start <= 3 && selected.source_range.byte_end >= 5);
        assert_eq!(
            session.source_range_for_node(&selected.node_id),
            Some(selected.clone())
        );
        assert_eq!(
            serde_json::to_value(&selected).unwrap()["sourceRange"]["byteStart"],
            selected.source_range.byte_start
        );
    }

    #[test]
    fn disposing_previews_evicts_their_authoritative_caches() {
        let mut session = open("# preview\n");
        for index in 0..1_000 {
            session
                .render_full(
                    PreviewSessionId(format!("preview-{index}")),
                    &RenderContext::default(),
                )
                .unwrap();
        }
        assert_eq!(session.preview_count(), 1_000);
        for index in 0..1_000 {
            assert!(session.dispose_preview(&PreviewSessionId(format!("preview-{index}"))));
        }
        assert_eq!(session.preview_count(), 0);
    }
}
