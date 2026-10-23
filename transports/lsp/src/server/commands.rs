//! Role: Deliver LSP and RPC requests and session events over the daemon transport.
use super::state::*;
use super::wire::*;

fn command_result(
    message: &str,
    open_uri: Option<String>,
    data: Option<String>,
) -> fleximark_protocol::CommandResult {
    fleximark_protocol::CommandResult {
        message: Some(fleximark_protocol::CommandMessage {
            level: fleximark_protocol::CommandMessageLevel::Info,
            text: message.into(),
        }),
        open_uri,
        data,
    }
}

pub(super) fn note_options_value(options: fleximark_application::operations::NoteOptions) -> Value {
    fn category(value: fleximark_application::operations::NoteCategory) -> Value {
        json!({"name":value.name,"children":value.children.into_iter().map(category).collect::<Vec<_>>()})
    }
    json!({"categories":options.categories.into_iter().map(category).collect::<Vec<_>>(),"templates":options.templates})
}

impl Server {
    pub(crate) fn execute_command(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let id = id?;
        let params = match deserialize_params::<ExecuteCommandParams>(&id, params) {
            Ok(params) => params,
            Err(response) => return Some(response),
        };
        Some(match self.execute_operation(&params) {
            Ok(result) => response_value(Response::success(id, result)),
            Err((code, message)) => response_value(Response::error(id, code, message)),
        })
    }
    pub(super) fn execute_operation(
        &mut self,
        params: &ExecuteCommandParams,
    ) -> Result<fleximark_protocol::CommandResult, (i64, String)> {
        use fleximark_application::operations::{
            OperationOutcome as Out, OperationRequest, ServiceOperationError as Error,
            WorkspaceOperation as Op,
        };
        let operation = match params.command.as_str() {
            "initializeWorkspace" => Op::Initialize,
            "editTheme" => Op::EditTheme,
            "createNote" => Op::CreateNote,
            "exportHtml" => Op::Export,
            "acknowledgeExport" => Op::AcknowledgeExport,
            "collectAdmonitions" => Op::CollectAdmonitions,
            _ => {
                return Err((
                    -32020,
                    format!("unknown FlexiMark command: {}", params.command),
                ));
            }
        };
        let outcome = self
            .registry
            .execute_operation(
                operation,
                OperationRequest {
                    daemon: &params.daemon_instance_id,
                    workspace: params.workspace_uri.as_deref(),
                    document_session: params.document_session_id.as_deref(),
                    document_version: params
                        .expected_document_version
                        .map(|version| version.get()),
                    destination: params.destination_uri.as_deref(),
                    category: params.note_category_path.as_deref(),
                    template: params.note_template.as_deref(),
                    name: params.note_file_name.as_deref(),
                },
                &self.work_cancellation,
            )
            .map_err(|error| {
                let code = match error {
                    Error::WrongDaemon => -32602,
                    Error::WriteDenied => -32021,
                    Error::Failed(_) => -32020,
                };
                (code, error.to_string())
            })?;
        Ok(match outcome {
            Out::Initialized(outcome) => {
                command_result("Workspace initialized", Some(outcome.config_uri), None)
            }
            Out::Theme(outcome) => command_result("Theme opened", Some(outcome.uri), None),
            Out::Note(outcome) => command_result("Note created", Some(outcome.uri), None),
            Out::Exported(outcome) => command_result(
                "HTML exported",
                Some(outcome.output_uri),
                Some(outcome.generation.to_string()),
            ),
            Out::ExportAcknowledged => command_result(
                "Export opened and validated; recovery backup released",
                None,
                None,
            ),
            Out::Admonitions(outcome) => command_result(
                &format!("Collected {} admonitions", outcome.count),
                Some(outcome.uri),
                None,
            ),
        })
    }

    pub(super) fn get_note_options(&self, id: Option<Value>, params: Value) -> Option<Value> {
        let id = id?;
        let params = match deserialize_params::<GetNoteOptionsParams>(&id, params) {
            Ok(params) => params,
            Err(response) => return Some(response),
        };
        if params.daemon_instance_id != self.registry.daemon_instance_id() {
            return Some(session_error(id, SessionError::WrongDaemon));
        }
        if !self.workspace_grants_write(&params.workspace_uri) {
            return Some(response_value(Response::error(
                id,
                -32021,
                "note options denied: the workspace is not trusted",
            )));
        }
        Some(
            match fleximark_application::native::get_note_options(&params.workspace_uri) {
                Ok(options) => response_value(Response::success(id, note_options_value(options))),
                Err(error) => response_value(Response::error(id, -32020, error.to_string())),
            },
        )
    }
}
