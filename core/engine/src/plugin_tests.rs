//! Role: Verify abstract plugin sequencing and transactional document updates.
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::{DocumentEngine, RenderConfig};
use fleximark_model::{Document, DocumentUri, PositionEncoding};
use fleximark_plugin_api::{
    CancellationToken, HostError, PluginExecutor, PluginFailureKind, PluginRun, PreprocessedSource,
    UnsafeExportOutput,
};
use fleximark_render_html::RenderContext;

#[derive(Default)]
struct RecordingExecutor {
    calls: Mutex<Vec<&'static str>>,
}
impl RecordingExecutor {
    fn result<T>(
        &self,
        operation: &'static str,
        value: T,
        cancellation: &CancellationToken,
    ) -> Result<PluginRun<T>, HostError> {
        self.calls.lock().unwrap().push(operation);
        if cancellation.is_cancelled() {
            return Err(HostError::RequiredPluginFailed {
                plugin_id: "test".into(),
                kind: PluginFailureKind::Cancelled,
                message: "cancelled".into(),
            });
        }
        Ok(PluginRun {
            value,
            diagnostics: Vec::new(),
        })
    }
}
impl PluginExecutor for RecordingExecutor {
    fn config_hash(&self) -> &str {
        "0000000000000000000000000000000000000000000000000000000000000000"
    }
    fn generation(&self) -> u64 {
        1
    }
    fn plugin_set_hash(&self) -> String {
        "0000000000000000000000000000000000000000000000000000000000000000".into()
    }
    fn preprocess_source(
        &self,
        _: u64,
        source: &str,
        c: &CancellationToken,
    ) -> Result<PluginRun<PreprocessedSource>, HostError> {
        self.result(
            "preprocess",
            PreprocessedSource {
                text: source.into(),
                segments: Vec::new(),
            },
            c,
        )
    }
    fn transform_blocks(
        &self,
        _: &str,
        d: &Document,
        c: &CancellationToken,
    ) -> Result<PluginRun<Document>, HostError> {
        self.result("blocks", d.clone(), c)
    }
    fn transform_document(
        &self,
        _: &str,
        d: &Document,
        c: &CancellationToken,
    ) -> Result<PluginRun<Document>, HostError> {
        self.result("document", d.clone(), c)
    }
    fn extend_render_model(
        &self,
        _: &Document,
        _: &str,
        c: &CancellationToken,
    ) -> Result<PluginRun<BTreeMap<String, String>>, HostError> {
        self.result(
            "render",
            BTreeMap::from([("key".into(), "value".into())]),
            c,
        )
    }
    fn unsafe_export_html(
        &self,
        _: u64,
        html: String,
        c: &CancellationToken,
    ) -> Result<PluginRun<UnsafeExportOutput>, HostError> {
        self.result(
            "export",
            UnsafeExportOutput {
                html,
                unsafe_output_used: false,
            },
            c,
        )
    }
}

#[test]
fn abstract_hooks_preserve_pipeline_order_and_rollback_failed_changes() {
    let executor = Arc::new(RecordingExecutor::default());
    let config = RenderConfig::for_plugins(RenderContext::default(), None, executor.as_ref());
    let mut engine = DocumentEngine::open_configured(
        DocumentUri("file:///test.md".into()),
        1,
        "before\n".into(),
        PositionEncoding::Utf8,
        config,
        executor.clone(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(
        *executor.calls.lock().unwrap(),
        ["preprocess", "blocks", "document"]
    );
    let original = engine.document().clone();
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(
        engine
            .change_full_text_with_cancellation(2, "after\n".into(), &cancelled)
            .is_err()
    );
    assert_eq!(engine.source(), "before\n");
    assert_eq!(engine.document(), &original);
    assert!(!engine.is_out_of_sync());
    let snapshot = engine
        .render_configured(&CancellationToken::default())
        .unwrap()
        .snapshot;
    assert_eq!(snapshot.annotations["key"], "value");
    assert!(engine.render_configured(&cancelled).is_err());
}

#[test]
fn render_only_reconfiguration_preserves_document_and_hook_history() {
    let executor = Arc::new(RecordingExecutor::default());
    let config = RenderConfig::for_plugins(RenderContext::default(), None, executor.as_ref());
    let mut engine = DocumentEngine::open_configured(
        DocumentUri("file:///test.md".into()),
        1,
        "before\n".into(),
        PositionEncoding::Utf8,
        config.clone(),
        executor.clone(),
        &CancellationToken::default(),
    )
    .unwrap();
    let document = engine.document().clone();
    let calls = executor.calls.lock().unwrap().clone();
    engine.reconfigure_render(config).unwrap();
    assert_eq!(engine.document(), &document);
    assert_eq!(*executor.calls.lock().unwrap(), calls);
}
