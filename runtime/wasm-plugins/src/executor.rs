//! Role: Adapt verified Wasmtime execution to the engine's abstract plugin port.
use crate::{HostError, PluginHost, PluginRun};
use fleximark_model::Document;
use fleximark_plugin_api as api;
use std::collections::BTreeMap;

fn error(value: HostError) -> api::HostError {
    match value {
        HostError::DuplicatePlugin(v) => api::HostError::DuplicatePlugin(v),
        HostError::InvalidManifest(v) => api::HostError::InvalidManifest(v),
        HostError::RuntimeLoad(v) => api::HostError::RuntimeLoad(v),
        HostError::Integrity(v) => api::HostError::Integrity(v),
        HostError::RequiredPluginFailed {
            plugin_id,
            kind,
            message,
        } => api::HostError::RequiredPluginFailed {
            plugin_id,
            kind: failure_kind(kind),
            message,
        },
    }
}
fn failure_kind(value: crate::PluginFailureKind) -> api::PluginFailureKind {
    use crate::PluginFailureKind as K;
    match value {
        K::Timeout => api::PluginFailureKind::Timeout,
        K::MemoryLimit => api::PluginFailureKind::MemoryLimit,
        K::OutputLimit => api::PluginFailureKind::OutputLimit,
        K::Cancelled => api::PluginFailureKind::Cancelled,
        K::Trap => api::PluginFailureKind::Trap,
        K::MalformedOutput => api::PluginFailureKind::MalformedOutput,
        K::InvalidCandidate => api::PluginFailureKind::InvalidCandidate,
        K::PolicyViolation => api::PluginFailureKind::PolicyViolation,
    }
}
fn run<T, U>(
    value: Result<PluginRun<T>, HostError>,
    convert: impl FnOnce(T) -> U,
) -> Result<api::PluginRun<U>, api::HostError> {
    let value = value.map_err(error)?;
    Ok(api::PluginRun {
        value: convert(value.value),
        diagnostics: value
            .diagnostics
            .into_iter()
            .map(|d| api::PluginDiagnostic {
                plugin_id: d.plugin_id,
                message: d.message,
                kind: failure_kind(d.kind),
                hook: match d.hook {
                    fleximark_plugin_sdk::Hook::PreprocessSource => api::Hook::PreprocessSource,
                    fleximark_plugin_sdk::Hook::TransformDocument => api::Hook::TransformDocument,
                    fleximark_plugin_sdk::Hook::TransformBlock => api::Hook::TransformBlock,
                    fleximark_plugin_sdk::Hook::ExtendRenderModel => api::Hook::ExtendRenderModel,
                    fleximark_plugin_sdk::Hook::UnsafeExportHtml => api::Hook::UnsafeExportHtml,
                },
            })
            .collect(),
    })
}
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
        run(
            PluginHost::preprocess_source(self, version, source, cancellation),
            |v| api::PreprocessedSource {
                text: v.text,
                segments: v
                    .segments
                    .into_iter()
                    .map(|s| api::EditMapSegment {
                        output_start: s.output_start,
                        output_end: s.output_end,
                        origin: match s.origin {
                            fleximark_plugin_sdk::EditOrigin::Original {
                                ranges,
                                primary_range_index,
                            } => api::EditOrigin::Original {
                                ranges,
                                primary_range_index,
                            },
                            fleximark_plugin_sdk::EditOrigin::Derived {
                                ranges,
                                primary_range_index,
                            } => api::EditOrigin::Derived {
                                ranges,
                                primary_range_index,
                            },
                            fleximark_plugin_sdk::EditOrigin::Generated { anchor } => {
                                api::EditOrigin::Generated { anchor }
                            }
                        },
                    })
                    .collect(),
            },
        )
    }
    fn transform_blocks(
        &self,
        source: &str,
        document: &Document,
        cancellation: &api::CancellationToken,
    ) -> Result<api::PluginRun<Document>, api::HostError> {
        run(
            PluginHost::transform_blocks(self, source, document, cancellation),
            |v| v,
        )
    }
    fn transform_document(
        &self,
        source: &str,
        document: &Document,
        cancellation: &api::CancellationToken,
    ) -> Result<api::PluginRun<Document>, api::HostError> {
        run(
            PluginHost::transform_document(self, source, document, cancellation),
            |v| v,
        )
    }
    fn extend_render_model(
        &self,
        document: &Document,
        target: &str,
        cancellation: &api::CancellationToken,
    ) -> Result<api::PluginRun<BTreeMap<String, String>>, api::HostError> {
        run(
            PluginHost::extend_render_model(self, document, target, cancellation),
            |v| v,
        )
    }
    fn unsafe_export_html(
        &self,
        version: u64,
        html: String,
        cancellation: &api::CancellationToken,
    ) -> Result<api::PluginRun<api::UnsafeExportOutput>, api::HostError> {
        run(
            PluginHost::unsafe_export_html(self, version, html, cancellation),
            |v| api::UnsafeExportOutput {
                html: v.html,
                unsafe_output_used: v.unsafe_output_used,
            },
        )
    }
}
