//! Role: Associate workspace roots with trust and render policies.

use fleximark_engine::RenderConfig;

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorkspaceRoot(String);

pub struct Workspace {
    pub trusted: bool,
    pub enabled: bool,
    root: WorkspaceRoot,
    canonical_root: Option<std::path::PathBuf>,
    render: RenderConfig,
}

#[derive(Default)]
pub(super) struct WorkspaceAuthority {
    rooted: Vec<Workspace>,
}

impl WorkspaceAuthority {
    pub(super) fn replace_roots(
        &mut self,
        workspaces: Vec<(String, RenderConfig)>,
    ) -> Result<(), crate::error::ApplicationError> {
        let mut canonical_roots = std::collections::HashSet::new();
        for (uri, _) in &workspaces {
            if let Ok(path) = crate::native::workspace_path(uri) {
                if !canonical_roots.insert(path) {
                    return Err(crate::error::ApplicationError::InvalidState(
                        "canonical workspace root is duplicated".into(),
                    ));
                }
            }
        }
        self.rooted = workspaces
            .into_iter()
            .map(|(uri, render)| Workspace {
                trusted: true,
                enabled: true,
                canonical_root: crate::native::workspace_path(&uri).ok(),
                root: WorkspaceRoot(uri.trim_end_matches('/').to_owned()),
                render,
            })
            .collect();
        self.sort_roots();
        Ok(())
    }

    pub(super) fn revoke(&mut self, uri: &str) {
        if let Some(entry) = self.rooted.iter_mut().find(|entry| entry.root.0 == uri) {
            entry.trusted = false;
            entry.render = RenderConfig::default();
        }
    }
    pub(super) fn grants_write(&self, uri: &str) -> bool {
        let Ok(requested) = crate::native::workspace_path(uri) else {
            return false;
        };
        self.rooted
            .iter()
            .find(|entry| entry.canonical_root.as_ref() == Some(&requested))
            .is_some_and(|entry| entry.trusted && entry.enabled)
    }
    pub(super) fn status(&self) -> Vec<(String, bool, bool)> {
        self.rooted
            .iter()
            .map(|entry| (entry.root.0.clone(), entry.trusted, entry.enabled))
            .collect()
    }
    pub(super) fn set_status(&mut self, uri: &str, trusted: bool, enabled: bool) {
        if let Some(entry) = self
            .rooted
            .iter_mut()
            .find(|entry| entry.root.0 == uri.trim_end_matches('/'))
        {
            entry.trusted = trusted;
            entry.enabled = enabled;
        } else {
            self.rooted.push(Workspace {
                root: WorkspaceRoot(uri.trim_end_matches('/').to_owned()),
                canonical_root: crate::native::workspace_path(uri).ok(),
                trusted,
                enabled,
                render: RenderConfig::default(),
            });
            self.sort_roots();
        }
    }
    fn select(&self, document_uri: &str) -> Option<&Workspace> {
        let document = crate::native::canonical_document_path(document_uri).ok()?;
        self.rooted.iter().find(|entry| {
            entry
                .canonical_root
                .as_ref()
                .is_some_and(|root| document != *root && document.starts_with(root))
        })
    }
    pub(super) fn document_workspace(&self, document_uri: &str) -> Option<(String, bool)> {
        self.select(document_uri)
            .map(|entry| (entry.root.0.clone(), entry.trusted))
    }
    pub(super) fn matching(&self, document_uri: &str) -> Option<(&str, &RenderConfig)> {
        let entry = self.select(document_uri)?;
        if !entry.enabled {
            return None;
        }
        Some((entry.root.0.as_str(), &entry.render))
    }

    pub(super) fn replace(&mut self, workspace_uri: &str, render: RenderConfig) {
        if let Some(entry) = self
            .rooted
            .iter_mut()
            .find(|entry| entry.root.0 == workspace_uri)
        {
            entry.render = render;
        } else {
            self.rooted.push(Workspace {
                trusted: true,
                enabled: true,
                canonical_root: crate::native::workspace_path(workspace_uri).ok(),
                root: WorkspaceRoot(workspace_uri.to_owned()),
                render,
            });
            self.sort_roots();
        }
    }

    fn sort_roots(&mut self) {
        self.rooted.sort_by_key(|entry| {
            std::cmp::Reverse(
                entry
                    .canonical_root
                    .as_ref()
                    .map_or(0, |root| root.components().count()),
            )
        });
    }
}
