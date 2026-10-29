//! Role: Load authenticated browser resources from a verified runtime distribution.
use std::{
    io,
    path::{Path, PathBuf},
};

use fleximark_distribution::{RuntimeDistribution, release_metadata, specification};

pub struct PreviewAssets {
    runtime: RuntimeDistribution,
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
        Ok(Self {
            runtime: RuntimeDistribution::open(
                root,
                &release_metadata()?,
                env!("FLEXIMARK_BUILD_TARGET"),
            )?,
        })
    }
    pub fn read(&self, name: &str) -> io::Result<String> {
        self.read_path(&format!("resources/preview/{name}"))
    }
    pub fn read_path(&self, relative: &str) -> io::Result<String> {
        if !specification()?
            .files
            .iter()
            .any(|file| file.source != "native" && file.path == relative)
        {
            return Err(io::Error::other("unknown runtime resource"));
        }
        String::from_utf8(self.runtime.read(relative)?).map_err(io::Error::other)
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
    use fleximark_distribution::sha256;
    use std::fs;
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
        let mut files = std::collections::BTreeMap::new();
        for relative in specification()
            .unwrap()
            .paths(env!("FLEXIMARK_BUILD_TARGET"))
            .unwrap()
        {
            let path = root.join(&relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            if relative != "resources/preview/browser-host.js" {
                fs::write(&path, "resource").unwrap();
            }
            files.insert(relative, sha256(&fs::read(path).unwrap()));
        }
        fs::write(root.join("manifest.json"),serde_json::json!({"schemaVersion":1,"version":env!("FLEXIMARK_RELEASE_VERSION"),"target":env!("FLEXIMARK_BUILD_TARGET"),"protocolVersion":env!("FLEXIMARK_PROTOCOL_VERSION").parse::<u32>().unwrap(),"files":files}).to_string()).unwrap();
        let assets = PreviewAssets::open(&root).unwrap();
        assert_eq!(assets.read("browser-host.js").unwrap(), "const version=1;");
        assert!(assets.read("../manifest.json").is_err());
        assert_eq!(assets.read("katex.css").unwrap(), "resource");
        assert!(assets.read("unlisted.js").is_err());
        fs::write(root.join("resources/preview/browser-host.js"), "changed").unwrap();
        assert!(assets.read("browser-host.js").is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
