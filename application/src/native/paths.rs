//! Role: Reject linked path components before native workspace access.
use crate::native::ServiceError;
use std::{fs, path::Path};
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
