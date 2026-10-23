//! Role: Define document processing and rendering failure outcomes.
use fleximark_parser::ParseError;
use fleximark_render_html::RenderError;

#[derive(Debug)]
pub enum EngineError {
    Parse(ParseError),
    Render(RenderError),
    StaleVersion { current: u64, received: u64 },
    ContentModified,
    Plugin(String),
    UnsafeExportPolicy,
    Asset(String),
}
impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(value) => std::fmt::Display::fmt(value, f),
            Self::Render(value) => std::fmt::Display::fmt(value, f),
            Self::StaleVersion { current, received } => {
                write!(f, "document version {received} is not newer than {current}")
            }
            Self::ContentModified => write!(
                f,
                "the document session is out of sync and requires full text"
            ),
            Self::Plugin(value) => write!(f, "plugin pipeline failed: {0}", value),
            Self::UnsafeExportPolicy => write!(
                f,
                "unsafe plugin HTML is available only after a safe portable render"
            ),
            Self::Asset(value) => write!(f, "invalid resolved render asset: {0}", value),
        }
    }
}
impl std::error::Error for EngineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(value) => std::error::Error::source(value),
            Self::Render(value) => std::error::Error::source(value),
            _ => None,
        }
    }
}
impl From<ParseError> for EngineError {
    fn from(value: ParseError) -> Self {
        Self::Parse(value)
    }
}
impl From<RenderError> for EngineError {
    fn from(value: RenderError) -> Self {
        Self::Render(value)
    }
}
