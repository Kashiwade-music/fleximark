use std::path::{Path, PathBuf};

use crate::ServiceError;
use crate::workspace::validate_config;

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
    let document = workspace_path(document_uri)?;
    let workspace = workspace_path(workspace_uri)?;
    Ok(document != workspace && document.starts_with(workspace))
}

pub fn workspace_for_document(document_uri: &str) -> Result<String, ServiceError> {
    let document = workspace_path(document_uri)?;
    for parent in document
        .parent()
        .ok_or(ServiceError::InvalidWorkspaceUri)?
        .ancestors()
    {
        if parent.join(".fleximark/config.toml").is_file() {
            validate_config(&parent.join(".fleximark/config.toml"))?;
            return path_to_file_uri(parent);
        }
    }
    Err(ServiceError::NotInitialized)
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
    use super::*;
    #[test]
    fn rejects_non_file_workspace_uris() {
        assert!(matches!(
            workspace_path("https://example.test/x"),
            Err(ServiceError::InvalidWorkspaceUri)
        ));
    }
}
