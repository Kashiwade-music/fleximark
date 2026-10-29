//! Role: Configure native execution and explicitly apply recoverable migrations.
use super::ServiceError;
use crate::outcomes::{MigrationOutput, OperationError};
impl From<ServiceError> for OperationError {
    fn from(error: ServiceError) -> Self {
        match error {
            ServiceError::NotInitialized => Self::NotInitialized,
            ServiceError::InvalidWorkspaceUri => Self::InvalidWorkspaceUri,
            other => Self::Failed(other.to_string()),
        }
    }
}
pub fn same_workspace(left: &str, right: &str) -> bool {
    match (super::workspace_path(left), super::workspace_path(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}
pub fn migrate_workspace(workspace_uri: &str) -> Result<MigrationOutput, OperationError> {
    let workspace = crate::native::workspace_path(workspace_uri)?;
    let plan = fleximark_migration::plan(&workspace, None)
        .map_err(|e| OperationError::Failed(e.to_string()))?;
    let mut stdout =
        serde_json::to_string_pretty(&plan).map_err(|e| OperationError::Failed(e.to_string()))?;
    stdout.push('\n');
    if let Some(backup) = plan
        .apply()
        .map_err(|e| OperationError::Failed(e.to_string()))?
    {
        stdout.push_str(&format!(
            "Migration applied; original workspace data backed up at {}\n",
            backup.display()
        ));
    }
    Ok(MigrationOutput { stdout })
}
