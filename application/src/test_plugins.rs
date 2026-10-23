//! Role: Provide deterministic abstract plugin execution for application tests.
use fleximark_model::Document;
use fleximark_plugin_api::{
    CancellationToken, HostError, PluginExecutor, PluginRun, PreprocessedSource, UnsafeExportOutput,
};
use std::collections::BTreeMap;

pub(crate) struct TestPlugins {
    pub hash: String,
    pub calls: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    pub marker: Option<String>,
}

impl PluginExecutor for TestPlugins {
    fn config_hash(&self) -> &str {
        &self.hash
    }
    fn generation(&self) -> u64 {
        1
    }
    fn plugin_set_hash(&self) -> String {
        self.hash.clone()
    }
    fn preprocess_source(
        &self,
        _: u64,
        source: &str,
        _: &CancellationToken,
    ) -> Result<PluginRun<PreprocessedSource>, HostError> {
        if let Some(calls) = &self.calls {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(PluginRun {
            value: PreprocessedSource {
                text: source.into(),
                segments: vec![],
            },
            diagnostics: vec![],
        })
    }
    fn transform_blocks(
        &self,
        _: &str,
        doc: &Document,
        _: &CancellationToken,
    ) -> Result<PluginRun<Document>, HostError> {
        Ok(PluginRun {
            value: doc.clone(),
            diagnostics: vec![],
        })
    }
    fn transform_document(
        &self,
        _: &str,
        doc: &Document,
        _: &CancellationToken,
    ) -> Result<PluginRun<Document>, HostError> {
        let mut doc = doc.clone();
        if let Some(marker) = &self.marker {
            doc.metadata.front_matter = Some(marker.clone());
        }
        Ok(PluginRun {
            value: doc,
            diagnostics: vec![],
        })
    }
    fn extend_render_model(
        &self,
        _: &Document,
        _: &str,
        _: &CancellationToken,
    ) -> Result<PluginRun<BTreeMap<String, String>>, HostError> {
        Ok(PluginRun {
            value: BTreeMap::new(),
            diagnostics: vec![],
        })
    }
    fn unsafe_export_html(
        &self,
        _: u64,
        html: String,
        _: &CancellationToken,
    ) -> Result<PluginRun<UnsafeExportOutput>, HostError> {
        Ok(PluginRun {
            value: UnsafeExportOutput {
                html,
                unsafe_output_used: false,
            },
            diagnostics: vec![],
        })
    }
}
