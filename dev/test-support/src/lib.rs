//! Role: Own isolated filesystem fixtures for Rust test suites.
use std::fs;
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// An isolated directory removed when its owning fixture leaves scope.
#[derive(Debug)]
pub struct TempWorkspace(PathBuf);
impl TempWorkspace {
    pub fn new(label: &str) -> io::Result<Self> {
        let label: String = label
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        loop {
            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_nanos();
            let serial = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "fleximark-{label}-{}-{time}-{serial}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }
}
impl AsRef<Path> for TempWorkspace {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}
impl Deref for TempWorkspace {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    //! Role: Verify isolation and cleanup of shared filesystem fixtures.
    use super::*;
    #[test]
    fn parallel_same_label_fixtures_are_distinct_and_removed_on_drop() -> io::Result<()> {
        let workers: Vec<_> = (0..16)
            .map(|_| std::thread::spawn(|| TempWorkspace::new("parallel")))
            .collect();
        let mut owners = Vec::new();
        for worker in workers {
            let owner = worker
                .join()
                .map_err(|_| io::Error::other("fixture worker failed"))??;
            fs::write(owner.join("sentinel"), "owned")?;
            owners.push(owner);
        }
        let paths: Vec<_> = owners.iter().map(|owner| owner.to_path_buf()).collect();
        let distinct: std::collections::HashSet<_> = paths.iter().collect();
        assert_eq!(distinct.len(), paths.len());
        drop(owners);
        assert!(paths.iter().all(|path| !path.exists()));
        Ok(())
    }
    #[test]
    fn error_return_removes_partial_fixture() -> io::Result<()> {
        fn fail(path: &mut PathBuf) -> io::Result<()> {
            let owner = TempWorkspace::new("early-return")?;
            *path = owner.to_path_buf();
            fs::write(owner.join("partial"), "data")?;
            Err(io::Error::other("injected fixture failure"))
        }
        let mut path = PathBuf::new();
        assert!(fail(&mut path).is_err());
        assert!(!path.exists());
        Ok(())
    }
}
