//! Role: Configure native workspace authority and transactional reconfiguration.
use crate::CancellationToken;
use crate::error::SessionError;
use crate::native;
use crate::registry::SessionRegistry;
use std::collections::{HashMap, HashSet};
pub struct WorkspaceConfigurationStatus {
    pub uri: String,
    pub enabled: bool,
    pub error: Option<String>,
}
impl SessionRegistry {
    pub fn configure_native_workspaces(
        &mut self,
        roots: Vec<(String, bool)>,
    ) -> Result<Vec<WorkspaceConfigurationStatus>, SessionError> {
        let generation = self
            .config_generation
            .checked_add(1)
            .ok_or_else(|| SessionError::Engine("configuration generation exhausted".into()))?;
        let mut seen = HashSet::new();
        if roots.iter().any(|(uri, _)| !seen.insert(uri.clone())) {
            return Err(SessionError::Engine("workspace URI is duplicated".into()));
        }
        let mut canonical_roots = HashSet::new();
        for (uri, _) in &roots {
            if let Ok(path) = native::workspace_path(uri) {
                if !canonical_roots.insert(path) {
                    return Err(SessionError::Engine(
                        "canonical workspace root is duplicated".into(),
                    ));
                }
            }
        }
        let mut configured = Vec::new();
        let mut statuses = Vec::new();
        let mut grants = Vec::new();
        for (uri, trusted) in roots {
            match native::configured_render_config(&uri, trusted, generation) {
                Ok(config) => {
                    configured.push((uri.clone(), config));
                    grants.push((uri.clone(), trusted, true));
                    statuses.push(WorkspaceConfigurationStatus {
                        uri,
                        enabled: true,
                        error: None,
                    });
                }
                Err(error) => {
                    grants.push((uri.clone(), false, false));
                    statuses.push(WorkspaceConfigurationStatus {
                        uri,
                        enabled: false,
                        error: Some(error.to_string()),
                    });
                }
            }
        }
        self.configure_workspaces(configured)?;
        for (uri, trusted, enabled) in grants {
            self.set_workspace_status(&uri, trusted, enabled);
        }
        self.config_generation = generation;
        Ok(statuses)
    }
    pub fn reconfigure_native_workspace(
        &mut self,
        daemon: &str,
        uri: &str,
        trusted: bool,
        cancellation: &CancellationToken,
    ) -> Result<(), SessionError> {
        self.verify_daemon(daemon)?;
        if !self
            .workspace_statuses()
            .iter()
            .any(|(root, _, _)| root == uri)
        {
            return Err(SessionError::Engine("workspace is not registered".into()));
        }
        if !trusted {
            self.workspaces.revoke(uri);
            let documents = self.workspace_documents(uri);
            let mut failure = None;
            for (document_uri, _) in documents {
                if let Some(session) = self.index.by_uri_mut(&document_uri) {
                    if let Err(error) = session
                        .engine
                        .set_render_config(fleximark_engine::RenderConfig::default())
                    {
                        failure = Some(error);
                    }
                }
            }
            if let Some(error) = failure {
                return Err(crate::error::engine_error(error));
            }
        }
        let generation = self
            .config_generation
            .checked_add(1)
            .ok_or_else(|| SessionError::Engine("configuration generation exhausted".into()))?;
        let config = native::configured_render_config(uri, trusted, generation)
            .map_err(|error| SessionError::Engine(error.to_string()))?;
        let mut assets = HashMap::new();
        for (document_uri, document) in self.workspace_documents(uri) {
            let resolved = if trusted {
                native::resolve_render_assets(&document_uri, uri, &document)
                    .map_err(|error| SessionError::Engine(error.to_string()))?
            } else {
                Default::default()
            };
            assets.insert(document_uri, resolved);
        }
        self.reconfigure_workspace(uri, config, assets, cancellation)?;
        self.set_workspace_status(uri, trusted, true);
        self.config_generation = generation;
        Ok(())
    }
}
