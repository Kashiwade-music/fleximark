//! Role: Define and validate the strict current workspace configuration.
use caseless::Caseless;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use unicode_normalization::UnicodeNormalization;
pub const CONFIG_SCHEMA_VERSION: u32 = 1;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlexiMarkConfig {
    pub schema_version: u32,
    #[serde(default)]
    pub notes: NotesConfig,
    #[serde(default)]
    pub assets: AssetsConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotesConfig {
    #[serde(default = "default_note_root")]
    pub root: String,
    #[serde(default)]
    pub file_name_prefix: String,
    #[serde(default)]
    pub file_name_suffix: String,
    #[serde(default)]
    pub categories: BTreeMap<String, NoteCategoryConfig>,
    #[serde(default)]
    pub templates: BTreeMap<String, Vec<String>>,
}

impl Default for NotesConfig {
    fn default() -> Self {
        Self {
            root: default_note_root(),
            file_name_prefix: String::new(),
            file_name_suffix: String::new(),
            categories: BTreeMap::new(),
            templates: BTreeMap::new(),
        }
    }
}

fn default_note_root() -> String {
    ".".to_owned()
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NoteCategoryConfig(pub BTreeMap<String, NoteCategoryConfig>);

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetsConfig {
    #[serde(default)]
    pub roots: Vec<String>,
}

impl FlexiMarkConfig {
    pub fn from_toml(source: &str) -> Result<Self, ConfigError> {
        let config: Self =
            toml::from_str(source).map_err(|error| ConfigError::Syntax(error.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ConfigError::Schema(self.schema_version));
        }
        if !is_safe_note_root(&self.notes.root) {
            return Err(ConfigError::NoteRoot(self.notes.root.clone()));
        }
        validate_note_categories(&self.notes.categories, 0)?;
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConfigError {
    Syntax(String),
    Schema(u32),
    NoteCategoryName(String),
    NoteRoot(String),
    NoteCategoryCollision(String),
    NoteCategoryDepth,
}

fn validate_note_categories(
    categories: &BTreeMap<String, NoteCategoryConfig>,
    depth: usize,
) -> Result<(), ConfigError> {
    if depth >= 32 && !categories.is_empty() {
        return Err(ConfigError::NoteCategoryDepth);
    }
    let mut filesystem_names = HashSet::new();
    for (name, category) in categories {
        let Some(filesystem_name) = note_category_filesystem_key(name) else {
            return Err(ConfigError::NoteCategoryName(name.clone()));
        };
        if !filesystem_names.insert(filesystem_name) {
            return Err(ConfigError::NoteCategoryCollision(name.clone()));
        }
        validate_note_categories(&category.0, depth + 1)?;
    }
    Ok(())
}

pub fn is_safe_note_category_name(name: &str) -> bool {
    if name.trim() != name
        || name.is_empty()
        || name.len() > 255
        || name.encode_utf16().count() > 255
        || matches!(name, "." | "..")
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|character| character.is_control() || "\\/:*?\"<>|".contains(character))
    {
        return false;
    }
    let device_stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(['.', ' '])
        .to_ascii_uppercase();
    if matches!(device_stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return false;
    }
    let device_number = device_stem
        .strip_prefix("COM")
        .or_else(|| device_stem.strip_prefix("LPT"));
    !device_number.is_some_and(|number| {
        matches!(
            number,
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        )
    })
}

pub fn is_safe_note_root(root: &str) -> bool {
    root == "."
        || (!root.is_empty()
            && !root.contains('\\')
            && root.split('/').all(is_safe_note_category_name))
}

pub fn note_category_filesystem_key(name: &str) -> Option<String> {
    is_safe_note_category_name(name).then(|| name.chars().nfd().default_case_fold().nfd().collect())
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syntax(value) => write!(f, "configuration TOML is invalid: {0}", value),
            Self::Schema(value) => {
                write!(f, "configuration schema version {0} is unsupported", value)
            }
            Self::NoteCategoryName(value) => write!(
                f,
                "note category name is not a safe directory segment: {0}",
                value
            ),
            Self::NoteRoot(value) => write!(
                f,
                "note root must be '.' or a safe workspace-relative path: {0}",
                value
            ),
            Self::NoteCategoryCollision(value) => write!(
                f,
                "note category name collides with a sibling on common filesystems: {0}",
                value
            ),
            Self::NoteCategoryDepth => f.write_str("note category nesting exceeds 32 levels"),
        }
    }
}
impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    //! Role: Verify current configuration syntax and filesystem safety.
    use super::*;
    #[test]
    fn plugins_are_unknown_fields() {
        assert!(matches!(
            FlexiMarkConfig::from_toml("schema_version=1\n[[plugins]]\nid='old'\n"),
            Err(ConfigError::Syntax(_))
        ));
    }

    #[test]
    fn config_defaults_are_secure_and_unknown_fields_are_rejected() {
        let config = FlexiMarkConfig::from_toml("schema_version = 1\n").unwrap();
        assert_eq!(config.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(config.notes, NotesConfig::default());
        assert_eq!(config.notes.root, ".");
        assert_eq!(config.assets, AssetsConfig::default());
        assert!(FlexiMarkConfig::from_toml("schema_version = 1\nlegacy = true\n").is_err());
    }

    #[test]
    fn note_root_accepts_workspace_root_and_safe_relative_directories() {
        for root in [".", "notes", "content/notes", "日本語/ノート"] {
            let source = format!("schema_version = 1\n[notes]\nroot = {root:?}\n");
            assert_eq!(
                FlexiMarkConfig::from_toml(&source).unwrap().notes.root,
                root
            );
        }
        for root in ["", "..", "../notes", "/notes", "notes\\nested", "CON"] {
            let source = format!("schema_version = 1\n[notes]\nroot = {root:?}\n");
            assert!(
                matches!(
                    FlexiMarkConfig::from_toml(&source),
                    Err(ConfigError::NoteRoot(_))
                ),
                "unsafe note root was accepted: {root:?}"
            );
        }
    }

    #[test]
    fn recursive_note_categories_accept_inline_and_dotted_maps_and_safe_names() {
        let config = FlexiMarkConfig::from_toml(
            r#"schema_version = 1

[notes.categories]
Work = { Project = {}, Reports = { Weekly = {} } }
Left = { Same = {} }
Right = { Same = {} }
"#,
        )
        .unwrap();
        assert!(config.notes.categories["Work"].0["Project"].0.is_empty());
        assert!(
            config.notes.categories["Work"].0["Reports"]
                .0
                .contains_key("Weekly")
        );
        assert!(config.notes.categories["Left"].0.contains_key("Same"));
        assert!(config.notes.categories["Right"].0.contains_key("Same"));

        let dotted = FlexiMarkConfig::from_toml(
            r#"schema_version = 1
[notes.categories.Work.Project]
"#,
        )
        .unwrap();
        assert!(dotted.notes.categories["Work"].0.contains_key("Project"));

        let traversal = r#"schema_version = 1
[notes.categories]
".." = {}
"#;
        assert_eq!(
            FlexiMarkConfig::from_toml(traversal).unwrap_err(),
            ConfigError::NoteCategoryName("..".into())
        );

        for unsafe_name in [
            "",
            "   ",
            " leading",
            ".",
            "..",
            "trailing.",
            "trailing ",
            "a/b",
            "a\\b",
            "a:b",
            "a\0b",
            "a\u{001f}b",
            "CON",
            "con.txt",
            "PRN.md",
            "AUX",
            "NUL",
            "COM1",
            "COM¹",
            "COM².txt",
            "COM³",
            "com9.log",
            "LPT1",
            "LPT¹",
            "LPT².txt",
            "LPT³",
            "lpt9.txt",
        ] {
            assert!(
                !is_safe_note_category_name(unsafe_name),
                "unsafe category was accepted: {unsafe_name:?}"
            );
        }
        for safe_name in [
            "Internal space",
            "COM0",
            "COM10",
            "LPT0",
            "LPT10",
            "Console",
            "Project.md",
        ] {
            assert!(is_safe_note_category_name(safe_name));
        }

        let ascii_collision = r#"schema_version = 1
[notes.categories]
Work = {}
work = {}
"#;
        assert_eq!(
            FlexiMarkConfig::from_toml(ascii_collision).unwrap_err(),
            ConfigError::NoteCategoryCollision("work".into())
        );
        let normalization_collision =
            "schema_version = 1\n[notes.categories]\n\"Café\" = {}\n\"Cafe\u{301}\" = {}\n";
        assert_eq!(
            FlexiMarkConfig::from_toml(normalization_collision).unwrap_err(),
            ConfigError::NoteCategoryCollision("Café".into())
        );
        let unicode_case_collision =
            "schema_version = 1\n[notes.categories]\n\"École\" = {}\n\"école\" = {}\n";
        assert_eq!(
            FlexiMarkConfig::from_toml(unicode_case_collision).unwrap_err(),
            ConfigError::NoteCategoryCollision("école".into())
        );
        for collision in [
            "schema_version = 1\n[notes.categories]\n\"Σ\" = {}\n\"ς\" = {}\n",
            "schema_version = 1\n[notes.categories]\nS = {}\n\"ſ\" = {}\n",
        ] {
            assert!(matches!(
                FlexiMarkConfig::from_toml(collision),
                Err(ConfigError::NoteCategoryCollision(_))
            ));
        }

        assert!(is_safe_note_category_name(&"a".repeat(255)));
        assert!(!is_safe_note_category_name(&"a".repeat(256)));
        assert!(is_safe_note_category_name(&"é".repeat(127)));
        assert!(!is_safe_note_category_name(&"é".repeat(128)));
        assert!(is_safe_note_category_name(&"😀".repeat(63)));
        assert!(!is_safe_note_category_name(&"😀".repeat(64)));
    }
}
