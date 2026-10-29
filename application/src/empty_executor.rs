//! Role: Bridge the engine while plugin hooks are removed.
use fleximark_model::Document;
use fleximark_plugin_api::{
    CancellationToken, HostError, PluginExecutor, PluginFailureKind, PluginRun, PreprocessedSource,
    UnsafeExportOutput,
};
use std::collections::BTreeMap;

pub(crate) struct EmptyPluginExecutor(pub fleximark_engine::RenderConfig);

impl PluginExecutor for EmptyPluginExecutor {
    fn config_hash(&self) -> &str {
        &self.0.config_hash
    }
    fn generation(&self) -> u64 {
        self.0.config_generation
    }
    fn plugin_set_hash(&self) -> String {
        self.0.plugin_set_hash.clone()
    }
    fn preprocess_source(
        &self,
        _: u64,
        source: &str,
        cancellation: &CancellationToken,
    ) -> Result<PluginRun<PreprocessedSource>, HostError> {
        check(cancellation)?;
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
        cancellation: &CancellationToken,
    ) -> Result<PluginRun<Document>, HostError> {
        check(cancellation)?;
        Ok(PluginRun {
            value: doc.clone(),
            diagnostics: vec![],
        })
    }
    fn transform_document(
        &self,
        _: &str,
        doc: &Document,
        cancellation: &CancellationToken,
    ) -> Result<PluginRun<Document>, HostError> {
        #[cfg(test)]
        crate::cancellation_checks::checkpoint("after_parse");
        let doc = doc.clone();
        check(cancellation)?;
        Ok(PluginRun {
            value: doc,
            diagnostics: vec![],
        })
    }
    fn extend_render_model(
        &self,
        _: &Document,
        _: &str,
        cancellation: &CancellationToken,
    ) -> Result<PluginRun<BTreeMap<String, String>>, HostError> {
        check(cancellation)?;
        Ok(PluginRun {
            value: BTreeMap::new(),
            diagnostics: vec![],
        })
    }
    fn unsafe_export_html(
        &self,
        _: u64,
        html: String,
        cancellation: &CancellationToken,
    ) -> Result<PluginRun<UnsafeExportOutput>, HostError> {
        check(cancellation)?;
        Ok(PluginRun {
            value: UnsafeExportOutput {
                html,
                unsafe_output_used: false,
            },
            diagnostics: vec![],
        })
    }
}

fn check(token: &CancellationToken) -> Result<(), HostError> {
    if token.is_cancelled() {
        Err(HostError::RequiredPluginFailed {
            plugin_id: "internal".into(),
            kind: PluginFailureKind::Cancelled,
            message: "operation cancelled".into(),
        })
    } else {
        Ok(())
    }
}
