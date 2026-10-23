//! Role: Native workspace uri operations and validation.
use std::path::{Path, PathBuf};

use crate::native::ServiceError;

pub fn workspace_path(uri: &str) -> Result<PathBuf, ServiceError> {
    file_uri_path(uri)?.canonicalize().map_err(ServiceError::Io)
}

pub(crate) fn file_uri_path(uri: &str) -> Result<PathBuf, ServiceError> {
    let encoded = uri
        .strip_prefix("file://")
        .ok_or(ServiceError::InvalidWorkspaceUri)?;
    let decoded = percent_decode(encoded)?;
    #[cfg(windows)]
    let decoded = decoded.strip_prefix('/').unwrap_or(&decoded);
    let path = PathBuf::from(decoded);
    if !path.is_absolute() {
        return Err(ServiceError::InvalidWorkspaceUri);
    }
    Ok(path)
}

pub fn document_is_in_workspace(
    document_uri: &str,
    workspace_uri: &str,
) -> Result<bool, ServiceError> {
    let document = match canonical_document_path(document_uri) {
        Ok(document) => document,
        Err(ServiceError::InvalidWorkspaceUri) if document_uri.starts_with("file://") => {
            return Ok(false);
        }
        Err(error) => return Err(error),
    };
    let workspace = workspace_path(workspace_uri)?;
    Ok(document != workspace && document.starts_with(workspace))
}

pub(crate) fn canonical_document_path(document_uri: &str) -> Result<PathBuf, ServiceError> {
    let document = file_uri_path(document_uri)?;
    if document.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::CurDir
        )
    }) {
        return Err(ServiceError::InvalidWorkspaceUri);
    }
    let mut ancestor = document.as_path();
    let mut missing = Vec::new();
    let document = loop {
        match ancestor.canonicalize() {
            Ok(mut resolved) => {
                for component in missing.iter().rev() {
                    resolved.push(component);
                }
                break resolved;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(name) = ancestor.file_name() else {
                    return Err(error.into());
                };
                missing.push(name.to_owned());
                let Some(parent) = ancestor.parent() else {
                    return Err(error.into());
                };
                ancestor = parent;
            }
            Err(error) => return Err(error.into()),
        }
    };
    Ok(document)
}

pub fn path_to_file_uri(path: &Path) -> Result<String, ServiceError> {
    let path = match path.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .ok_or(ServiceError::InvalidWorkspaceUri)?
                .canonicalize()?;
            parent.join(path.file_name().ok_or(ServiceError::InvalidWorkspaceUri)?)
        }
        Err(error) => return Err(error.into()),
    };
    path_to_file_uri_unchecked(&path)
}

pub(crate) fn path_to_file_uri_unchecked(path: &Path) -> Result<String, ServiceError> {
    if !path.is_absolute() {
        return Err(ServiceError::InvalidWorkspaceUri);
    }
    let normalized = path.to_string_lossy().replace('\\', "/");
    #[cfg(windows)]
    let normalized = normalized
        .strip_prefix("//?/")
        .unwrap_or(&normalized)
        .to_owned();
    let encoded = normalized
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect::<String>();
    Ok(if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    })
}

pub(crate) fn percent_decode(value: &str) -> Result<String, ServiceError> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = bytes
                .get(index + 1..index + 3)
                .ok_or(ServiceError::InvalidWorkspaceUri)?;
            let text = std::str::from_utf8(hex).map_err(|_| ServiceError::InvalidWorkspaceUri)?;
            let byte =
                u8::from_str_radix(text, 16).map_err(|_| ServiceError::InvalidWorkspaceUri)?;
            if byte == 0 {
                return Err(ServiceError::InvalidWorkspaceUri);
            }
            output.push(byte);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).map_err(|_| ServiceError::InvalidWorkspaceUri)
}

#[cfg(test)]
mod tests {
    //! Role: Verify workspace operations and security invariants.
    use super::*;
    #[test]
    fn rejects_non_file_workspace_uris() {
        assert!(matches!(
            workspace_path("https://example.test/x"),
            Err(ServiceError::InvalidWorkspaceUri)
        ));
    }
    #[test]
    fn new_documents_inherit_only_canonical_contained_workspace_authority() {
        let root = crate::native::test_support::test_workspace("membership");
        let outside = crate::native::test_support::test_workspace("membership-outside");
        let workspace = path_to_file_uri(&root).unwrap();
        let new_document = format!("{workspace}/new/sub/document.md");
        assert!(document_is_in_workspace(&new_document, &workspace).unwrap());
        assert!(
            !document_is_in_workspace(&format!("{workspace}/../escape.md"), &workspace).unwrap()
        );
        assert!(
            !document_is_in_workspace(&format!("{workspace}/%2E%2E/escape.md"), &workspace)
                .unwrap()
        );
        assert!(document_is_in_workspace("untitled:new", &workspace).is_err());
        assert!(
            document_is_in_workspace(&new_document, "file:///definitely-missing-workspace")
                .is_err()
        );
        let linked = root.join("outside");
        #[cfg(unix)]
        let linked_result = std::os::unix::fs::symlink(&outside, &linked);
        #[cfg(windows)]
        let linked_result = std::os::windows::fs::symlink_dir(&outside, &linked);
        if linked_result.is_ok() {
            assert!(
                !document_is_in_workspace(
                    &format!("{workspace}/outside/missing/new.md"),
                    &workspace
                )
                .unwrap()
            );
            std::fs::remove_dir(&linked).unwrap();
        }
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }
}
