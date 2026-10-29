//! Role: Implement standard editor code actions and document-aware browser commands.
use super::protocol::counter;
use super::state::*;
use super::wire::*;

use fleximark_protocol::{
    CommandContext, CommandKind, DocumentCommandContext, WorkspaceCommand, WorkspaceCommandRequest,
};
fn default_operation(kind: CommandKind) -> WorkspaceCommand {
    match kind {
        CommandKind::InitializeWorkspace => WorkspaceCommand::InitializeWorkspace {},
        CommandKind::EditTheme => WorkspaceCommand::EditTheme {},
        CommandKind::CreateNote => WorkspaceCommand::CreateNote {
            category_path: None,
            template: None,
            name: None,
        },
        CommandKind::GetNoteOptions => WorkspaceCommand::GetNoteOptions {},
        CommandKind::CollectAdmonitions => WorkspaceCommand::CollectAdmonitions {},
        CommandKind::ExportHtml => WorkspaceCommand::ExportHtml {
            destination_uri: None,
        },
        CommandKind::AcknowledgeExport => WorkspaceCommand::AcknowledgeExport {
            destination_uri: None,
        },
        CommandKind::PlanMigration => WorkspaceCommand::PlanMigration {
            settings_uri: None,
            recover: false,
        },
        CommandKind::ApplyMigration => WorkspaceCommand::ApplyMigration {
            plan_id: String::new(),
        },
        CommandKind::OpenPreview => WorkspaceCommand::OpenPreview {
            target: PreviewTarget::ExternalBrowser,
            source_offset: None,
        },
        CommandKind::OpenWorkspace => WorkspaceCommand::OpenWorkspace {},
    }
}
#[derive(serde::Serialize)]
struct CodeAction {
    title: String,
    kind: &'static str,
    command: EditorCommand,
}
#[derive(serde::Serialize)]
struct EditorCommand {
    title: String,
    command: &'static str,
    arguments: Vec<WorkspaceCommandRequest>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecuteCommand {
    command: String,
    arguments: Vec<WorkspaceCommandRequest>,
}
impl Server {
    pub(super) fn code_actions(&self, id: Option<Value>, params: &Value) -> Option<Value> {
        let id = id?;
        let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) else {
            return Some(invalid_params(id, "textDocument.uri is required"));
        };
        let Some(session) = self.registry.session_id_for_uri(uri) else {
            return Some(response_value(Response::success(
                id,
                Vec::<CodeAction>::new(),
            )));
        };
        let result = (|| {
            let version = self.registry.current_version(session)?;
            let version = i64::try_from(version)
                .ok()
                .and_then(|version| fleximark_protocol::JsSafeI64::new(version).ok())
                .ok_or(ApplicationError::IntegerRange(version))?;
            let context = CommandContext {
                workspace_uri: self.workspace_for_document(uri).map(|(uri, _)| uri),
                document: Some(DocumentCommandContext {
                    document_session_id: session.to_owned(),
                    expected_document_version: version,
                }),
            };
            let mut actions = Vec::new();
            for descriptor in fleximark_protocol::COMMAND_DESCRIPTORS
                .iter()
                .filter(|descriptor| descriptor.code_action)
            {
                let operation = default_operation(descriptor.kind);
                let request = WorkspaceCommandRequest {
                    daemon_instance_id: self.daemon_instance_id().to_owned(),
                    context: context.clone(),
                    operation,
                };
                actions.push(CodeAction {
                    title: descriptor.title.to_owned(),
                    kind: "source",
                    command: EditorCommand {
                        title: descriptor.title.to_owned(),
                        command: descriptor.lsp_command,
                        arguments: vec![request.clone()],
                    },
                });
                if descriptor.kind == CommandKind::OpenPreview {
                    let position = params
                        .pointer("/range/start")
                        .cloned()
                        .unwrap_or_else(|| json!({"line":0,"character":0}));
                    let position: super::lsp_types::Position = serde_json::from_value(position)?;
                    let offset = self.position_offset(uri, &position)?;
                    let mut request = request;
                    request.operation = WorkspaceCommand::OpenPreview {
                        target: PreviewTarget::ExternalBrowser,
                        source_offset: Some(counter(offset as u64)?),
                    };
                    actions.push(CodeAction {
                        title: "Preview at selection".into(),
                        kind: "source",
                        command: EditorCommand {
                            title: "Preview at selection".into(),
                            command: descriptor.lsp_command,
                            arguments: vec![request],
                        },
                    });
                }
            }
            Ok::<_, ApplicationError>(actions)
        })();
        Some(match result {
            Ok(actions) => response_value(Response::success(id, actions)),
            Err(error) => session_error(id, error),
        })
    }
    pub(super) fn standard_execute_command(
        &mut self,
        id: Option<Value>,
        params: Value,
    ) -> Option<Value> {
        let id = id?;
        let params = match deserialize_params::<ExecuteCommand>(&id, params) {
            Ok(params) => params,
            Err(error) => return Some(error),
        };
        let Some(descriptor) = fleximark_protocol::COMMAND_DESCRIPTORS
            .iter()
            .find(|descriptor| descriptor.lsp_command == params.command)
        else {
            return Some(invalid_params(id, "unknown command"));
        };
        if params.arguments.len() != 1 {
            return Some(invalid_params(id, "command requires one typed request"));
        }
        let Some(request) = params.arguments.into_iter().next() else {
            return Some(invalid_params(id, "command requires one typed request"));
        };
        if request.operation.kind() != descriptor.kind {
            return Some(invalid_params(id, "command and operation do not match"));
        }
        let result = self.dispatch_command(request, false).and_then(|result| {
            if let fleximark_protocol::CommandResult::MigrationPlanned { plan, .. } = &result {
                let url = self.workspace_ui_url(fleximark_protocol::WorkspaceStateRequest {
                    context: fleximark_protocol::WorkspaceUiContext {
                        workspace_uri: Some(plan.workspace_uri.clone()),
                        document_uri: None,
                        document_version: None,
                    },
                    command: Some(CommandKind::PlanMigration),
                })?;
                self.show_document(&url, true, None);
            }
            Ok(result)
        });
        Some(match result {
            Ok(result) => response_value(Response::success(id, result)),
            Err(error) => session_error(id, error),
        })
    }

    pub(super) fn refresh_explicit_jump(
        &mut self,
        preview_id: &str,
    ) -> Result<(), ApplicationError> {
        let Some(state) = self.preview_states.get(preview_id).cloned() else {
            return Ok(());
        };
        let Some(position) = state.explicit_jump else {
            return Ok(());
        };
        let version = self.registry.current_version(&state.document_session_id)?;
        let Some(entry) =
            self.navigation_at_position(&state.document_session_id, version, &position)?
        else {
            return Ok(());
        };
        let event = RenderNavigationEvent::Viewport {
            preview_session_id: preview_id.into(),
            render_revision: counter(state.delivered_revision)?,
            node_id: fleximark_protocol::NodeId(entry.node_id.0),
        };
        if let Some(token) = &state.token {
            self.previews.retain_jump(token, &event);
        }
        Ok(())
    }

    pub(crate) fn show_document(&mut self, uri: &str, external: bool, selection: Option<Value>) {
        let request_id = format!("fleximark-show-{}", self.next_client_request);
        self.next_client_request += 1;
        self.pending_client_requests.insert(request_id.clone());
        let mut params = json!({"uri":uri,"external":external,"takeFocus":true});
        if let Some(selection) = selection {
            params["selection"] = selection;
        }
        self.outgoing_events.push(
            json!({"jsonrpc":"2.0","id":request_id,"method":"window/showDocument","params":params}),
        );
    }
}
