//! Role: Define HTML contexts, results, errors and block metadata.

use fleximark_model::{NavigationEntry, NodeId};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HtmlTarget {
    Preview,
    Portable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderContext {
    pub target: HtmlTarget,
    pub allow_remote_resources: bool,
    pub allow_data_resources: bool,
    /// Canonical source spelling to an opaque, already-validated asset reference.
    pub resolved_resources: BTreeMap<String, String>,
}

impl Default for RenderContext {
    fn default() -> Self {
        Self {
            target: HtmlTarget::Preview,
            allow_remote_resources: false,
            allow_data_resources: false,
            resolved_resources: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedBlock {
    pub id: NodeId,
    pub node_ids: Vec<NodeId>,
    pub navigation: Vec<NavigationEntry>,
    pub html: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RenderError {
    DuplicateNodeId(String),
    InvalidRawHtmlBoundary,
    ResourceRejected(String),
}
impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateNodeId(value) => {
                write!(f, "duplicate NodeId at render boundary: {0}", value)
            }
            Self::InvalidRawHtmlBoundary => {
                write!(f, "raw HTML sequence lost its internal boundary marker")
            }
            Self::ResourceRejected(value) => write!(
                f,
                "resource URL is rejected by the target policy: {0}",
                value
            ),
        }
    }
}
impl std::error::Error for RenderError {}

pub(crate) struct BlockMetadata {
    pub(crate) node_ids: Vec<NodeId>,
    pub(crate) navigation: Vec<NavigationEntry>,
}
