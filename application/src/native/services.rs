//! Role: Configure native execution and explicitly apply recoverable migrations.
use super::ApplicationError;
use crate::outcomes::MigrationOutput;
pub fn same_workspace(left: &str, right: &str) -> bool {
    match (super::workspace_path(left), super::workspace_path(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}
pub fn migrate_workspace(workspace_uri: &str) -> Result<MigrationOutput, ApplicationError> {
    let workspace = crate::native::workspace_path(workspace_uri)?;
    let plan = fleximark_migration::plan(&workspace, None)?;
    let mut stdout = serde_json::to_string_pretty(&plan)?;
    stdout.push('\n');
    if let Some(backup) = plan.apply()? {
        stdout.push_str(&format!(
            "Migration applied; original workspace data backed up at {}\n",
            backup.display()
        ));
    }
    Ok(MigrationOutput { stdout })
}
