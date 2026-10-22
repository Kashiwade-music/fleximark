//! Role: Native workspace lib operations and validation.
mod assets;
mod error;
mod export;
mod integrity;
mod notes;
mod paths;
mod plugins;
mod services;
mod theme;
mod uri;
mod workspace;

pub use assets::{ExportAsset, resolve_render_assets};
pub use error::ServiceError;
pub use export::{
    acknowledge_export, default_export_destination, export_document, export_html_with_safety,
};
pub use notes::{collect_admonitions, create_note, create_note_with_options, get_note_options};
pub use plugins::load_plugin_host;
pub use theme::edit_theme;
pub use uri::{document_is_in_workspace, path_to_file_uri, workspace_path};
pub use workspace::{initialize_workspace, workspace_for_document};

/// Native filesystem implementation of application workspace operations.
pub struct NativeWorkspace;

#[cfg(test)]
mod test_support;
