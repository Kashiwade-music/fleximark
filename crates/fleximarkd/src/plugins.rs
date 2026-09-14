use std::fs;
use std::io::Read;
use std::path::Path;

use cap_std::{ambient_authority, fs::Dir};
use fleximark_engine::RenderConfig;
use fleximark_plugin_host::{ExecutionLimits, HostPolicy, PluginHost, VerifiedPluginPackage};
use fleximark_plugin_sdk::FlexiMarkConfig;

use crate::export::sha256;
use crate::theme::render_config;
use crate::workspace::{CONFIG, open_control_directory, open_regular_nofollow};
use crate::{ServiceError, workspace_path};

pub fn load_plugin_host(
    workspace_uri: &str,
    trusted: bool,
    generation: u64,
) -> Result<(PluginHost, RenderConfig), ServiceError> {
    let workspace = workspace_path(workspace_uri)?;
    let control = open_control_directory(&workspace, false)?;
    let config_source = if let Some(control) = &control {
        match open_regular_nofollow(control, Path::new("config.toml")) {
            Ok(mut file) => {
                let mut text = String::new();
                file.read_to_string(&mut text)?;
                text
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => CONFIG.to_owned(),
            Err(error) => return Err(error.into()),
        }
    } else {
        CONFIG.to_owned()
    };
    let config =
        FlexiMarkConfig::from_toml(&config_source).map_err(|_| ServiceError::InvalidConfig)?;
    let config_hash = sha256(config_source.as_bytes());
    let mut host = PluginHost::configured(
        ExecutionLimits::default(),
        HostPolicy {
            workspace_trusted: trusted,
            workspace_root: workspace.to_string_lossy().into_owned(),
        },
        config_hash.clone(),
        generation,
    )
    .map_err(|error| ServiceError::PluginConfig(error.to_string()))?;
    if !trusted || config.plugins.iter().all(|plugin| !plugin.enabled) {
        let render_config = render_config(&workspace, &config, &host, trusted)?;
        return Ok((host, render_config));
    }
    let plugin_root = workspace.join(".fleximark/plugins");
    reject_linked_path(&workspace, Path::new(".fleximark/plugins"), true)?;
    let canonical_plugin_root = plugin_root.canonicalize().map_err(|error| {
        ServiceError::PluginConfig(format!("plugin directory is unavailable: {error}"))
    })?;
    if !canonical_plugin_root.starts_with(&workspace) {
        return Err(ServiceError::PluginConfig(
            "plugin directory escapes the workspace".to_owned(),
        ));
    }
    let directory = Dir::open_ambient_dir(&canonical_plugin_root, ambient_authority())?;
    for (config_order, plugin) in config
        .plugins
        .iter()
        .filter(|plugin| plugin.enabled)
        .enumerate()
    {
        let manifest = read_plugin_file(&directory, &canonical_plugin_root, &plugin.manifest)?;
        let wasm = read_plugin_file(&directory, &canonical_plugin_root, &plugin.wasm)?;
        let signature = read_plugin_file(&directory, &canonical_plugin_root, &plugin.signature)?;
        host.register_verified(VerifiedPluginPackage {
            configured_id: &plugin.id,
            config_order,
            manifest_bytes: &manifest,
            expected_manifest_sha256: &plugin.manifest_sha256,
            wasm_bytes: &wasm,
            signature_bytes: &signature,
            signer_public_key: &plugin.signer_public_key,
            grants: plugin.grants.clone(),
            environment: plugin.environment.clone(),
        })
        .map_err(|error| ServiceError::PluginConfig(error.to_string()))?;
    }
    let render_config = render_config(&workspace, &config, &host, trusted)?;
    Ok((host, render_config))
}

fn read_plugin_file(directory: &Dir, root: &Path, relative: &str) -> Result<Vec<u8>, ServiceError> {
    let relative = Path::new(relative);
    reject_linked_path(root, relative, false)?;
    let absolute = root.join(relative).canonicalize()?;
    if !absolute.starts_with(root) {
        return Err(ServiceError::PluginConfig(
            "plugin package path escapes .fleximark/plugins".to_owned(),
        ));
    }
    let metadata = fs::symlink_metadata(&absolute)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(ServiceError::PluginConfig(
            "plugin package entry is not a regular file".to_owned(),
        ));
    }
    let mut bytes = Vec::new();
    directory.open(relative)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub(crate) fn reject_linked_path(
    root: &Path,
    relative: &Path,
    final_directory: bool,
) -> Result<(), ServiceError> {
    let mut current = root.to_path_buf();
    for (index, component) in relative.components().enumerate() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink()
            || (index + 1 == relative.components().count() && final_directory && !metadata.is_dir())
        {
            return Err(ServiceError::PluginConfig(
                "plugin package contains a linked or invalid path component".to_owned(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_workspace;
    use crate::{initialize_workspace, path_to_file_uri};
    #[test]
    fn plugin_loader_reads_verified_package_files() {
        let root = test_workspace("plugin-loader-test");
        let uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&uri).unwrap();
        let plugin_root = root.join(".fleximark/plugins");
        fs::create_dir(&plugin_root).unwrap();
        let wasm = b"\0asm\x01\0\0\0";
        fs::write(plugin_root.join("sample.wasm"), wasm).unwrap();
        let manifest = format!(
            "schema_version = 1\n[plugin]\nid = \"sample.plugin\"\napi_version = 1\nrequired = false\n[artifact]\nwasm_sha256 = \"{}\"\n[capabilities]\n",
            sha256(wasm)
        );
        fs::write(plugin_root.join("sample.toml"), &manifest).unwrap();
        fs::write(plugin_root.join("sample.sig"), [0_u8; 64]).unwrap();
        fs::write(
            root.join(".fleximark/config.toml"),
            format!(
                "schema_version = 1\n[[plugins]]\nid = \"sample.plugin\"\nwasm = \"sample.wasm\"\nmanifest = \"sample.toml\"\nsignature = \"sample.sig\"\nmanifest_sha256 = \"{}\"\nsigner_public_key = \"{}\"\n",
                sha256(manifest.as_bytes()),
                "00".repeat(32)
            ),
        )
        .unwrap();
        assert!(matches!(
            load_plugin_host(&uri, true, 1),
            Err(ServiceError::PluginConfig(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
