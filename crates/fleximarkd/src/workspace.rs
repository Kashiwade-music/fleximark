use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{ambient_authority, fs::Dir};
use fleximark_plugin_sdk::{
    AssetsConfig, FlexiMarkConfig, NoteCategoryConfig, NotesConfig, note_category_filesystem_key,
};
use fleximark_protocol::{CommandMessage, CommandResult};
use serde_json::Value;

use crate::{ServiceError, path_to_file_uri, workspace_path};

pub(crate) const CONFIG: &str =
    "# FlexiMark workspace configuration\nschema_version = 2\n\n[notes]\nroot = \"notes\"\n";
const THEME: &str = "/* FlexiMark workspace theme */\n:root { color-scheme: light dark; }\n";

pub fn initialize_workspace(workspace_uri: &str) -> Result<CommandResult, ServiceError> {
    let root = workspace_path(workspace_uri)?;
    let control = open_control_directory(&root, true)?.expect("created control directory");
    write_new_at(&control, "config.toml", CONFIG)?;
    validate_config(&root.join(".fleximark/config.toml"))?;
    write_new_at(&control, "theme.css", THEME)?;
    let config = root.join(".fleximark/config.toml");
    Ok(CommandResult {
        message: Some(CommandMessage {
            level: fleximark_protocol::CommandMessageLevel::Info,
            text: "Initialized .fleximark/config.toml and .fleximark/theme.css".into(),
        }),
        open_uri: Some(path_to_file_uri(&config)?),
        data: None,
    })
}

pub fn inspect_legacy_workspace(workspace_uri: &str) -> Result<Option<bool>, ServiceError> {
    let root = workspace_path(workspace_uri)?;
    let Some(control) = open_control_directory(&root, false)? else {
        return Ok(None);
    };
    if path_exists(&control, "config.toml")? || !regular_file_exists(&control, "fleximark.json")? {
        return Ok(None);
    }
    Ok(Some(regular_file_exists(&control, "parserPlugin.js")?))
}

pub fn migrate_legacy_workspace(
    workspace_uri: &str,
    settings: &Value,
) -> Result<CommandResult, ServiceError> {
    let settings = settings.as_object().ok_or(ServiceError::InvalidConfig)?;
    let root = workspace_path(workspace_uri)?;
    let control = open_control_directory(&root, false)?.ok_or(ServiceError::NotInitialized)?;
    if path_exists(&control, "config.toml")? {
        return Ok(empty_command_result());
    }
    if !regular_file_exists(&control, "fleximark.json")? {
        return Err(ServiceError::NotInitialized);
    }

    let config = FlexiMarkConfig {
        schema_version: 2,
        notes: NotesConfig {
            root: ".".to_owned(),
            file_name_prefix: legacy_string(settings.get("noteFileNamePrefix")),
            file_name_suffix: legacy_string(settings.get("noteFileNameSuffix")),
            categories: legacy_categories(settings.get("noteCategories"))?,
            templates: legacy_templates(settings.get("noteTemplates")),
        },
        assets: AssetsConfig {
            roots: root
                .join("attachments")
                .symlink_metadata()
                .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
                .then(|| "attachments".to_owned())
                .into_iter()
                .collect(),
        },
        plugins: Vec::new(),
    };
    config.validate().map_err(|_| ServiceError::InvalidConfig)?;
    let config = migrated_config_toml(&config)?;

    if !path_exists(&control, "theme.css")? {
        if regular_file_exists(&control, "fleximark.css")? {
            let mut theme = Vec::new();
            open_regular_nofollow(&control, Path::new("fleximark.css"))?.read_to_end(&mut theme)?;
            write_new_bytes_at(&control, "theme.css", &theme)?;
        } else {
            write_new_at(&control, "theme.css", THEME)?;
        }
    }
    write_new_at(&control, "config.toml", &config)?;
    Ok(empty_command_result())
}

fn empty_command_result() -> CommandResult {
    CommandResult {
        message: None,
        open_uri: None,
        data: None,
    }
}

