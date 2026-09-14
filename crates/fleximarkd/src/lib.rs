mod assets;
mod error;
mod export;
mod notes;
mod plugins;
mod theme;
mod uri;
mod workspace;

pub use assets::{
    ExportAsset, ResolvedExportAssets, compose_portable_html, resolve_export_assets,
    resolve_render_assets,
};
pub use error::ServiceError;
pub use export::{
    acknowledge_export, default_export_destination, export_html, export_html_with_safety,
    export_render_context, preflight_export,
};
pub use notes::{collect_admonitions, create_note, create_note_with_options, get_note_options};
pub use plugins::load_plugin_host;
pub use theme::edit_theme;
pub use uri::{document_is_in_workspace, path_to_file_uri, workspace_for_document, workspace_path};
pub use workspace::initialize_workspace;

#[cfg(test)]
mod test_support;
