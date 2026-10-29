//! Role: Compose document parsing, stable identity, and HTML snapshots.
mod assets;
mod error;
mod identity;
mod render;
mod session;

pub use assets::{
    AssetDiagnostic, AssetDiagnosticKind, RenderAsset, RenderConfig, RenderStyle, ResolvedAssets,
    ResolvedRenderAsset,
};
pub use error::EngineError;
pub use render::{RenderBlock, RenderSnapshot};
pub use session::DocumentEngine;

#[cfg(test)]
mod tests;