fn legacy_string(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn legacy_categories(
    value: Option<&Value>,
) -> Result<BTreeMap<String, NoteCategoryConfig>, ServiceError> {
    fn convert(
        entries: &serde_json::Map<String, Value>,
    ) -> Result<BTreeMap<String, NoteCategoryConfig>, ServiceError> {
        let mut filesystem_names = HashSet::new();
        let mut categories = BTreeMap::new();
        for (name, child) in entries {
            let filesystem_name =
                note_category_filesystem_key(name).ok_or(ServiceError::InvalidConfig)?;
            if !filesystem_names.insert(filesystem_name) {
                return Err(ServiceError::InvalidConfig);
            }
            categories.insert(
                name.clone(),
                NoteCategoryConfig(match child.as_object() {
                    Some(children) => convert(children)?,
                    None => BTreeMap::new(),
                }),
            );
        }
        Ok(categories)
    }

    Ok(value
        .and_then(Value::as_object)
        .map(convert)
        .transpose()?
        .unwrap_or_default())
}

fn legacy_templates(value: Option<&Value>) -> BTreeMap<String, Vec<String>> {
    value
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|templates| templates.iter())
        .filter_map(|(name, lines)| {
            (!name.is_empty()).then_some(())?;
            let lines = lines
                .as_array()?
                .iter()
                .map(Value::as_str)
                .collect::<Option<Vec<_>>>()?;
            Some((name.clone(), lines.into_iter().map(str::to_owned).collect()))
        })
        .collect()
}

fn migrated_config_toml(config: &FlexiMarkConfig) -> Result<String, ServiceError> {
    let mut base = config.clone();
    base.notes.categories.clear();
    let base = toml::to_string_pretty(&base).map_err(|_| ServiceError::InvalidConfig)?;
    let marker = "[notes.categories]\n";
    if !base.contains(marker) {
        return Err(ServiceError::InvalidConfig);
    }
    let mut categories = marker.to_owned();
    for (name, children) in &config.notes.categories {
        categories.push_str(&toml_value(name)?);
        categories.push_str(" = ");
        write_category_inline_table(&mut categories, &children.0, 0)?;
        categories.push('\n');
    }
    Ok(format!(
        "# FlexiMark workspace configuration\n# Migrated from the legacy VS Code workspace format.\n{}",
        base.replacen(marker, &categories, 1)
    ))
}

fn write_category_inline_table(
    output: &mut String,
    categories: &BTreeMap<String, NoteCategoryConfig>,
    indent: usize,
) -> Result<(), ServiceError> {
    if categories.is_empty() {
        output.push_str("{}");
        return Ok(());
    }
    output.push_str("{\n");
    for (name, children) in categories {
        output.push_str(&" ".repeat(indent + 2));
        output.push_str(&toml_value(name)?);
        output.push_str(" = ");
        write_category_inline_table(output, &children.0, indent + 2)?;
        output.push_str(",\n");
    }
    output.push_str(&" ".repeat(indent));
    output.push('}');
    Ok(())
}

fn toml_value<T: serde::Serialize + ?Sized>(value: &T) -> Result<String, ServiceError> {
    let mut output = String::new();
    value
        .serialize(toml::ser::ValueSerializer::new(&mut output))
        .map_err(|_| ServiceError::InvalidConfig)?;
    Ok(output)
}

