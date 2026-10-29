//! Role: Reload workspace configuration and commit document render policies together.
use crate::{ApplicationError, CancellationToken, DocumentStore, WorkspaceState, WorkspaceStatus};
use std::collections::{HashMap, HashSet};
impl DocumentStore {
    pub fn configure_native_workspaces(
        &mut self,
        roots: Vec<(String, bool)>,
    ) -> Result<Vec<WorkspaceStatus>, ApplicationError> {
        let generation = self.config_generation.checked_add(1).ok_or_else(|| {
            ApplicationError::InternalState("configuration generation exhausted".into())
        })?;
        let mut uris = HashSet::new();
        let mut paths = HashSet::new();
        for (uri, _) in &roots {
            if !uris.insert(uri.clone()) {
                return Err(ApplicationError::InvalidState(
                    "workspace URI is duplicated".into(),
                ));
            }
            if let Ok(path) = crate::native::workspace_path(uri) {
                if !paths.insert(path) {
                    return Err(ApplicationError::InvalidState(
                        "canonical workspace root is duplicated".into(),
                    ));
                }
            }
        }
        let mut loaded = Vec::new();
        let mut failures = Vec::new();
        let mut configured = Vec::new();
        for (uri, trusted) in roots {
            match crate::native::load_configuration(&uri, trusted, generation) {
                Ok((config, render)) => {
                    configured.push((uri.clone(), render));
                    loaded.push((
                        uri,
                        trusted,
                        if trusted {
                            WorkspaceState::Trusted
                        } else {
                            WorkspaceState::Restricted
                        },
                        Some(config),
                    ));
                }
                Err(error) => {
                    let message = error.to_string();
                    failures.push((uri.clone(), error));
                    configured.push((uri.clone(), Default::default()));
                    loaded.push((uri, trusted, WorkspaceState::Invalid(message), None));
                }
            }
        }
        self.configure_workspaces(configured)?;
        for (uri, trusted, state, config) in loaded {
            self.workspaces.set_state(&uri, state);
            self.workspaces.set_grant(&uri, trusted);
            if let Some(config) = config {
                self.workspaces.install_config(&uri, config);
            } else {
                self.workspaces.invalidate(&uri);
            }
        }
        for (uri, error) in failures {
            self.workspaces.record_failure(&uri, error);
        }
        self.config_generation = generation;
        Ok(self.workspace_statuses())
    }
    pub fn reconfigure_native_workspace(
        &mut self,
        uri: &str,
        trusted: bool,
        cancellation: &CancellationToken,
    ) -> Result<(), ApplicationError> {
        if !self
            .workspace_statuses()
            .iter()
            .any(|status| status.uri == uri)
        {
            return Err(ApplicationError::InvalidState(
                "workspace is not registered".into(),
            ));
        }
        self.workspaces.invalidate(uri);
        for (document_uri, _) in self.workspace_documents(uri) {
            if let Some(session) = self.index.by_uri_mut(&document_uri) {
                session.asset_memo.clear();
            }
        }
        if !trusted {
            self.workspaces.revoke(uri);
            for (document_uri, _) in self.workspace_documents(uri) {
                if let Some(session) = self.index.by_uri_mut(&document_uri) {
                    session.asset_memo.clear();
                    session.engine.set_render_config(Default::default())?;
                    for preview in session.previews.values() {
                        preview.invalidate_publication();
                    }
                }
            }
        }
        let generation = self.config_generation.checked_add(1).ok_or_else(|| {
            ApplicationError::InternalState("configuration generation exhausted".into())
        })?;
        let (config, render) = match crate::native::load_configuration(uri, trusted, generation) {
            Ok(value) => value,
            Err(error) => {
                self.workspaces
                    .set_state(uri, WorkspaceState::Invalid(error.to_string()));
                let error = std::sync::Arc::new(error);
                self.workspaces
                    .record_shared_failure(uri, std::sync::Arc::clone(&error));
                return Err(ApplicationError::WorkspaceConfiguration(error));
            }
        };
        let mut assets = HashMap::new();
        for (document_uri, document) in self.workspace_documents(uri) {
            assets.insert(
                document_uri.clone(),
                if trusted {
                    crate::native::resolve_render_assets(&document_uri, uri, &document, &config)?
                } else {
                    Default::default()
                },
            );
        }
        self.reconfigure_workspace(uri, render, assets, cancellation)?;
        self.workspaces.install_config(uri, config);
        self.workspaces.set_state(
            uri,
            if trusted {
                WorkspaceState::Trusted
            } else {
                WorkspaceState::Restricted
            },
        );
        self.config_generation = generation;
        Ok(())
    }
}
