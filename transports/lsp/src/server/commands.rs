//! Role: Dispatch typed commands once and map application outcomes to wire results.
use super::state::*;
use super::wire::*;
use fleximark_protocol::{CommandResult, WorkspaceCommandRequest};
pub(super) fn note_options(
    options: fleximark_application::NoteOptions,
) -> fleximark_protocol::GetNoteOptionsResult {
    fn category(
        value: fleximark_application::NoteCategory,
    ) -> fleximark_protocol::NoteCategoryOption {
        fleximark_protocol::NoteCategoryOption {
            name: value.name,
            children: value.children.into_iter().map(category).collect(),
        }
    }
    fleximark_protocol::GetNoteOptionsResult {
        categories: options.categories.into_iter().map(category).collect(),
        templates: options.templates,
    }
}
impl Server {
    pub(super) fn workspace_ui_url(
        &self,
        request: fleximark_protocol::WorkspaceStateRequest,
    ) -> Result<String, ApplicationError> {
        Ok(self.previews.publish_workspace(&random_token()?, request))
    }
    pub(crate) fn execute_command(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let id = id?;
        let params = match deserialize_params::<WorkspaceCommandRequest>(&id, params) {
            Ok(params) => params,
            Err(response) => return Some(response),
        };
        Some(match self.dispatch_command(params, false) {
            Ok(result) => response_value(Response::success(id, result)),
            Err(error) => session_error(id, error),
        })
    }
    pub(super) fn dispatch_command(
        &mut self,
        mut request: WorkspaceCommandRequest,
        browser: bool,
    ) -> Result<CommandResult, ApplicationError> {
        self.verify_daemon(&request.daemon_instance_id)?;
        if browser && !request.operation.descriptor().browser_ui {
            return Err(ApplicationError::InvalidState(
                "command is not available in the browser UI".into(),
            ));
        }
        if let Some(document) = &request.context.document {
            let version = u64::try_from(document.expected_document_version.get())
                .map_err(|_| ApplicationError::VersionMismatch)?;
            let document = self
                .registry
                .document(&document.document_session_id, version)?;
            if request.context.workspace_uri.is_none() {
                request.context.workspace_uri = document.workspace_uri().map(str::to_owned);
            } else if document.workspace_uri() != request.context.workspace_uri.as_deref() {
                return Err(ApplicationError::InvalidState(
                    "active document belongs to a different workspace authority".into(),
                ));
            }
        }
        let context = request.context.clone();
        let outcome = fleximark_application::WorkspaceOps::new(
            &mut self.registry,
            self.preview_assets.clone(),
        )
        .execute(request.operation, request.context, &self.work_cancellation)?;
        use fleximark_application::CommandOutcome as Out;
        let result = match outcome {
            Out::Initialized(value) => CommandResult::WorkspaceInitialized {
                config_uri: value.config_uri,
                theme_uri: value.theme_uri,
                message: None,
            },
            Out::Theme(value) => CommandResult::ThemeEdited {
                uri: value.uri,
                message: None,
            },
            Out::Note(value) => CommandResult::NoteCreated {
                uri: value.uri,
                message: None,
            },
            Out::NoteOptions(value) => CommandResult::NoteOptions {
                options: note_options(value),
                message: None,
            },
            Out::Exported(value) => CommandResult::HtmlExported {
                output_uri: value.output_uri,
                generation: super::protocol::counter(value.generation)?,
                message: None,
            },
            Out::ExportAcknowledged => CommandResult::ExportAcknowledged { message: None },
            Out::Admonitions(value) => CommandResult::AdmonitionsCollected {
                uri: value.uri,
                count: super::protocol::counter(value.count as u64)?,
                message: None,
            },
            Out::MigrationPlanned(plan) => CommandResult::MigrationPlanned {
                plan,
                message: None,
            },
            Out::MigrationApplied(value) => CommandResult::MigrationApplied {
                backup_uri: value.backup_uri,
                recovered: value.recovered,
                message: None,
            },
            Out::OpenPreview {
                document_session_id,
                document_version,
                target,
                source_offset,
            } => {
                let preview = self.open_preview(
                    &document_session_id,
                    document_version,
                    target,
                    source_offset,
                )?;
                CommandResult::PreviewOpened {
                    preview,
                    message: None,
                }
            }
            Out::OpenWorkspace { workspace_uri } => {
                let ui_context = self.ui_context(&context)?;
                let url = self.workspace_ui_url(fleximark_protocol::WorkspaceStateRequest {
                    context: fleximark_protocol::WorkspaceUiContext {
                        workspace_uri: Some(workspace_uri),
                        ..ui_context
                    },
                    command: None,
                })?;
                CommandResult::WorkspaceOpened { url, message: None }
            }
        };
        match &result {
            CommandResult::WorkspaceInitialized { config_uri, .. } => {
                self.show_document(config_uri, false, None)
            }
            CommandResult::ThemeEdited { uri, .. }
            | CommandResult::NoteCreated { uri, .. }
            | CommandResult::AdmonitionsCollected { uri, .. } => {
                self.show_document(uri, false, None)
            }
            CommandResult::HtmlExported { output_uri, .. } => {
                self.show_document(output_uri, false, None)
            }
            CommandResult::PreviewOpened { preview, .. } => {
                if let Some(url) = &preview.url {
                    self.show_document(url, true, None);
                }
            }
            CommandResult::WorkspaceOpened { url, .. } => self.show_document(url, true, None),
            _ => {}
        }
        Ok(result)
    }
}
