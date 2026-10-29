//! Role: Install validated document updates atomically.
use crate::assets::{AssetDiagnostic, RenderConfig};
use crate::error::EngineError;
use crate::identity::{content_hash, reconcile_node_ids};
use fleximark_model::{Document, DocumentUri, NavigationEntry, PositionEncoding};
use fleximark_parser::parse;
use std::sync::Arc;

#[derive(Clone)]
pub struct DocumentEngine {
    pub(super) source: Arc<str>,
    pub(super) position_encoding: PositionEncoding,
    pub(super) document: Arc<Document>,
    pub(super) out_of_sync: bool,
    pub(super) render_config: Arc<RenderConfig>,
}

impl DocumentEngine {
    pub fn open(
        uri: DocumentUri,
        version: u64,
        source: String,
        position_encoding: PositionEncoding,
        render_config: RenderConfig,
    ) -> Result<Self, EngineError> {
        render_config.validate()?;
        let mut document = parse(uri, version, &source)?;
        reconcile_node_ids(None, &mut document);
        Ok(Self {
            source: source.into(),
            position_encoding,
            document: Arc::new(document),
            out_of_sync: false,
            render_config: Arc::new(render_config),
        })
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

    pub fn set_render_config(&mut self, config: RenderConfig) -> Result<(), EngineError> {
        config.validate()?;
        self.render_config = Arc::new(config);
        Ok(())
    }

    pub fn change(&mut self, version: u64, source: String) -> Result<(), EngineError> {
        if version <= self.document.document_version {
            self.mark_out_of_sync();
            return Err(EngineError::StaleVersion {
                current: self.document.document_version,
                received: version,
            });
        }
        if self.out_of_sync {
            return Err(EngineError::ContentModified);
        }
        self.install_source(version, source)
    }

    pub fn resync(&mut self, version: u64, source: String) -> Result<(), EngineError> {
        if version < self.document.document_version
            || (!self.out_of_sync && version == self.document.document_version)
        {
            return Err(EngineError::StaleVersion {
                current: self.document.document_version,
                received: version,
            });
        }
        self.install_source(version, source)?;
        self.out_of_sync = false;
        Ok(())
    }

    pub fn mark_out_of_sync(&mut self) {
        self.out_of_sync = true;
    }

    fn install_source(&mut self, version: u64, source: String) -> Result<(), EngineError> {
        let mut document = parse(self.document.uri.clone(), version, &source)?;
        reconcile_node_ids(Some(&self.document), &mut document);
        self.source = source.into();
        self.document = Arc::new(document);
        Ok(())
    }
}