fn path_exists(directory: &Dir, path: &str) -> Result<bool, ServiceError> {
    match directory.symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn regular_file_exists(directory: &Dir, path: &str) -> Result<bool, ServiceError> {
    match directory.symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.is_file() && !metadata.file_type().is_symlink()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn safe_workspace_relative_path(value: &str) -> bool {
    !value.is_empty()
        && Path::new(value)
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

pub(crate) fn open_control_directory(
    root: &Path,
    create: bool,
) -> Result<Option<Dir>, ServiceError> {
    let workspace = Dir::open_ambient_dir(root, ambient_authority())?;
    match workspace.open_dir_nofollow(".fleximark") {
        Ok(control) => Ok(Some(control)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
            workspace.create_dir(".fleximark")?;
            workspace
                .open_dir_nofollow(".fleximark")
                .map(Some)
                .map_err(ServiceError::Io)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(ServiceError::LinkedControlDirectory),
    }
}

fn write_new_at(directory: &Dir, path: &str, contents: &str) -> Result<(), ServiceError> {
    write_new_bytes_at(directory, path, contents.as_bytes())
}

fn write_new_bytes_at(directory: &Dir, path: &str, contents: &[u8]) -> Result<(), ServiceError> {
    let mut options = cap_std::fs::OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    match directory.open_with(path, &options) {
        Ok(mut file) => {
            file.write_all(contents)?;
            file.sync_all()?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = directory.symlink_metadata(path)?;
            if metadata.is_file() && !metadata.file_type().is_symlink() {
                Ok(())
            } else {
                Err(ServiceError::InvalidControlPath)
            }
        }
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn open_regular_nofollow(
    directory: &Dir,
    path: &Path,
) -> std::io::Result<cap_std::fs::File> {
    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = directory.open_with(path, &options)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "path is not a regular file",
        ));
    }
    Ok(file)
}

pub(crate) fn reject_link(path: &Path) -> Result<(), ServiceError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(ServiceError::InvalidControlPath)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn validate_config(path: &Path) -> Result<FlexiMarkConfig, ServiceError> {
    let control_path = path.parent().ok_or(ServiceError::InvalidConfig)?;
    let root = control_path.parent().ok_or(ServiceError::InvalidConfig)?;
    let control = open_control_directory(root, false)?.ok_or(ServiceError::NotInitialized)?;
    let mut file = open_regular_nofollow(&control, Path::new("config.toml")).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ServiceError::NotInitialized
        } else {
            ServiceError::InvalidConfig
        }
    })?;
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|_| ServiceError::InvalidConfig)?;
    FlexiMarkConfig::from_toml(&text).map_err(|_| ServiceError::InvalidConfig)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_workspace;
    use crate::{create_note, get_note_options, load_plugin_host};
    #[test]
    fn linked_control_directory_never_reads_or_writes_outside_the_workspace() {
        let root = test_workspace("linked-control-test");
        let outside = test_workspace("linked-control-outside");
        fs::write(outside.join("sentinel"), "unchanged").unwrap();
        fs::write(outside.join("config.toml"), CONFIG).unwrap();
        let linked = root.join(".fleximark");
        #[cfg(unix)]
        let link_result = std::os::unix::fs::symlink(&outside, &linked);
        #[cfg(windows)]
        let link_result = std::os::windows::fs::symlink_dir(&outside, &linked);
        if link_result.is_err() {
            fs::remove_dir_all(root).unwrap();
            fs::remove_dir_all(outside).unwrap();
            return;
        }
        let uri = path_to_file_uri(&root).unwrap();
        assert!(matches!(
            initialize_workspace(&uri),
            Err(ServiceError::LinkedControlDirectory)
        ));
        assert!(matches!(
            get_note_options(&uri),
            Err(ServiceError::LinkedControlDirectory)
        ));
        assert!(matches!(
            load_plugin_host(&uri, false, 1),
            Err(ServiceError::LinkedControlDirectory)
        ));
        assert_eq!(
            fs::read_to_string(outside.join("sentinel")).unwrap(),
            "unchanged"
        );
        assert!(!outside.join("theme.css").exists());
        fs::remove_file(&linked).unwrap_or_else(|_| fs::remove_dir(&linked).unwrap());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn old_workspace_layout_is_unsupported_and_never_loaded() {
        let root = test_workspace("old-layout-unsupported");
        let control = root.join(".fleximark");
        fs::create_dir(&control).unwrap();
        fs::write(control.join("fleximark.json"), "not valid json").unwrap();
        fs::write(
            control.join("parserPlugin.js"),
            "throw new Error('executed')",
        )
        .unwrap();
        fs::write(
            control.join("fleximark.css"),
            "@import 'https://example.test'",
        )
        .unwrap();
        let uri = path_to_file_uri(&root).unwrap();
        let (_, config) = load_plugin_host(&uri, true, 1).unwrap();
        assert!(config.style.is_none());
        initialize_workspace(&uri).unwrap();
        assert!(control.join("config.toml").is_file());
        assert!(control.join("theme.css").is_file());
        assert_eq!(
            fs::read_to_string(control.join("parserPlugin.js")).unwrap(),
            "throw new Error('executed')"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_migration_is_rust_owned_idempotent_and_preserves_legacy_files() {
        let root = test_workspace("legacy-migration");
        let control = root.join(".fleximark");
        fs::create_dir(&control).unwrap();
        fs::create_dir(root.join("attachments")).unwrap();
        fs::write(control.join("fleximark.json"), "{}").unwrap();
        fs::write(control.join("fleximark.css"), "body { color: purple; }").unwrap();
        fs::write(control.join("parserPlugin.js"), "module.exports = {};").unwrap();
        let uri = path_to_file_uri(&root).unwrap();

        assert_eq!(inspect_legacy_workspace(&uri).unwrap(), Some(true));
        migrate_legacy_workspace(
            &uri,
            &serde_json::json!({
                "noteFileNamePrefix": "${CURRENT_YEAR}_",
                "noteFileNameSuffix": 42,
                "noteCategories": {
                    "General": { "Reports": { "Weekly": {} } },
                    "Project #1": { "Plan = A": {} }
                },
                "noteTemplates": {
                    "default": ["# ${1:Title}", "Created ${CURRENT_DATE}"],
                    "invalid": "not an array"
                },
                "unknown": true
            }),
        )
        .unwrap();

        let config = validate_config(&control.join("config.toml")).unwrap();
        let config_source = fs::read_to_string(control.join("config.toml")).unwrap();
        assert!(config_source.contains(
            "[notes.categories]\n\"General\" = {\n  \"Reports\" = {\n    \"Weekly\" = {},\n  },\n}"
        ));
        assert!(config_source.contains("\"Project #1\" = {\n  \"Plan = A\" = {},\n}"));
        assert!(!config_source.contains("[notes.categories.General"));
        assert_eq!(config.notes.file_name_prefix, "${CURRENT_YEAR}_");
        assert_eq!(config.notes.file_name_suffix, "");
        let general = &config.notes.categories["General"];
        let reports = &general.0["Reports"];
        assert!(reports.0["Weekly"].0.is_empty());
        assert!(
            config.notes.categories["Project #1"].0["Plan = A"]
                .0
                .is_empty()
        );
        let weekly = [
            "General".to_owned(),
            "Reports".to_owned(),
            "Weekly".to_owned(),
        ];
        assert_eq!(config.notes.root, ".");
        let migrated_note =
            crate::create_note_with_options(&uri, Some(&weekly), None, Some("weekly")).unwrap();
        let migrated_note = workspace_path(migrated_note.open_uri.as_deref().unwrap()).unwrap();
        assert_eq!(
            migrated_note.parent().unwrap(),
            root.join("General/Reports/Weekly").canonicalize().unwrap()
        );
        assert_eq!(config.assets.roots, ["attachments"]);
        assert_eq!(
            fs::read_to_string(control.join("theme.css")).unwrap(),
            "body { color: purple; }"
        );
        assert!(control.join("fleximark.json").is_file());
        assert!(control.join("parserPlugin.js").is_file());
        assert_eq!(inspect_legacy_workspace(&uri).unwrap(), None);

        let first_config = fs::read(control.join("config.toml")).unwrap();
        migrate_legacy_workspace(&uri, &serde_json::json!({"noteFileNamePrefix":"changed"}))
            .unwrap();
        assert_eq!(fs::read(control.join("config.toml")).unwrap(), first_config);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_migration_rejects_sibling_filesystem_collisions_atomically() {
        let root = test_workspace("legacy-migration-collision");
        let control = root.join(".fleximark");
        fs::create_dir(&control).unwrap();
        fs::write(control.join("fleximark.json"), "legacy-marker").unwrap();
        fs::write(control.join("fleximark.css"), "body { color: purple; }").unwrap();
        fs::write(control.join("parserPlugin.js"), "module.exports = {};").unwrap();
        let uri = path_to_file_uri(&root).unwrap();

        assert!(matches!(
            migrate_legacy_workspace(
                &uri,
                &serde_json::json!({"noteCategories":{"Work":{},"work":{}}}),
            ),
            Err(ServiceError::InvalidConfig)
        ));
        assert!(!control.join("config.toml").exists());
        assert!(!control.join("theme.css").exists());
        assert_eq!(
            fs::read_to_string(control.join("fleximark.json")).unwrap(),
            "legacy-marker"
        );
        assert_eq!(
            fs::read_to_string(control.join("fleximark.css")).unwrap(),
            "body { color: purple; }"
        );
        assert!(control.join("parserPlugin.js").is_file());
        assert_eq!(inspect_legacy_workspace(&uri).unwrap(), Some(true));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_migration_rejects_invalid_nested_category_names_atomically() {
        let root = test_workspace("legacy-migration-invalid-category");
        let control = root.join(".fleximark");
        fs::create_dir(&control).unwrap();
        fs::write(control.join("fleximark.json"), "legacy-marker").unwrap();
        fs::write(control.join("fleximark.css"), "body { color: purple; }").unwrap();
        fs::write(control.join("parserPlugin.js"), "module.exports = {};").unwrap();
        let uri = path_to_file_uri(&root).unwrap();

        assert!(matches!(
            migrate_legacy_workspace(
                &uri,
                &serde_json::json!({"noteCategories":{"Valid":{"CON":{}}}}),
            ),
            Err(ServiceError::InvalidConfig)
        ));
        assert!(!control.join("config.toml").exists());
        assert!(!control.join("theme.css").exists());
        assert_eq!(
            fs::read_to_string(control.join("fleximark.json")).unwrap(),
            "legacy-marker"
        );
        assert_eq!(
            fs::read_to_string(control.join("fleximark.css")).unwrap(),
            "body { color: purple; }"
        );
        assert!(control.join("parserPlugin.js").is_file());
        assert_eq!(inspect_legacy_workspace(&uri).unwrap(), Some(true));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn canonical_config_parser_rejects_unknown_fields() {
        let root = test_workspace("config-test");
        let uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&uri).unwrap();
        fs::write(
            root.join(".fleximark/config.toml"),
            "schema_version = 2\nunknown = true\n",
        )
        .unwrap();
        assert!(matches!(
            create_note(&uri),
            Err(ServiceError::InvalidConfig)
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
