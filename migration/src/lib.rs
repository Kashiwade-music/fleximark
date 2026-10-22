//! Role: Decode legacy data and apply validated recoverable migration plans.
use fleximark_config::{AssetsConfig, FlexiMarkConfig, NoteCategoryConfig, NotesConfig};
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};
mod jsonc;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Ready,
    AlreadyCurrent,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPlan {
    pub status: Status,
    pub workspace: PathBuf,
    pub outputs: Vec<String>,
    pub warnings: Vec<String>,
    pub config: String,
    #[serde(skip)]
    snapshots: Vec<(PathBuf, String)>,
    #[serde(skip)]
    theme: Vec<u8>,
}
fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn regular(path: &Path) -> io::Result<Vec<u8>> {
    let metadata = path.symlink_metadata()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid(format!("not a regular file: {}", path.display())));
    }
    fs::read(path)
}
fn directory(path: &Path) -> io::Result<()> {
    let metadata = path.symlink_metadata()?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid("migration directory must not be linked"));
    }
    Ok(())
}
fn snapshot_tree(path: &Path, snapshots: &mut Vec<(PathBuf, String)>) -> io::Result<()> {
    directory(path)?;
    let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    let names = entries
        .iter()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    snapshots.push((
        path.to_path_buf(),
        format!("directory:{}", digest(names.as_bytes())),
    ));
    for entry in entries {
        if entry.file_type()?.is_dir() {
            snapshot_tree(&entry.path(), snapshots)?;
        } else {
            snapshots.push((entry.path(), digest(&regular(&entry.path())?)));
        }
    }
    Ok(())
}
fn settings_object(value: &Value) -> io::Result<&Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| invalid("legacy configuration must be an object"))
}
fn categories(value: &Value) -> io::Result<BTreeMap<String, NoteCategoryConfig>> {
    settings_object(value)?
        .iter()
        .map(|(name, children)| Ok((name.clone(), NoteCategoryConfig(categories(children)?))))
        .collect()
}
pub fn plan(workspace: &Path, editor_settings: Option<&Path>) -> io::Result<MigrationPlan> {
    let workspace = workspace.canonicalize()?;
    if workspace.join(JOURNAL).try_exists()? {
        return Err(invalid(
            "interrupted migration requires fleximark migrate --recover",
        ));
    }
    let control = workspace.join(".fleximark");
    directory(&control)?;
    let mut snapshots = Vec::new();
    snapshot_tree(&control, &mut snapshots)?;
    let current = control.join("config.toml");
    let current_config = if current.try_exists()? {
        let config = String::from_utf8(regular(&current)?).map_err(io::Error::other)?;
        FlexiMarkConfig::from_toml(&config).map_err(io::Error::other)?;
        Some(config)
    } else {
        None
    };
    if !control.join("fleximark.json").try_exists()? {
        let config = current_config
            .ok_or_else(|| invalid("no legacy or current workspace configuration found"))?;
        return Ok(MigrationPlan {
            status: Status::AlreadyCurrent,
            workspace,
            outputs: vec![],
            warnings: vec![],
            config,
            snapshots,
            theme: vec![],
        });
    }
    let legacy = regular(&control.join("fleximark.json"))?;
    let source: Value = jsonc::decode(&legacy)?;
    let mut settings = settings_object(&source)?.clone();
    let default_settings = workspace.join(".vscode/settings.json");
    if editor_settings.is_none() && default_settings.is_file() {
        directory(&workspace.join(".vscode"))?;
    }
    let editor_settings = editor_settings.or_else(|| {
        default_settings
            .is_file()
            .then_some(default_settings.as_path())
    });
    if let Some(path) = editor_settings {
        let path = path.canonicalize()?;
        let bytes = regular(&path)?;
        snapshots.push((path, digest(&bytes)));
        let editor: Value = jsonc::decode(&bytes)?;
        let editor = editor.get("settings").unwrap_or(&editor);
        for (key, value) in settings_object(editor)? {
            if let Some(key) = key.strip_prefix("fleximark.") {
                settings.insert(key.to_owned(), value.clone());
            }
        }
    }
    let mut notes = NotesConfig::default();
    for (key, target) in [
        ("noteFileNamePrefix", &mut notes.file_name_prefix),
        ("noteFileNameSuffix", &mut notes.file_name_suffix),
    ] {
        if let Some(value) = settings.get(key) {
            *target = value
                .as_str()
                .ok_or_else(|| invalid(format!("{key} must be a string")))?
                .to_owned();
        }
    }
    if let Some(value) = settings.get("noteCategories") {
        notes.categories = categories(value)?;
    }
    if let Some(value) = settings.get("noteTemplates") {
        for (name, lines) in settings_object(value)? {
            if name.is_empty() {
                return Err(invalid("template name must not be empty"));
            }
            let lines = lines
                .as_array()
                .ok_or_else(|| invalid("template must be an array"))?;
            notes.templates.insert(
                name.clone(),
                lines
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| invalid("template lines must be strings"))
                    })
                    .collect::<io::Result<_>>()?,
            );
        }
    }
    let mut warnings = Vec::new();
    for key in settings.keys() {
        if !matches!(
            key.as_str(),
            "noteFileNamePrefix" | "noteFileNameSuffix" | "noteCategories" | "noteTemplates"
        ) {
            warnings.push(format!("Unconverted legacy setting: {key}"));
        }
    }
    if control.join("parserPlugin.js").try_exists()? {
        warnings.push("Unsupported JavaScript plugin preserved and disabled; replace it with a signed WASM plugin.".into());
    }
    let attachments = workspace.join("attachments");
    let roots = if attachments.try_exists()? {
        directory(&attachments)?;
        vec!["attachments".to_owned()]
    } else {
        vec![]
    };
    let config = FlexiMarkConfig {
        schema_version: 2,
        notes,
        assets: AssetsConfig { roots },
        plugins: vec![],
    };
    config.validate().map_err(io::Error::other)?;
    if let Some(current) = current_config {
        if FlexiMarkConfig::from_toml(&current).map_err(io::Error::other)? != config {
            return Err(invalid(
                "existing current configuration conflicts with the legacy migration plan",
            ));
        }
        return Ok(MigrationPlan {
            status: Status::AlreadyCurrent,
            workspace,
            outputs: vec![],
            warnings,
            config: current,
            snapshots,
            theme: vec![],
        });
    }
    let config = toml::to_string_pretty(&config).map_err(io::Error::other)?;
    FlexiMarkConfig::from_toml(&config).map_err(io::Error::other)?;
    let theme = if control.join("theme.css").try_exists()? {
        regular(&control.join("theme.css"))?
    } else if control.join("fleximark.css").try_exists()? {
        regular(&control.join("fleximark.css"))?
    } else {
        b"/* FlexiMark workspace theme */\n".to_vec()
    };
    Ok(MigrationPlan {
        status: Status::Ready,
        workspace,
        outputs: vec![
            ".fleximark/config.toml".into(),
            ".fleximark/theme.css".into(),
        ],
        warnings,
        config,
        snapshots,
        theme,
    })
}
fn copy_tree(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            let bytes = regular(&entry.path())?;
            write_new(&target, &bytes)?;
        }
    }
    Ok(())
}
fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}
const JOURNAL: &str = ".fleximark-migration-journal.json";
#[derive(serde::Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    stage: String,
    backup: String,
    config_hash: String,
    original: Vec<(PathBuf, String)>,
}
fn transaction_path(workspace: &Path, name: &str, prefix: &str) -> io::Result<PathBuf> {
    if !name.starts_with(prefix) || Path::new(name).components().count() != 1 {
        return Err(invalid("invalid migration journal path"));
    }
    Ok(workspace.join(name))
}
fn original_matches(root: &Path, expected: &[(PathBuf, String)]) -> io::Result<bool> {
    let mut actual = Vec::new();
    snapshot_tree(root, &mut actual)?;
    let actual = actual
        .into_iter()
        .map(|(path, hash)| (path.strip_prefix(root).unwrap().to_path_buf(), hash))
        .collect::<Vec<_>>();
    Ok(actual == expected)
}
/// Restore or acknowledge an interrupted explicitly requested migration.
pub fn recover(workspace: &Path) -> io::Result<String> {
    let workspace = workspace.canonicalize()?;
    let journal_path = workspace.join(JOURNAL);
    let journal: Journal =
        serde_json::from_slice(&regular(&journal_path)?).map_err(io::Error::other)?;
    let backup = transaction_path(&workspace, &journal.backup, ".fleximark-backup-")?;
    let _stage = transaction_path(&workspace, &journal.stage, ".fleximark-migration-")?;
    let control = workspace.join(".fleximark");
    let result = if !control.try_exists()? {
        if !original_matches(&backup, &journal.original)? {
            return Err(invalid("migration backup changed; refusing recovery"));
        }
        fs::rename(&backup, &control)?;
        "Restored original .fleximark directory; staged migration preserved"
    } else if original_matches(&control, &journal.original)? {
        "Original workspace data is intact; staged migration preserved"
    } else if backup.try_exists()?
        && original_matches(&backup, &journal.original)?
        && digest(&regular(&control.join("config.toml"))?) == journal.config_hash
    {
        "Migration was installed; original backup preserved"
    } else {
        return Err(invalid(
            "migration recovery conflicts with current user data; no files changed",
        ));
    };
    fs::remove_file(journal_path)?;
    Ok(result.into())
}
impl MigrationPlan {
    pub fn apply(&self) -> io::Result<Option<PathBuf>> {
        self.apply_with_checkpoint(|_| Ok(()))
    }
    fn apply_with_checkpoint(
        &self,
        mut checkpoint: impl FnMut(&str) -> io::Result<()>,
    ) -> io::Result<Option<PathBuf>> {
        if self.status == Status::AlreadyCurrent {
            return Ok(None);
        }
        let control = self.workspace.join(".fleximark");
        if self.workspace.join(JOURNAL).try_exists()? {
            return Err(invalid(
                "interrupted migration requires fleximark migrate --recover",
            ));
        }
        let mut actual = Vec::new();
        snapshot_tree(&control, &mut actual)?;
        let expected_control = self
            .snapshots
            .iter()
            .filter(|(path, _)| path.starts_with(&control))
            .cloned()
            .collect::<Vec<_>>();
        if actual != expected_control {
            return Err(invalid("migration input changed after planning"));
        }
        for (path, expected) in self
            .snapshots
            .iter()
            .filter(|(path, _)| !path.starts_with(&control))
        {
            if digest(&regular(path)?) != *expected {
                return Err(invalid("editor settings changed after planning"));
            }
        }
        if control.join("config.toml").try_exists()? {
            return Err(invalid("current configuration conflicts with migration"));
        }
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let stage = self.workspace.join(format!(".fleximark-migration-{nonce}"));
        let backup = self.workspace.join(format!(".fleximark-backup-{nonce}"));
        copy_tree(&control, &stage)?;
        write_new(&stage.join("config.toml"), self.config.as_bytes())?;
        if !stage.join("theme.css").try_exists()? {
            write_new(&stage.join("theme.css"), &self.theme)?;
        }
        let mut recheck = Vec::new();
        snapshot_tree(&control, &mut recheck)?;
        if recheck != expected_control {
            return Err(invalid(
                "migration inputs changed during staging; staged files retained",
            ));
        }
        let journal = Journal {
            stage: stage.file_name().unwrap().to_string_lossy().into_owned(),
            backup: backup.file_name().unwrap().to_string_lossy().into_owned(),
            config_hash: digest(self.config.as_bytes()),
            original: expected_control
                .iter()
                .map(|(path, hash)| {
                    (
                        path.strip_prefix(&control).unwrap().to_path_buf(),
                        hash.clone(),
                    )
                })
                .collect(),
        };
        let journal_path = self.workspace.join(JOURNAL);
        write_new(
            &journal_path,
            &serde_json::to_vec(&journal).map_err(io::Error::other)?,
        )?;
        checkpoint("journal-written")?;
        fs::rename(&control, &backup)?;
        checkpoint("old-moved")?;
        if !original_matches(&backup, &journal.original)? {
            fs::rename(&backup, &control)?;
            return Err(invalid(
                "migration inputs changed during commit; originals restored",
            ));
        }
        if let Err(error) = fs::rename(&stage, &control) {
            fs::rename(&backup, &control).map_err(|rollback| {
                invalid(format!(
                    "install failed: {error}; restore backup {}: {rollback}",
                    backup.display()
                ))
            })?;
            return Err(error);
        }
        checkpoint("installed")?;
        fs::remove_file(journal_path)?;
        Ok(Some(backup))
    }
}

