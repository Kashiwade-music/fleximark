//! Role: Define native workspace operation outcomes and errors.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitializedWorkspace {
    pub config_uri: String,
    pub theme_uri: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreatedNote {
    pub uri: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectedAdmonitions {
    pub uri: String,
    pub count: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeResource {
    pub uri: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportReceipt {
    pub output_uri: String,
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteCategory {
    pub name: String,
    pub children: Vec<NoteCategory>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteOptions {
    pub categories: Vec<NoteCategory>,
    pub templates: Vec<String>,
}

pub struct MigrationOutput {
    pub stdout: String,
}
