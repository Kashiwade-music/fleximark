//! Role: Own canonical workspace authority and validated configuration snapshots.
use crate::ApplicationError;
use fleximark_config::FlexiMarkConfig;
use fleximark_engine::RenderConfig;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceState {
    Restricted,
    Trusted,
    Invalid(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceStatus {
    pub uri: String,
    pub state: WorkspaceState,
    pub trusted: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceMembership {
    pub uri: String,
    pub trusted: bool,
}
#[derive(Clone)]
struct Workspace {
    uri: String,
    trusted: bool,
    lease: crate::CancellationToken,
    canonical_root: Option<PathBuf>,
    state: WorkspaceState,
    render: RenderConfig,
    config: Option<Arc<FlexiMarkConfig>>,
}
pub(super) enum PendingMigration {
    Plan(fleximark_migration::MigrationPlan),
    Recovery(PathBuf),
}
#[derive(Default)]
pub struct WorkspaceAuthority {
    rooted: Vec<Workspace>,
    failures: HashMap<String, Arc<ApplicationError>>,
    pub(super) migrations: HashMap<String, (String, PendingMigration)>,
    pub(super) next_plan: u64,
}
impl WorkspaceAuthority {
    pub(crate) fn worker_snapshot(&self, uri: Option<&str>) -> Self {
        let mut rooted = self
            .rooted
            .iter()
            .filter(|entry| Some(entry.uri.as_str()) == uri)
            .cloned()
            .collect::<Vec<_>>();
        for entry in &mut rooted {
            entry.lease = crate::CancellationToken::default();
        }
        Self {
            rooted,
            failures: self
                .failures
                .iter()
                .filter(|(root, _)| Some(root.as_str()) == uri)
                .map(|(root, error)| (root.clone(), Arc::clone(error)))
                .collect(),
            migrations: HashMap::new(),
            next_plan: self.next_plan,
        }
    }
    pub(crate) fn validate_execution_root(&self, uri: &str) -> Result<(), ApplicationError> {
        let entry = self
            .rooted
            .iter()
            .find(|entry| entry.uri == uri)
            .ok_or_else(|| ApplicationError::InvalidState("workspace is not registered".into()))?;
        let actual = Self::workspace_path(uri)?;
        if entry.canonical_root.as_ref() != Some(&actual) {
            return Err(ApplicationError::LinkedPath(
                "workspace root changed after authorization".into(),
            ));
        }
        Ok(())
    }
    pub(crate) fn operation_lease(&self, uri: &str) -> Option<crate::CancellationToken> {
        self.rooted
            .iter()
            .find(|entry| entry.uri == uri)
            .map(|entry| entry.lease.clone())
    }
    pub(crate) fn operation_matches(&self, uri: &str, lease: &crate::CancellationToken) -> bool {
        self.operation_lease(uri)
            .is_some_and(|current| current.same_scope(lease))
    }
    pub(crate) fn operation_current(&self, uri: &str, lease: &crate::CancellationToken) -> bool {
        self.operation_lease(uri)
            .is_some_and(|current| current.same_scope(lease) && !lease.is_cancelled())
    }
    pub(crate) fn merge_worker_root(&mut self, uri: &str, worker: &Self) {
        self.migrations.retain(|_, (root, _)| root != uri);
        if let (Some(current), Some(snapshot)) = (
            self.rooted.iter_mut().find(|entry| entry.uri == uri),
            worker.rooted.iter().find(|entry| entry.uri == uri),
        ) {
            current.lease.cancel();
            current.lease = crate::CancellationToken::default();
            current.config = snapshot.config.clone();
            current.render = snapshot.render.clone();
            current.state = snapshot.state.clone();
            self.failures.remove(uri);
            if let Some(error) = worker.failures.get(uri) {
                self.failures.insert(uri.into(), Arc::clone(error));
            }
        }
    }
    pub(crate) fn root_render(&self, uri: &str) -> Option<RenderConfig> {
        self.rooted
            .iter()
            .find(|entry| entry.uri == uri)
            .map(|entry| entry.render.clone())
    }
    pub(crate) fn add_restricted(
        &mut self,
        uri: String,
    ) -> Result<WorkspaceStatus, ApplicationError> {
        let uri = uri.trim_end_matches('/').to_owned();
        if let Some(existing) = self.statuses().into_iter().find(|status| status.uri == uri) {
            return Ok(existing);
        }
        let canonical = Self::workspace_path(&uri)?;
        if let Some(existing) = self
            .rooted
            .iter()
            .find(|entry| entry.canonical_root.as_ref() == Some(&canonical))
        {
            return Ok(WorkspaceStatus {
                uri: existing.uri.clone(),
                state: existing.state.clone(),
                trusted: existing.trusted,
            });
        }
        let config = Arc::new(FlexiMarkConfig::from_toml(
            crate::native::workspace::CONFIG,
        )?);
        self.rooted.push(Workspace {
            uri: uri.clone(),
            trusted: false,
            lease: crate::CancellationToken::default(),
            canonical_root: Some(canonical),
            state: WorkspaceState::Restricted,
            render: RenderConfig::default(),
            config: Some(config),
        });
        self.sort_roots();
        Ok(WorkspaceStatus {
            uri,
            state: WorkspaceState::Restricted,
            trusted: false,
        })
    }
    pub(crate) fn remove_root(&mut self, uri: &str) {
        for entry in self.rooted.iter().filter(|entry| entry.uri == uri) {
            entry.lease.cancel();
        }
        self.rooted.retain(|entry| entry.uri != uri);
        self.failures.remove(uri);
        self.migrations.retain(|_, (root, _)| root != uri);
    }
    pub fn path_to_file_uri(path: &Path) -> Result<String, ApplicationError> {
        crate::native::path_to_file_uri(path)
    }
    pub fn workspace_path(uri: &str) -> Result<PathBuf, ApplicationError> {
        crate::native::workspace_path(uri)
    }
    pub fn discover_workspace(uri: &str) -> Result<String, ApplicationError> {
        crate::native::workspace_for_document(uri)
    }
    pub fn default_export_destination(uri: &str) -> Result<String, ApplicationError> {
        crate::native::default_export_destination(uri)
    }
    pub fn statuses(&self) -> Vec<WorkspaceStatus> {
        self.rooted
            .iter()
            .map(|entry| WorkspaceStatus {
                uri: entry.uri.clone(),
                state: entry.state.clone(),
                trusted: entry.trusted,
            })
            .collect()
    }
    pub fn membership(&self, document_uri: &str) -> Option<WorkspaceMembership> {
        self.select(document_uri).map(|entry| WorkspaceMembership {
            uri: entry.uri.clone(),
            trusted: entry.trusted,
        })
    }
    pub fn cached_config(&self, uri: &str) -> Result<Arc<FlexiMarkConfig>, ApplicationError> {
        let uri = uri.trim_end_matches('/');
        if let Some(config) = self
            .rooted
            .iter()
            .find(|entry| entry.uri == uri)
            .and_then(|entry| entry.config.clone())
        {
            return Ok(config);
        }
        if let Some(error) = self.failures.get(uri) {
            return Err(ApplicationError::WorkspaceConfiguration(Arc::clone(error)));
        }
        Err(ApplicationError::InvalidState(
            "workspace configuration is not loaded".into(),
        ))
    }
    pub(super) fn record_shared_failure(&mut self, uri: &str, error: Arc<ApplicationError>) {
        self.failures
            .insert(uri.trim_end_matches('/').into(), error);
    }
    pub(super) fn record_failure(&mut self, uri: &str, error: ApplicationError) {
        self.failures
            .insert(uri.trim_end_matches('/').into(), Arc::new(error));
    }
    pub(super) fn replace_roots(
        &mut self,
        roots: Vec<(String, RenderConfig)>,
    ) -> Result<(), ApplicationError> {
        let mut canonical = std::collections::HashSet::new();
        for (uri, _) in &roots {
            if let Ok(path) = Self::workspace_path(uri) {
                if !canonical.insert(path) {
                    return Err(ApplicationError::InvalidState(
                        "canonical workspace root is duplicated".into(),
                    ));
                }
            }
        }
        let default = Arc::new(FlexiMarkConfig::from_toml(
            crate::native::workspace::CONFIG,
        )?);
        for entry in &self.rooted {
            entry.lease.cancel();
        }
        self.rooted = roots
            .into_iter()
            .map(|(uri, render)| Workspace {
                canonical_root: Self::workspace_path(&uri).ok(),
                uri: uri.trim_end_matches('/').to_owned(),
                trusted: true,
                lease: crate::CancellationToken::default(),
                state: WorkspaceState::Trusted,
                render,
                config: Some(Arc::clone(&default)),
            })
            .collect();
        self.migrations.clear();
        self.failures.clear();
        self.sort_roots();
        Ok(())
    }
    pub(super) fn revoke(&mut self, uri: &str) {
        if let Some(entry) = self.rooted.iter_mut().find(|entry| entry.uri == uri) {
            entry.lease.cancel();
            entry.lease = crate::CancellationToken::default();
            entry.trusted = false;
            entry.state = WorkspaceState::Restricted;
            entry.render = RenderConfig::default();
            entry.config = None;
        }
        self.migrations.retain(|_, (root, _)| root != uri);
    }
    pub(super) fn invalidate(&mut self, uri: &str) {
        self.failures.remove(uri.trim_end_matches('/'));
        if let Some(entry) = self.rooted.iter_mut().find(|entry| entry.uri == uri) {
            entry.lease.cancel();
            entry.lease = crate::CancellationToken::default();
            entry.config = None;
        }
        self.migrations.retain(|_, (root, _)| root != uri);
    }
    pub(super) fn is_trusted(&self, uri: &str) -> bool {
        self.rooted
            .iter()
            .find(|entry| entry.uri == uri.trim_end_matches('/'))
            .is_some_and(|entry| entry.trusted)
    }
    pub(super) fn set_state(&mut self, uri: &str, state: WorkspaceState) {
        if let Some(entry) = self
            .rooted
            .iter_mut()
            .find(|entry| entry.uri == uri.trim_end_matches('/'))
        {
            entry.trusted = match state {
                WorkspaceState::Trusted => true,
                WorkspaceState::Restricted => false,
                WorkspaceState::Invalid(_) => entry.trusted,
            };
            entry.state = state;
        } else {
            self.rooted.push(Workspace {
                uri: uri.trim_end_matches('/').to_owned(),
                trusted: matches!(state, WorkspaceState::Trusted),
                lease: crate::CancellationToken::default(),
                canonical_root: Self::workspace_path(uri).ok(),
                state,
                render: RenderConfig::default(),
                config: None,
            });
            self.sort_roots();
        }
    }
    pub(super) fn set_grant(&mut self, uri: &str, trusted: bool) {
        if let Some(entry) = self
            .rooted
            .iter_mut()
            .find(|entry| entry.uri == uri.trim_end_matches('/'))
        {
            entry.trusted = trusted;
        }
    }
    pub(super) fn install_config(&mut self, uri: &str, config: Arc<FlexiMarkConfig>) {
        self.failures.remove(uri.trim_end_matches('/'));
        if let Some(entry) = self
            .rooted
            .iter_mut()
            .find(|entry| entry.uri == uri.trim_end_matches('/'))
        {
            entry.config = Some(config);
        }
    }
    fn select(&self, uri: &str) -> Option<&Workspace> {
        let document = crate::native::canonical_document_path(uri).ok()?;
        self.rooted.iter().find(|entry| {
            entry
                .canonical_root
                .as_ref()
                .is_some_and(|root| document != *root && document.starts_with(root))
        })
    }
    pub(super) fn document_workspace(&self, uri: &str) -> Option<WorkspaceMembership> {
        self.membership(uri)
    }
    pub(super) fn matching(&self, uri: &str) -> Option<(&str, &RenderConfig)> {
        let entry = self.select(uri)?;
        if matches!(entry.state, WorkspaceState::Invalid(_)) {
            None
        } else {
            Some((&entry.uri, &entry.render))
        }
    }
    pub(super) fn replace(&mut self, uri: &str, render: RenderConfig) {
        if let Some(entry) = self.rooted.iter_mut().find(|entry| entry.uri == uri) {
            entry.render = render;
        } else {
            self.rooted.push(Workspace {
                uri: uri.into(),
                trusted: true,
                lease: crate::CancellationToken::default(),
                canonical_root: Self::workspace_path(uri).ok(),
                state: WorkspaceState::Trusted,
                render,
                config: None,
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
