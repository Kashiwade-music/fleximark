//! Role: Verify and load installed runtime browser resources.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    version: String,
    target: String,
    protocol_version: u32,
    files: std::collections::BTreeMap<String, String>,
}

pub struct PreviewAssets {
    root: PathBuf,
    manifest: Manifest,
}
impl PreviewAssets {
    pub fn installed() -> io::Result<Self> {
        let root = match std::env::var_os("FLEXIMARK_RUNTIME_ROOT") {
            Some(root) => PathBuf::from(root),
            None => std::env::current_exe()?
                .parent()
                .and_then(Path::parent)
                .ok_or_else(|| io::Error::other("runtime executable must reside in bin/"))?
                .to_path_buf(),
        };
        Self::open(&root)
    }
    pub fn open(root: &Path) -> io::Result<Self> {
        let root = root.canonicalize()?;
        let manifest: Manifest = serde_json::from_slice(&fs::read(root.join("manifest.json"))?)
            .map_err(io::Error::other)?;
        if manifest.schema_version != 1
            || manifest.protocol_version.to_string() != env!("FLEXIMARK_PROTOCOL_VERSION")
            || manifest.version != env!("FLEXIMARK_RELEASE_VERSION")
            || manifest.target != env!("FLEXIMARK_BUILD_TARGET")
        {
            return Err(io::Error::other("unsupported runtime resource manifest"));
        }
        Ok(Self { root, manifest })
    }
    pub fn read(&self, name: &str) -> io::Result<String> {
        if !matches!(name, "browser-host.js" | "fleximark.css" | "katex.css") {
            return Err(io::Error::other("unknown preview resource"));
        }
        let relative = format!("resources/preview/{name}");
        self.read_path(&relative)
    }
    pub fn read_path(&self, relative: &str) -> io::Result<String> {
        if !matches!(
            relative,
            "resources/preview/browser-host.js"
                | "resources/preview/fleximark.css"
                | "resources/preview/katex.css"
                | "resources/workspace/workspace.js"
                | "resources/workspace/workspace.css"
        ) {
            return Err(io::Error::other("unknown runtime resource"));
        }
        let expected = self
            .manifest
            .files
            .get(relative)
            .ok_or_else(|| io::Error::other("resource missing from runtime manifest"))?;
        let path = self.root.join(relative);
        if path.symlink_metadata()?.file_type().is_symlink()
            || !path.canonicalize()?.starts_with(&self.root)
        {
            return Err(io::Error::other("runtime resource escapes distribution"));
        }
        let bytes = fs::read(path)?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        if &digest != expected {
            return Err(io::Error::other("runtime resource digest mismatch"));
        }
        String::from_utf8(bytes).map_err(io::Error::other)
    }
}
pub fn read(name: &str) -> io::Result<String> {
    PreviewAssets::installed()?.read(name)
}
pub fn read_path(path: &str) -> io::Result<String> {
    PreviewAssets::installed()?.read_path(path)
}

#[cfg(test)]
mod tests {
    //! Role: Verify resource manifests reject missing, changed, and unlisted browser assets.
    use super::*;
    #[test]
    fn manifest_integrity_is_checked_on_every_read() {
        let root = std::env::temp_dir().join(format!(
            "fleximark-assets-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("resources/preview")).unwrap();
        fs::write(
            root.join("resources/preview/browser-host.js"),
            "const version=1;",
        )
        .unwrap();
        let hash = format!("{:x}", Sha256::digest(b"const version=1;"));
        fs::write(root.join("manifest.json"),serde_json::json!({"schemaVersion":1,"version":env!("FLEXIMARK_RELEASE_VERSION"),"target":env!("FLEXIMARK_BUILD_TARGET"),"protocolVersion":env!("FLEXIMARK_PROTOCOL_VERSION").parse::<u32>().unwrap(),"files":{"resources/preview/browser-host.js":hash}}).to_string()).unwrap();
        let assets = PreviewAssets::open(&root).unwrap();
        assert_eq!(assets.read("browser-host.js").unwrap(), "const version=1;");
        assert!(assets.read("../manifest.json").is_err());
        assert!(assets.read("katex.css").is_err());
        fs::write(root.join("resources/preview/browser-host.js"), "changed").unwrap();
        assert!(assets.read("browser-host.js").is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
