//! Role: Expose the editor-independent document and syntax model APIs.
mod nodes;
pub mod syntax;
mod traversal;
mod validation;
mod values;

pub use nodes::{
    Block, BlockKind, Document, DocumentMetadata, DocumentUri, Inline, InlineKind, NavigationEntry,
    Node, NodeId, PositionEncoding, SourceIndex, SourcePosition, SourceRange, ValidationError,
};
pub use traversal::{
    Visitor, VisitorMut, collect_navigation, walk_blocks, walk_blocks_mut, walk_inlines,
};
pub use values::{AssetRef, InvalidValue, Sha256Hex};

#[cfg(test)]
mod tests;
