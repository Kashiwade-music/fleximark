//! Role: Validate canonical runtime distributions and their contained payloads.
mod release;
pub use release::{ReleaseMetadata, release_metadata, valid_version};

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DistributionSpec {
    pub targets: Vec<Target>,
    pub files: Vec<PayloadFile>,
    pub limits: Limits,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Target {
    pub triple: String,
    pub vscode: String,
    pub os: String,
    pub arch: String,
    pub executable_suffix: String,
    pub runner: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayloadFile {
    pub path: String,
    pub source: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Limits {
    pub manifest_bytes: u64,
    pub file_bytes: u64,
    pub total_bytes: u64,
}

pub fn specification() -> io::Result<&'static DistributionSpec> {
    static SPEC: OnceLock<Result<DistributionSpec, String>> = OnceLock::new();
    match SPEC.get_or_init(|| {
        serde_json::from_str(include_str!("../distribution.json"))
            .map_err(|error| error.to_string())
    }) {
        Ok(spec) => Ok(spec),
        Err(error) => Err(io::Error::other(error.clone())),
    }
}

impl DistributionSpec {
    pub fn target(&self, triple: &str) -> io::Result<&Target> {
        self.targets
            .iter()
            .find(|target| target.triple == triple)
            .ok_or_else(|| io::Error::other("unsupported runtime target"))
    }

    pub fn platform(&self, os: &str, arch: &str) -> io::Result<&Target> {
        self.targets
            .iter()
            .find(|target| target.os == os && target.arch == arch)
            .ok_or_else(|| io::Error::other("unsupported runtime platform"))
    }

    pub fn paths(&self, target: &str) -> io::Result<Vec<String>> {
        let target = self.target(target)?;
        Ok(self
            .files
            .iter()
            .map(|file| file.path.replace("{exe}", &target.executable_suffix))
            .collect())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeManifest {
    pub schema_version: u32,
    pub version: String,
    pub target: String,
    pub protocol_version: u32,
    pub files: BTreeMap<String, String>,
}

impl RuntimeManifest {
    pub fn validate(&self, release: &ReleaseMetadata, target: &str) -> io::Result<()> {
        if self.schema_version != 1
            || !valid_version(&self.version)
            || self.version != release.version
            || self.protocol_version != release.protocol_version
            || self.target != target
        {
            return Err(io::Error::other("incompatible runtime manifest"));
        }
        let expected = specification()?.paths(target)?;
        if self.files.len() != expected.len()
            || expected.iter().any(|path| !self.files.contains_key(path))
            || self.files.values().any(|hash| !valid_sha256(hash))
        {
            return Err(io::Error::other("invalid runtime payload set or checksum"));
        }
        Ok(())
    }
}

pub fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub struct RuntimeDistribution {
    root: PathBuf,
    manifest: RuntimeManifest,
}

impl RuntimeDistribution {
    pub fn open(root: &Path, release: &ReleaseMetadata, target: &str) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(root)?;
        if !metadata.is_dir() || linked(&metadata) {
            return Err(io::Error::other("runtime root must be a regular directory"));
        }
        let root = root.canonicalize()?;
        let bytes = read_contained(
            &root,
            "manifest.json",
            specification()?.limits.manifest_bytes,
        )?;
        let manifest: RuntimeManifest = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        manifest.validate(release, target)?;
        Ok(Self { root, manifest })
    }

    pub fn manifest(&self) -> &RuntimeManifest {
        &self.manifest
    }

    pub fn read(&self, relative: &str) -> io::Result<Vec<u8>> {
        let expected = self
            .manifest
            .files
            .get(relative)
            .ok_or_else(|| io::Error::other("unlisted runtime payload"))?;
        let bytes = read_contained(&self.root, relative, specification()?.limits.file_bytes)?;
        if sha256(&bytes) != *expected {
            return Err(io::Error::other("runtime payload checksum mismatch"));
        }
        Ok(bytes)
    }

    pub fn verify(&self) -> io::Result<()> {
        let mut size = 0_u64;
        for name in self.manifest.files.keys() {
            size = size
                .checked_add(self.read(name)?.len() as u64)
                .ok_or_else(|| io::Error::other("runtime payload size overflow"))?;
        }
        if size > specification()?.limits.total_bytes {
            return Err(io::Error::other("runtime payload exceeds total size limit"));
        }
        let mut expected = self.manifest.files.keys().cloned().collect::<HashSet<_>>();
        expected.insert("manifest.json".into());
        verify_tree(&self.root, &self.root, &expected)
    }
}

fn read_contained(root: &Path, relative: &str, limit: u64) -> io::Result<Vec<u8>> {
    let mut current = root.to_path_buf();
    for part in Path::new(relative).components() {
        let Component::Normal(part) = part else {
            return Err(io::Error::other("invalid runtime payload path"));
        };
        current.push(part);
        if linked(&fs::symlink_metadata(&current)?) {
            return Err(io::Error::other("runtime payload contains a link"));
        }
    }
    let path = current.canonicalize()?;
    if !path.starts_with(root) {
        return Err(io::Error::other("runtime payload escapes installation"));
    }
    let file = fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit {
        return Err(io::Error::other(
            "runtime payload is empty, oversized or not regular",
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::other(
            "runtime payload grew beyond its size limit",
        ));
    }
    Ok(bytes)
}

fn linked(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn verify_tree(root: &Path, directory: &Path, expected: &HashSet<String>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let metadata = fs::symlink_metadata(&path)?;
        if linked(&metadata) {
            return Err(io::Error::other("runtime tree contains a link"));
        }
        if metadata.is_dir() {
            let relative = path
                .strip_prefix(root)
                .map_err(io::Error::other)?
                .to_string_lossy()
                .replace('\\', "/");
            if !expected
                .iter()
                .any(|name| name.starts_with(&format!("{relative}/")))
            {
                return Err(io::Error::other(
                    "runtime tree contains an unlisted directory",
                ));
            }
            verify_tree(root, &path, expected)?;
        } else if !metadata.is_file()
            || !expected.contains(
                &path
                    .strip_prefix(root)
                    .map_err(io::Error::other)?
                    .to_string_lossy()
                    .replace('\\', "/"),
            )
        {
            return Err(io::Error::other(
                "runtime tree contains an unlisted payload",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
