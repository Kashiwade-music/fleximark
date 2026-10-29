//! Role: Native workspace lib operations and validation.
mod assets;
mod export;
mod integrity;
mod notes;
mod open;
mod paths;
mod services;
mod theme;
mod uri;
mod workspace;

use crate::ApplicationError;
pub use assets::{ExportAsset, resolve_render_assets};
pub use export::{acknowledge_export, default_export_destination, export_document, export_html};
pub use notes::{collect_admonitions, create_note, create_note_with_options, get_note_options};
pub use open::{open_document, refresh_document_assets};
pub use theme::configured_render_config;
pub use theme::edit_theme;
pub(crate) use uri::canonical_document_path;
pub use uri::{document_is_in_workspace, path_to_file_uri, workspace_path};
pub use workspace::{initialize_workspace, workspace_for_document};

pub use services::{migrate_workspace, same_workspace};

#[cfg(test)]
pub(crate) mod test_support;
