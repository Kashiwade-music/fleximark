//! Role: Adapt verified Wasmtime execution to the engine's abstract plugin port.
use crate::PluginHost;
use fleximark_model::Document;
use fleximark_plugin_api as api;
use std::collections::BTreeMap;

impl api::PluginExecutor for PluginHost {
    fn config_hash(&self) -> &str {
        PluginHost::config_hash(self)
    }
    fn generation(&self) -> u64 {
        PluginHost::generation(self)
    }
    fn plugin_set_hash(&self) -> String {
        PluginHost::plugin_set_hash(self)
    }
    fn preprocess_source(
        &self,
        version: u64,
        source: &str,
        cancellation: &api::CancellationToken,
    ) -> Result<api::PluginRun<api::PreprocessedSource>, api::HostError> {
        PluginHost::preprocess_source(self, version, source, cancellation)
    }
    fn transform_blocks(
        &self,
        source: &str,
        document: &Document,
        cancellation: &api::CancellationToken,
    ) -> Result<api::PluginRun<Document>, api::HostError> {
        PluginHost::transform_blocks(self, source, document, cancellation)
    }
    fn transform_document(
        &self,
        source: &str,
        document: &Document,
        cancellation: &api::CancellationToken,
    ) -> Result<api::PluginRun<Document>, api::HostError> {
        PluginHost::transform_document(self, source, document, cancellation)
    }
    fn extend_render_model(
        &self,
        document: &Document,
        target: &str,
        cancellation: &api::CancellationToken,
    ) -> Result<api::PluginRun<BTreeMap<String, String>>, api::HostError> {
        PluginHost::extend_render_model(self, document, target, cancellation)
    }
    fn unsafe_export_html(
        &self,
        version: u64,
        html: String,
        cancellation: &api::CancellationToken,
    ) -> Result<api::PluginRun<api::UnsafeExportOutput>, api::HostError> {
        PluginHost::unsafe_export_html(self, version, html, cancellation)
    }
}
