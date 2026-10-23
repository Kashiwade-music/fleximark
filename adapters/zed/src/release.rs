//! Role: Install and validate platform-specific shared runtime releases.

use sha2::{Digest, Sha256};
use std::{fs, path::Path};
use zed_extension_api::{self as zed, serde_json::Value};

const VERSION: &str = env!("FLEXIMARK_RELEASE_VERSION");
const REPOSITORY: &str = "Kashiwade-music/fleximark";

pub fn target(os: zed::Os, arch: zed::Architecture) -> Result<&'static str, String> {
    match (os, arch) {
        (zed::Os::Mac, zed::Architecture::Aarch64) => Ok("aarch64-apple-darwin"),
        (zed::Os::Mac, zed::Architecture::X8664) => Ok("x86_64-apple-darwin"),
        (zed::Os::Linux, zed::Architecture::Aarch64) => Ok("aarch64-unknown-linux-gnu"),
        (zed::Os::Linux, zed::Architecture::X8664) => Ok("x86_64-unknown-linux-gnu"),
        (zed::Os::Windows, zed::Architecture::Aarch64) => Ok("aarch64-pc-windows-msvc"),
        (zed::Os::Windows, zed::Architecture::X8664) => Ok("x86_64-pc-windows-msvc"),
        _ => Err("No FlexiMark runtime release for this platform".into()),
    }
}

pub fn install(id: &zed::LanguageServerId) -> Result<String, String> {
    let (os, arch) = zed::current_platform();
    let target = target(os, arch)?;
    let directory = format!("fleximark-{VERSION}-{target}");
    let executable = if os == zed::Os::Windows {
        "bin/fleximarkd.exe"
    } else {
        "bin/fleximarkd"
    };
    let binary = format!("{directory}/{executable}");
    if verify(Path::new(&directory), VERSION, target).is_err() {
        zed::set_language_server_installation_status(
            id,
            &zed::LanguageServerInstallationStatus::Downloading,
        );
        let release = zed::github_release_by_tag_name(REPOSITORY, &format!("v{VERSION}"))?;
        let name = format!("fleximark-{VERSION}-{target}.zip");
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == name)
            .ok_or_else(|| format!("Release v{VERSION} has no {name}"))?;
        zed::download_file(
            &asset.download_url,
            &directory,
            zed::DownloadedFileType::Zip,
        )?;
        verify(Path::new(&directory), VERSION, target)?;
    }
    zed::make_file_executable(&binary)?;
    let cli = if os == zed::Os::Windows {
        "bin/fleximark.exe"
    } else {
        "bin/fleximark"
    };
    zed::make_file_executable(&format!("{directory}/{cli}"))?;
    Ok(fs::canonicalize(binary)
        .map_err(|error| error.to_string())?
        .to_string_lossy()
        .into_owned())
}

pub fn check_binary(binary: &str) -> Result<(), String> {
    let output = zed::process::Command::new(binary)
        .arg("--version")
        .output()?;
    if output.status != Some(0) {
        return Err("FlexiMark runtime version check failed".into());
    }
    let version = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
    validate_version(version.trim(), VERSION)
}

fn validate_version(output: &str, version: &str) -> Result<(), String> {
    if output == format!("fleximarkd {version}") {
        Ok(())
    } else {
        Err(format!("Expected fleximarkd {version}; received {output}"))
    }
}

