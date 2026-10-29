//! Role: Native workspace lib operations and validation.
pub(crate) mod assets;
mod export;
mod integrity;
mod notes;
mod open;
mod paths;
mod theme;
mod uri;
pub(crate) mod workspace;

use crate::ApplicationError;
pub use assets::resolve_render_assets;
pub use export::{acknowledge_export, default_export_destination, export_document};
#[cfg(test)]
pub use notes::create_note;
pub use notes::{collect_admonitions, create_note_with_options, get_note_options};
#[cfg(test)]
pub use open::open_document;
pub use open::refresh_document_assets;
#[cfg(test)]
pub use theme::configured_render_config;
pub use theme::edit_theme;
pub(crate) use theme::load_configuration;
pub(crate) use uri::canonical_document_path;
pub use uri::{path_to_file_uri, workspace_path};
pub use workspace::{initialize_workspace, workspace_for_document};

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
pub use export::export_html;
