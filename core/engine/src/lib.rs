//! Role: Compose document parsing, plugin hooks, rendering, and export.
mod assets;
mod error;
mod identity;
mod provenance;
mod render;
mod session;

pub use assets::{
    AssetDiagnostic, AssetDiagnosticKind, RenderAsset, RenderConfig, RenderStyle, ResolvedAssets,
    ResolvedRenderAsset,
};
pub use error::EngineError;
pub use render::{
    PluginRenderSnapshot, PreparedExport, RenderBlock, RenderSnapshot, ResolvedExport,
};
pub use session::DocumentEngine;

#[cfg(test)]
mod plugin_tests;
#[cfg(test)]
mod tests;