fn verify(root: &Path, version: &str, target: &str) -> Result<(), String> {
    let root_metadata = fs::symlink_metadata(root).map_err(|error| error.to_string())?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err("Runtime installation must be a regular directory".into());
    }
    regular_file(root, "manifest.json")?;
    let manifest: Value = zed::serde_json::from_slice(
        &fs::read(root.join("manifest.json")).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let fields = manifest
        .as_object()
        .ok_or("Runtime manifest must be an object")?;
    let expected_fields = [
        "schemaVersion",
        "version",
        "target",
        "protocolVersion",
        "files",
    ];
    if fields.len() != expected_fields.len()
        || expected_fields
            .iter()
            .any(|field| !fields.contains_key(*field))
    {
        return Err("Runtime manifest has unexpected or missing fields".into());
    }
    if manifest["schemaVersion"] != 1
        || manifest["version"] != version
        || manifest["target"] != target
        || manifest["protocolVersion"].as_u64()
            != env!("FLEXIMARK_PROTOCOL_VERSION").parse::<u64>().ok()
    {
        return Err("Incompatible FlexiMark runtime manifest".into());
    }
    let files = manifest["files"]
        .as_object()
        .ok_or("Runtime manifest lacks file checksums")?;
    let suffix = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let required_files = [
        format!("bin/fleximarkd{suffix}"),
        format!("bin/fleximark{suffix}"),
        "resources/preview/browser-host.js".into(),
        "resources/preview/fleximark.css".into(),
        "resources/preview/katex.css".into(),
        "resources/workspace/workspace.js".into(),
        "resources/workspace/workspace.css".into(),
    ];
    if files.len() != required_files.len() {
        return Err("Runtime manifest must contain exactly the seven distribution files".into());
    }
    for required in &required_files {
        if !files.contains_key(required) {
            return Err(format!("Missing required runtime file {required}"));
        }
    }
    for (relative, expected) in files {
        let checksum = expected
            .as_str()
            .ok_or("Runtime checksum must be a lowercase SHA-256 string")?;
        if checksum.len() != 64
            || !checksum
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(format!("Invalid runtime checksum: {relative}"));
        }
        regular_file(root, relative)?;
        let path = Path::new(relative);
        let resolved = fs::canonicalize(root.join(path)).map_err(|error| error.to_string())?;
        let canonical_root = fs::canonicalize(root).map_err(|error| error.to_string())?;
        if !resolved.starts_with(canonical_root) {
            return Err("Runtime file escapes installation directory".into());
        }
        let bytes = fs::read(resolved).map_err(|error| error.to_string())?;
        let actual = format!("{:x}", Sha256::digest(bytes));
        if expected.as_str() != Some(&actual) {
            return Err(format!("Runtime checksum mismatch: {relative}"));
        }
    }
    verify_tree(root, root, &required_files)?;
    Ok(())
}

fn regular_file(root: &Path, relative: &str) -> Result<(), String> {
    let path = Path::new(relative);
    let mut current = root.to_path_buf();
    for part in path.components() {
        let std::path::Component::Normal(part) = part else {
            return Err("Invalid runtime path".into());
        };
        current.push(part);
        let metadata = fs::symlink_metadata(&current).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("Runtime distribution contains a link".into());
        }
        if current == root.join(relative) && (!metadata.is_file() || metadata.len() == 0) {
            return Err(format!("Runtime file is empty or not regular: {relative}"));
        }
    }
    Ok(())
}

