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
#[derive(Debug)]
pub enum ServiceOperationError {
    WrongDaemon,
    WriteDenied,
    Failed(String),
}
impl std::fmt::Display for ServiceOperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongDaemon => f.write_str("daemon instance does not match"),
            Self::WriteDenied => {
                f.write_str("workspace write denied: the active workspace is not trusted")
            }
            Self::Failed(message) => f.write_str(message),
        }
    }
}
impl crate::registry::SessionRegistry {
    pub fn execute_operation(
        &self,
        operation: WorkspaceOperation,
        request: OperationRequest<'_>,
        cancellation: &crate::CancellationToken,
    ) -> Result<OperationOutcome, ServiceOperationError> {
        use crate::native;
        use OperationOutcome as Out;
        use ServiceOperationError as Error;
        if request.daemon != self.daemon_instance_id() {
            return Err(Error::WrongDaemon);
        }
        let workspace = request
            .workspace
            .ok_or_else(|| Error::Failed("command requires workspaceUri".into()))?;
        if !matches!(operation, WorkspaceOperation::EditTheme)
            && !self.workspace_grants_write(workspace)
        {
            return Err(Error::WriteDenied);
        }
        let result: Result<Out, String> = (|| {
            let document = || -> Result<&crate::document::DocumentSession, String> {
                let session = request
                    .document_session
                    .ok_or("operation requires an active document")?;
                let version = request
                    .document_version
                    .ok_or("operation requires document version")?;
                let document = self
                    .document(request.daemon, session, version)
                    .map_err(|error| error.to_string())?;
                if document.workspace_uri() != Some(workspace) {
                    return Err("active document belongs to a different workspace authority".into());
                }
                Ok(document)
            };
            let failure = |error: native::ServiceError| error.to_string();
            match operation {
                WorkspaceOperation::Initialize => native::initialize_workspace(workspace)
                    .map(Out::Initialized)
                    .map_err(failure),
                WorkspaceOperation::EditTheme => native::edit_theme(workspace)
                    .map(Out::Theme)
                    .map_err(failure),
                WorkspaceOperation::CreateNote => native::create_note_with_options(
                    workspace,
                    request.category,
                    request.template,
                    request.name,
                )
                .map(Out::Note)
                .map_err(failure),
                WorkspaceOperation::CollectAdmonitions => {
                    let document = document()?;
                    native::collect_admonitions(document.document(), document.source(), workspace)
                        .map(Out::Admonitions)
                        .map_err(failure)
                }
                WorkspaceOperation::Export | WorkspaceOperation::AcknowledgeExport => {
                    let document = document()?;
                    let destination = request
                        .destination
                        .map(str::to_owned)
                        .map(Ok)
                        .unwrap_or_else(|| native::default_export_destination(&document.uri))
                        .map_err(failure)?;
                    if matches!(operation, WorkspaceOperation::Export) {
                        native::export_document(
                            document.engine(),
                            &document.uri,
                            workspace,
                            &destination,
                            cancellation,
                        )
                        .map(Out::Exported)
                        .map_err(failure)
                    } else {
                        native::acknowledge_export(&document.uri, workspace, &destination)
                            .map(|_| Out::ExportAcknowledged)
                            .map_err(failure)
                    }
                }
            }
        })();
        result.map_err(Error::Failed)
    }
}
