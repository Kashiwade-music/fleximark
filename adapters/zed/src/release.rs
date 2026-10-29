//! Role: Install and validate platform-specific shared runtime releases.

use std::{fs, path::Path};
use zed_extension_api as zed;

const VERSION: &str = env!("FLEXIMARK_RELEASE_VERSION");
const REPOSITORY: &str = "Kashiwade-music/fleximark";

pub fn target(os: zed::Os, arch: zed::Architecture) -> Result<&'static str, String> {
    let os = match os {
        zed::Os::Mac => "darwin",
        zed::Os::Linux => "linux",
        zed::Os::Windows => "win32",
    };
    let arch = match arch {
        zed::Architecture::Aarch64 => "arm64",
        zed::Architecture::X8664 => "x64",
        _ => return Err("No FlexiMark runtime release for this platform".into()),
    };
    Ok(fleximark_distribution::specification()
        .map_err(|error| error.to_string())?
        .platform(os, arch)
        .map_err(|error| error.to_string())?
        .triple
        .as_str())
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
    let release = fleximark_distribution::ReleaseMetadata {
        version: version.to_owned(),
        protocol_version: env!("FLEXIMARK_PROTOCOL_VERSION")
            .parse::<u32>()
            .map_err(|error| error.to_string())?,
    };
    fleximark_distribution::RuntimeDistribution::open(root, &release, target)
        .and_then(|runtime| runtime.verify())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    //! Role: Verify native platform mapping and strict runtime version matching.
    use super::*;
    use zed::serde_json::Value;
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
                Value::String(fleximark_distribution::sha256(name.as_bytes())),
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
