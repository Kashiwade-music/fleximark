//! Role: Native workspace workspace operations and validation.
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use crate::outcomes::InitializedWorkspace;
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{ambient_authority, fs::Dir};
use fleximark_config::FlexiMarkConfig;

use crate::native::{ServiceError, path_to_file_uri, workspace_path};

pub(crate) const CONFIG: &str =
    "# FlexiMark workspace configuration\nschema_version = 2\n\n[notes]\nroot = \"notes\"\n";
const THEME: &str = "/* FlexiMark workspace theme */\n:root { color-scheme: light dark; }\n";

pub fn initialize_workspace(workspace_uri: &str) -> Result<InitializedWorkspace, ServiceError> {
    let root = workspace_path(workspace_uri)?;
    let control = open_control_directory(&root, true)?.expect("created control directory");
    write_new_at(&control, "config.toml", CONFIG)?;
    validate_config(&root.join(".fleximark/config.toml"))?;
    write_new_at(&control, "theme.css", THEME)?;
    let config = root.join(".fleximark/config.toml");
    Ok(InitializedWorkspace {
        config_uri: path_to_file_uri(&config)?,
        theme_uri: path_to_file_uri(&root.join(".fleximark/theme.css"))?,
    })
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

pub fn workspace_for_document(document_uri: &str) -> Result<String, ServiceError> {
    let document = workspace_path(document_uri)?;
    for parent in document
        .parent()
        .ok_or(ServiceError::InvalidWorkspaceUri)?
        .ancestors()
    {
        if parent.join(".fleximark/config.toml").is_file() {
            validate_config(&parent.join(".fleximark/config.toml"))?;
            return path_to_file_uri(parent);
        }
    }
    Err(ServiceError::NotInitialized)
}

#[cfg(test)]
mod tests {
    //! Role: Verify workspace operations and security invariants.
    use super::*;
    use crate::native::test_support::test_workspace;
    use crate::native::{create_note, get_note_options, load_plugin_host};
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
            load_plugin_host(&uri, true, 1),
            Err(ServiceError::LinkedControlDirectory)
        ));
        assert!(
            load_plugin_host(&uri, false, 1).is_ok(),
            "restricted configuration must not inspect a linked control directory"
        );
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
