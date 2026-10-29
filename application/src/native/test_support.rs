//! Role: Seed native test workspaces using the shared directory owner.
pub(crate) fn test_workspace(label: &str) -> fleximark_test_support::TempWorkspace {
    fleximark_test_support::TempWorkspace::new(label).unwrap()
}

#[cfg(windows)]
pub(crate) struct JunctionGuard(std::path::PathBuf);

#[cfg(windows)]
impl Drop for JunctionGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0);
    }
}

#[cfg(windows)]
pub(crate) fn create_junction(path: &std::path::Path, target: &std::path::Path) -> JunctionGuard {
    use std::process::Command;

    let status = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(path)
        .arg(target)
        .status()
        .expect("invoke mklink");
    assert!(status.success(), "create required test junction");
    JunctionGuard(path.to_owned())
}
