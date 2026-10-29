//! Role: Verify distribution integrity, strict formats and payload containment.
use super::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "fleximark-distribution-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let release = release_metadata().unwrap();
        let target = &specification().unwrap().targets[0].triple;
        let mut files = BTreeMap::new();
        for relative in specification().unwrap().paths(target).unwrap() {
            let path = root.join(&relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, relative.as_bytes()).unwrap();
            files.insert(relative.clone(), sha256(relative.as_bytes()));
        }
        let manifest = RuntimeManifest {
            schema_version: 1,
            version: release.version,
            protocol_version: release.protocol_version,
            target: target.clone(),
            files,
        };
        fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        Self(root)
    }
    fn open(&self) -> io::Result<RuntimeDistribution> {
        RuntimeDistribution::open(
            &self.0,
            &release_metadata()?,
            &specification()?.targets[0].triple,
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn canonical_targets_and_files_have_unique_identities() {
    let spec = specification().unwrap();
    assert!(!spec.targets.is_empty());
    let triples = spec
        .targets
        .iter()
        .map(|target| &target.triple)
        .collect::<HashSet<_>>();
    let names = spec
        .targets
        .iter()
        .map(|target| &target.vscode)
        .collect::<HashSet<_>>();
    assert_eq!(triples.len(), spec.targets.len());
    assert_eq!(names.len(), spec.targets.len());
    for target in &spec.targets {
        let paths = spec.paths(&target.triple).unwrap();
        assert_eq!(paths.iter().collect::<HashSet<_>>().len(), spec.files.len());
        assert_eq!(
            spec.platform(&target.os, &target.arch).unwrap().triple,
            target.triple
        );
        assert!(
            paths
                .iter()
                .all(|path| !path.contains('{') && !path.contains('}'))
        );
    }
}

#[test]
fn release_metadata_rejects_missing_duplicate_unknown_and_invalid_versions() {
    assert_eq!(
        ReleaseMetadata::parse("version = \"1.2.3-rc.1+build\"\nprotocol_version = 1\n")
            .unwrap()
            .protocol_version,
        1
    );
    for source in [
        "version = \"1.2\"\nprotocol_version = 1",
        "version = \"01.2.3\"\nprotocol_version = 1",
        "version = \"1.2.3\"\nprotocol_version = 0",
        "version = \"1.2.3\"",
        "version = \"1.2.3\"\nversion = \"1.2.4\"\nprotocol_version = 1",
        "version = \"1.2.3\"\nprotocol_version = 1\nunknown = true",
    ] {
        assert!(ReleaseMetadata::parse(source).is_err(), "{source}");
    }
}

#[test]
fn reads_recheck_hashes_and_reject_unlisted_payloads() {
    let fixture = Fixture::new();
    let runtime = fixture.open().unwrap();
    runtime.verify().unwrap();
    let relative = "resources/preview/browser-host.js";
    assert_eq!(runtime.read(relative).unwrap(), relative.as_bytes());
    assert!(runtime.read("../manifest.json").is_err());
    assert!(runtime.read("manifest.json").is_err());
    fs::write(fixture.0.join(relative), "changed").unwrap();
    assert!(runtime.read(relative).is_err());
    assert!(runtime.verify().is_err());
}

#[test]
fn validation_rejects_old_fields_wrong_targets_missing_and_extra_payloads() {
    let fixture = Fixture::new();
    let manifest = fs::read(fixture.0.join("manifest.json")).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
    value["legacy"] = true.into();
    fs::write(
        fixture.0.join("manifest.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    assert!(fixture.open().is_err());
    fs::write(fixture.0.join("manifest.json"), manifest).unwrap();
    let runtime = fixture.open().unwrap();
    let mut wrong = runtime.manifest().clone();
    wrong.target = "unsupported".into();
    assert!(
        wrong
            .validate(&release_metadata().unwrap(), "unsupported")
            .is_err()
    );
    wrong = runtime.manifest().clone();
    wrong.files.remove("resources/preview/katex.css");
    assert!(
        wrong
            .validate(&release_metadata().unwrap(), &wrong.target)
            .is_err()
    );
    fs::write(fixture.0.join("unlisted.js"), "extra").unwrap();
    assert!(runtime.verify().is_err());
}

#[test]
fn sparse_oversized_files_are_rejected_before_reading() {
    let fixture = Fixture::new();
    let runtime = fixture.open().unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(fixture.0.join("bin/fleximarkd"))
        .unwrap()
        .set_len(specification().unwrap().limits.file_bytes + 1)
        .unwrap();
    assert!(runtime.read("bin/fleximarkd").is_err());
}

#[cfg(unix)]
#[test]
fn linked_payload_directories_are_rejected_even_with_matching_hashes() {
    let fixture = Fixture::new();
    let outside = Fixture::new();
    let runtime = fixture.open().unwrap();
    fs::remove_dir_all(fixture.0.join("resources/preview")).unwrap();
    std::os::unix::fs::symlink(
        outside.0.join("resources/preview"),
        fixture.0.join("resources/preview"),
    )
    .unwrap();
    assert!(runtime.read("resources/preview/browser-host.js").is_err());
}