fn verify_tree(root: &Path, directory: &Path, required: &[String; 7]) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_dir()
            && required
                .iter()
                .any(|name| name.starts_with(&format!("{relative}/")))
        {
            verify_tree(root, &path, required)?;
        } else if !kind.is_file() || (relative != "manifest.json" && !required.contains(&relative))
        {
            return Err(format!("Unexpected runtime distribution entry: {relative}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Role: Verify native platform mapping and strict runtime version matching.
    use super::*;
    #[test]
    fn native_targets() {
        assert_eq!(
            target(zed::Os::Windows, zed::Architecture::X8664).unwrap(),
            "x86_64-pc-windows-msvc"
        );
        assert_eq!(
            target(zed::Os::Linux, zed::Architecture::Aarch64).unwrap(),
            "aarch64-unknown-linux-gnu"
        );
        assert_eq!(
            target(zed::Os::Mac, zed::Architecture::Aarch64).unwrap(),
            "aarch64-apple-darwin"
        );
    }
    #[test]
    fn rejects_incompatible_binaries() {
        assert!(validate_version("fleximarkd 0.1.0", "0.1.0").is_ok());
        assert!(validate_version("fleximarkd 0.2.0", "0.1.0").is_err());
        assert!(validate_version("fleximark 0.1.0", "0.1.0").is_err());
    }

    #[test]
    fn rejects_unsupported_architecture() {
        assert!(target(zed::Os::Linux, zed::Architecture::X86).is_err());
        assert_eq!(
            target(zed::Os::Mac, zed::Architecture::X8664).unwrap(),
            "x86_64-apple-darwin"
        );
        assert_eq!(
            target(zed::Os::Windows, zed::Architecture::Aarch64).unwrap(),
            "aarch64-pc-windows-msvc"
        );
    }

    #[test]
    fn verifies_distribution_files_and_rejects_tampering() {
        let directory =
            std::env::temp_dir().join(format!("fleximark-zed-manifest-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let names = [
            "bin/fleximarkd",
            "bin/fleximark",
            "resources/preview/browser-host.js",
            "resources/preview/fleximark.css",
            "resources/preview/katex.css",
            "resources/workspace/workspace.js",
            "resources/workspace/workspace.css",
        ];
        let mut files = zed::serde_json::Map::new();
        for name in names {
            let path = directory.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, name).unwrap();
            files.insert(
                name.into(),
                Value::String(format!("{:x}", Sha256::digest(name.as_bytes()))),
            );
        }
        let mut manifest = zed::serde_json::Map::new();
        manifest.insert("schemaVersion".into(), Value::from(1));
        manifest.insert(
            "protocolVersion".into(),
            Value::from(env!("FLEXIMARK_PROTOCOL_VERSION").parse::<u64>().unwrap()),
        );
        manifest.insert("version".into(), Value::from("1.2.3"));
        manifest.insert("target".into(), Value::from("x86_64-unknown-linux-gnu"));
        manifest.insert("files".into(), Value::Object(files));
        fs::write(
            directory.join("manifest.json"),
            zed::serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert!(verify(&directory, "1.2.3", "x86_64-unknown-linux-gnu").is_ok());
        assert!(verify(&directory, "1.2.4", "x86_64-unknown-linux-gnu").is_err());
        assert!(verify(&directory, "1.2.3", "aarch64-apple-darwin").is_err());
        let save = |manifest: &zed::serde_json::Map<String, Value>| {
            fs::write(
                directory.join("manifest.json"),
                zed::serde_json::to_vec(manifest).unwrap(),
            )
            .unwrap();
        };
        manifest.insert("legacy".into(), Value::Bool(true));
        save(&manifest);
        assert!(verify(&directory, "1.2.3", "x86_64-unknown-linux-gnu").is_err());
        manifest.remove("legacy");
        let resource = "resources/workspace/workspace.css";
        let checksum = manifest["files"][resource].clone();
        manifest
            .get_mut("files")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(resource);
        save(&manifest);
        assert!(verify(&directory, "1.2.3", "x86_64-unknown-linux-gnu").is_err());
        manifest
            .get_mut("files")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(resource.into(), checksum.clone());
        manifest
            .get_mut("files")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("extra.txt".into(), checksum.clone());
        save(&manifest);
        assert!(verify(&directory, "1.2.3", "x86_64-unknown-linux-gnu").is_err());
        manifest
            .get_mut("files")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("extra.txt");
        for malformed in [
            Value::String("G".repeat(64)),
            Value::String("A".repeat(64)),
            Value::String("a".repeat(63)),
            Value::Bool(true),
        ] {
            manifest
                .get_mut("files")
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert(resource.into(), malformed);
            save(&manifest);
            assert!(verify(&directory, "1.2.3", "x86_64-unknown-linux-gnu").is_err());
        }
        manifest
            .get_mut("files")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(resource.into(), checksum);
        save(&manifest);
        fs::write(directory.join("extra.txt"), "unlisted").unwrap();
        assert!(verify(&directory, "1.2.3", "x86_64-unknown-linux-gnu").is_err());
        fs::remove_file(directory.join("extra.txt")).unwrap();
        fs::write(directory.join(resource), "").unwrap();
        assert!(verify(&directory, "1.2.3", "x86_64-unknown-linux-gnu").is_err());
        fs::remove_file(directory.join(resource)).unwrap();
        fs::create_dir(directory.join(resource)).unwrap();
        assert!(verify(&directory, "1.2.3", "x86_64-unknown-linux-gnu").is_err());
        fs::remove_dir(directory.join(resource)).unwrap();
        fs::write(directory.join(resource), resource).unwrap();
        #[cfg(unix)]
        {
            let original = directory.join("original.css");
            fs::rename(directory.join(resource), &original).unwrap();
            std::os::unix::fs::symlink(&original, directory.join(resource)).unwrap();
            assert!(verify(&directory, "1.2.3", "x86_64-unknown-linux-gnu").is_err());
            fs::remove_file(directory.join(resource)).unwrap();
            fs::rename(original, directory.join(resource)).unwrap();
        }
        assert!(verify(&directory, "1.2.3", "x86_64-unknown-linux-gnu").is_ok());
        fs::write(directory.join("bin/fleximarkd"), "tampered").unwrap();
        assert!(verify(&directory, "1.2.3", "x86_64-unknown-linux-gnu").is_err());
        fs::remove_dir_all(directory).unwrap();
    }
}
