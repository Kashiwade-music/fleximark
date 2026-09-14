use fleximark_parser::ParseError;
use fleximark_render_html::RenderError;
use thiserror::Error;

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