#[cfg(test)]
mod tests {
    //! Role: Verify migration plans preserve legacy inputs and reject conflicts.
    use super::*;
    fn workspace(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "fleximark-migration-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join(".fleximark")).unwrap();
        root
    }
    #[test]
    fn validated_plan_preserves_originals_and_reports_unsupported_plugins() {
        let root = workspace("preserve");
        let control = root.join(".fleximark");
        fs::write(control.join("fleximark.json"),r##"{"noteCategories":{"Work":{"Weekly":{}}},"noteTemplates":{"daily":["# Title"]},"noteFileNamePrefix":"daily-"}"##).unwrap();
        fs::write(
            control.join("parserPlugin.js"),
            "throw new Error('must not execute');",
        )
        .unwrap();
        fs::write(control.join("fleximark.css"), "body {color:purple}").unwrap();
        let plan = plan(&root, None).unwrap();
        assert!(!control.join("config.toml").exists());
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("JavaScript"))
        );
        let config = FlexiMarkConfig::from_toml(&plan.config).unwrap();
        assert_eq!(config.notes.root, ".");
        assert_eq!(config.notes.file_name_prefix, "daily-");
        assert!(config.notes.categories["Work"].0.contains_key("Weekly"));
        let backup = plan.apply().unwrap().unwrap();
        assert_eq!(
            fs::read(control.join("fleximark.json")).unwrap(),
            fs::read(backup.join("fleximark.json")).unwrap()
        );
        assert_eq!(
            fs::read(control.join("theme.css")).unwrap(),
            b"body {color:purple}"
        );
        assert!(control.join("parserPlugin.js").is_file());
        assert_eq!(
            super::plan(&root, None).unwrap().status,
            Status::AlreadyCurrent
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn changed_inputs_and_new_output_conflicts_never_apply() {
        let root = workspace("conflicts");
        let control = root.join(".fleximark");
        fs::write(control.join("fleximark.json"), "{}").unwrap();
        let migration = plan(&root, None).unwrap();
        fs::write(
            control.join("fleximark.json"),
            r#"{"noteFileNamePrefix":"changed"}"#,
        )
        .unwrap();
        assert!(migration.apply().is_err());
        assert!(!control.join("config.toml").exists());
        let migration = plan(&root, None).unwrap();
        fs::write(control.join("config.toml"), "schema_version=2\n").unwrap();
        assert!(migration.apply().is_err());
        assert_eq!(
            fs::read_to_string(control.join("config.toml")).unwrap(),
            "schema_version=2\n"
        );
        assert!(plan(&root, None).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn invalid_values_and_filesystem_collisions_reject_before_writes() {
        let root = workspace("invalid");
        let control = root.join(".fleximark");
        for input in [
            r#"{"noteFileNamePrefix":42}"#,
            r#"{"noteTemplates":{"daily":"wrong"}}"#,
            r#"{"noteCategories":{"Work":{},"work":{}}}"#,
            r#"{"noteCategories":{"CON":{}}}"#,
        ] {
            fs::write(control.join("fleximark.json"), input).unwrap();
            assert!(plan(&root, None).is_err(), "{input}");
            assert!(!control.join("config.toml").exists());
            assert!(!control.join("theme.css").exists());
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn jsonc_editor_settings_override_legacy_values_and_preserve_unicode() {
        let root = workspace("jsonc");
        let control = root.join(".fleximark");
        fs::write(
            control.join("fleximark.json"),
            r#"{"noteFileNamePrefix":"legacy-"}"#,
        )
        .unwrap();
        fs::create_dir(root.join(".vscode")).unwrap();
        let settings = root.join(".vscode/settings.json");
        fs::write(&settings,"// legacy settings\r\n{\r\n/* comment */\"fleximark.noteFileNamePrefix\":\"日本-\",\r\n\"fleximark.noteTemplates\":{\"日誌\":[\"https://example.test/*literal*/\",\"line\\r\\nnext\",],},\r\n}").unwrap();
        let migration = plan(&root, None).unwrap();
        let config = FlexiMarkConfig::from_toml(&migration.config).unwrap();
        assert_eq!(config.notes.file_name_prefix, "日本-");
        assert_eq!(
            config.notes.templates["日誌"],
            ["https://example.test/*literal*/", "line\r\nnext"]
        );
        fs::write(&settings, "// changed after planning\n{}").unwrap();
        assert!(migration.apply().is_err());
        assert!(!control.join("config.toml").exists());
        let explicit = root.join("project.code-workspace");
        fs::write(
            &explicit,
            r#"{"folders":[],"settings":{"fleximark.noteFileNamePrefix":"explicit-",},}"#,
        )
        .unwrap();
        let migration = plan(&root, Some(&explicit)).unwrap();
        assert_eq!(
            FlexiMarkConfig::from_toml(&migration.config)
                .unwrap()
                .notes
                .file_name_prefix,
            "explicit-"
        );
        fs::write(&explicit, "/* unterminated").unwrap();
        assert!(plan(&root, Some(&explicit)).is_err());
        assert!(control.join("fleximark.json").exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn interrupted_directory_commit_can_be_explicitly_recovered_without_data_loss() {
        let root = workspace("recover");
        let control = root.join(".fleximark");
        let original = r#"{"noteFileNamePrefix":"original-"}"#;
        fs::write(control.join("fleximark.json"), original).unwrap();
        let migration = plan(&root, None).unwrap();
        let failure = migration.apply_with_checkpoint(|step| {
            if step == "old-moved" {
                Err(io::Error::other("injected interruption"))
            } else {
                Ok(())
            }
        });
        assert!(failure.is_err());
        assert!(!control.exists());
        assert!(root.join(JOURNAL).is_file());
        assert!(
            plan(&root, None)
                .unwrap_err()
                .to_string()
                .contains("--recover")
        );
        assert!(recover(&root).unwrap().contains("Restored original"));
        assert_eq!(
            fs::read_to_string(control.join("fleximark.json")).unwrap(),
            original
        );
        assert!(!control.join("config.toml").exists());
        assert!(!root.join(JOURNAL).exists());
        let migration = plan(&root, None).unwrap();
        assert!(migration.apply().unwrap().is_some());
        fs::remove_dir_all(root).unwrap();
    }
}
