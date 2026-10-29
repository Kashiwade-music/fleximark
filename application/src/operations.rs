//! Role: Authorize and orchestrate typed workspace operations.
pub use crate::outcomes::*;
#[derive(Clone, Copy)]
pub enum WorkspaceOperation {
    Initialize,
    EditTheme,
    CreateNote,
    Export,
    AcknowledgeExport,
    CollectAdmonitions,
}
pub struct OperationRequest<'a> {
    pub daemon: &'a str,
    pub workspace: Option<&'a str>,
    pub document_session: Option<&'a str>,
    pub document_version: Option<i64>,
    pub destination: Option<&'a str>,
    pub category: Option<&'a [String]>,
    pub template: Option<&'a str>,
    pub name: Option<&'a str>,
}
pub enum OperationOutcome {
    Initialized(InitializedWorkspace),
    Theme(ThemeResource),
    Note(CreatedNote),
    Exported(ExportReceipt),
    ExportAcknowledged,
    Admonitions(CollectedAdmonitions),
}
use crate::ApplicationError;
impl crate::registry::SessionRegistry {
    pub fn execute_operation(
        &self,
        operation: WorkspaceOperation,
        request: OperationRequest<'_>,
        cancellation: &crate::CancellationToken,
    ) -> Result<OperationOutcome, ApplicationError> {
        use crate::native;
        use OperationOutcome as Out;

        if request.daemon != self.daemon_instance_id() {
            return Err(ApplicationError::WrongDaemon);
        }
        let workspace = request.workspace.ok_or_else(|| {
            ApplicationError::InvalidState("command requires workspaceUri".into())
        })?;
        if !matches!(operation, WorkspaceOperation::EditTheme)
            && !self.workspace_grants_write(workspace)
        {
            return Err(ApplicationError::WriteDenied);
        }

        let document = || -> Result<&crate::document::DocumentSession, ApplicationError> {
            let session = request.document_session.ok_or_else(|| {
                ApplicationError::InvalidState("operation requires an active document".into())
            })?;
            let version = request.document_version.ok_or_else(|| {
                ApplicationError::InvalidState("operation requires document version".into())
            })?;
            let document = self.document(request.daemon, session, version)?;
            if document.workspace_uri() != Some(workspace) {
                return Err(ApplicationError::InvalidState(
                    "active document belongs to a different workspace authority".into(),
                ));
            }
            Ok(document)
        };
        match operation {
            WorkspaceOperation::Initialize => {
                native::initialize_workspace(workspace).map(Out::Initialized)
            }
            WorkspaceOperation::EditTheme => native::edit_theme(workspace).map(Out::Theme),
            WorkspaceOperation::CreateNote => native::create_note_with_options(
                workspace,
                request.category,
                request.template,
                request.name,
            )
            .map(Out::Note),
            WorkspaceOperation::CollectAdmonitions => {
                let document = document()?;
                native::collect_admonitions(document.document(), document.source(), workspace)
                    .map(Out::Admonitions)
            }
            WorkspaceOperation::Export | WorkspaceOperation::AcknowledgeExport => {
                let document = document()?;
                let destination = request
                    .destination
                    .map(str::to_owned)
                    .map(Ok)
                    .unwrap_or_else(|| native::default_export_destination(&document.uri))?;
                if matches!(operation, WorkspaceOperation::Export) {
                    native::export_document(
                        document.engine(),
                        &document.uri,
                        workspace,
                        &destination,
                        cancellation,
                    )
                    .map(Out::Exported)
                } else {
                    native::acknowledge_export(&document.uri, workspace, &destination)
                        .map(|_| Out::ExportAcknowledged)
                }
            }
        }
    }
}
