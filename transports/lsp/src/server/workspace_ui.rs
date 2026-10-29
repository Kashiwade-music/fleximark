//! Role: Resolve browser document context and return typed workspace UI state.
use super::state::*;
use fleximark_protocol::{
    CommandContext, DocumentCommandContext, WorkspaceCommand, WorkspaceCommandRequest,
    WorkspaceStateRequest, WorkspaceStateResult, WorkspaceUiContext, WorkspaceUiDocument,
    WorkspaceUiWorkspace,
};
impl Server {
    pub fn workspace_request(
        &mut self,
        request: crate::preview_http::WorkspaceRequest,
    ) -> Result<crate::preview_http::WorkspaceResponse, ApplicationError> {
        if !self.lsp_initialized {
            return Err(ApplicationError::InvalidState(
                "server is not initialized".into(),
            ));
        }
        match request {
            crate::preview_http::WorkspaceRequest::State(request) => self
                .workspace_state(request)
                .map(crate::preview_http::WorkspaceResponse::State),
            crate::preview_http::WorkspaceRequest::Command(request) => {
                let context = self.resolve_ui_context(request.context)?;
                let result = self.dispatch_command(
                    WorkspaceCommandRequest {
                        daemon_instance_id: self.daemon_instance_id().to_owned(),
                        context,
                        operation: request.operation,
                    },
                    true,
                )?;
                Ok(crate::preview_http::WorkspaceResponse::Command(result))
            }
        }
    }
    pub(super) fn ui_context(
        &self,
        context: &CommandContext,
    ) -> Result<WorkspaceUiContext, ApplicationError> {
        let (document_uri, document_version) = if let Some(document) = &context.document {
            let version = u64::try_from(document.expected_document_version.get())
                .map_err(|_| ApplicationError::VersionMismatch)?;
            let session = self
                .registry
                .document(&document.document_session_id, version)?;
            (
                Some(session.uri.clone()),
                Some(document.expected_document_version),
            )
        } else {
            (None, None)
        };
        Ok(WorkspaceUiContext {
            workspace_uri: context.workspace_uri.clone(),
            document_uri,
            document_version,
        })
    }
    fn resolve_ui_context(
        &self,
        context: WorkspaceUiContext,
    ) -> Result<CommandContext, ApplicationError> {
        let document = match (context.document_uri.as_deref(), context.document_version) {
            (Some(uri), Some(version)) => {
                let native =
                    u64::try_from(version.get()).map_err(|_| ApplicationError::VersionMismatch)?;
                let attached = self.registry.attach(uri, native)?;
                Some(DocumentCommandContext {
                    document_session_id: attached.document_session_id,
                    expected_document_version: version,
                })
            }
            (None, None) => None,
            _ => {
                return Err(ApplicationError::InvalidState(
                    "document URI and version must be selected together".into(),
                ));
            }
        };
        let workspace_uri = context.workspace_uri.or_else(|| {
            context
                .document_uri
                .as_deref()
                .and_then(|uri| self.workspace_for_document(uri).map(|(uri, _)| uri))
        });
        Ok(CommandContext {
            workspace_uri,
            document,
        })
    }
    fn workspace_state(
        &mut self,
        request: WorkspaceStateRequest,
    ) -> Result<WorkspaceStateResult, ApplicationError> {
        let context = self.resolve_ui_context(request.context.clone())?;
        let mut workspaces = Vec::new();
        for status in self.registry.workspace_statuses() {
            let options = self
                .dispatch_command(
                    WorkspaceCommandRequest {
                        daemon_instance_id: self.daemon_instance_id().to_owned(),
                        context: CommandContext {
                            workspace_uri: Some(status.uri.clone()),
                            document: None,
                        },
                        operation: WorkspaceCommand::GetNoteOptions {},
                    },
                    true,
                )
                .ok()
                .and_then(|result| match result {
                    fleximark_protocol::CommandResult::NoteOptions { options, .. } => Some(options),
                    _ => None,
                });
            let trusted = status.trusted;
            workspaces.push(WorkspaceUiWorkspace {
                name: status
                    .uri
                    .rsplit('/')
                    .next()
                    .unwrap_or(&status.uri)
                    .to_owned(),
                uri: status.uri,
                trusted,
                note_options: options,
            });
        }
        workspaces.sort_by(|a, b| a.uri.cmp(&b.uri));
        let documents = self
            .registry
            .open_documents()
            .into_iter()
            .map(|(uri, document)| {
                let version = i64::try_from(document.document_version)
                    .ok()
                    .and_then(|version| fleximark_protocol::JsSafeI64::new(version).ok())
                    .ok_or(ApplicationError::IntegerRange(document.document_version))?;
                let workspace_uri = self.workspace_for_document(&uri).map(|(uri, _)| uri);
                Ok(WorkspaceUiDocument {
                    uri,
                    version,
                    workspace_uri,
                })
            })
            .collect::<Result<Vec<_>, ApplicationError>>()?;
        let note_options = context
            .workspace_uri
            .as_ref()
            .and_then(|uri| workspaces.iter().find(|workspace| &workspace.uri == uri))
            .and_then(|workspace| workspace.note_options.clone());
        Ok(WorkspaceStateResult {
            workspaces,
            documents,
            note_options,
            command: request.command,
            context: request.context,
        })
    }

    pub fn browser_notification(&mut self, message: Value) -> Vec<Value> {
        let Some(preview) = message
            .pointer("/params/previewSessionId")
            .and_then(Value::as_str)
        else {
            return vec![];
        };
        let Some(state) = self.preview_states.get(preview) else {
            return vec![];
        };
        if message
            .pointer("/params/renderRevision")
            .and_then(Value::as_u64)
            != Some(state.delivered_revision)
        {
            return vec![];
        };
        let Ok(Some(frame)) = self
            .registry
            .read_preview(&state.document_session_id, preview, None)
        else {
            return vec![];
        };
        let Some(range) = message
            .pointer("/params/event/event/sourceRange")
            .or_else(|| message.pointer("/params/event/sourceRange"))
        else {
            return vec![];
        };
        let version = match self.registry.current_version(&state.document_session_id) {
            Ok(version) => version,
            Err(_) => return vec![],
        };
        if frame.document_version != version {
            return vec![];
        }
        let document = match self.registry.document(&state.document_session_id, version) {
            Ok(document) => document,
            Err(_) => return vec![],
        };
        let uri = document.uri.clone();
        let selection = json!({"start":{"line":range["start"]["line"],"character":range["start"]["character"]},"end":{"line":range["end"]["line"],"character":range["end"]["character"]}});
        self.show_document(&uri, false, Some(selection));
        let mut events = vec![message];
        events.append(&mut self.outgoing_events);
        events
    }

    pub fn client_response(&mut self, response: &Value) {
        if let Some(id) = response.get("id").and_then(Value::as_str) {
            self.pending_client_requests.remove(id);
        }
    }

    pub(crate) fn take_outgoing(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.outgoing_events)
    }
}
