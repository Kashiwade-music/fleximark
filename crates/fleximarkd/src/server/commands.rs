use super::*;
use fleximark_lsp::DocumentSession;

struct CommandDocument<'a> {
    document: &'a DocumentSession,
    session_id: &'a str,
    workspace_uri: &'a str,
}

#[derive(Clone, Copy)]
enum WorkspaceCommand {
    InitializeWorkspace,
    EditTheme,
    CreateNote,
    ExportHtml,
    AcknowledgeExport,
    CollectAdmonitions,
}

impl WorkspaceCommand {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "initializeWorkspace" => Some(Self::InitializeWorkspace),
            "editTheme" => Some(Self::EditTheme),
            "createNote" => Some(Self::CreateNote),
            "exportHtml" => Some(Self::ExportHtml),
            "acknowledgeExport" => Some(Self::AcknowledgeExport),
            "collectAdmonitions" => Some(Self::CollectAdmonitions),
            _ => None,
        }
    }

    fn writes_workspace(self) -> bool {
        matches!(
            self,
            Self::InitializeWorkspace
                | Self::CreateNote
                | Self::ExportHtml
                | Self::AcknowledgeExport
                | Self::CollectAdmonitions
        )
    }
}

impl Server {
    pub(crate) fn execute_command(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let id = id?;
        let params = match deserialize_params::<ExecuteCommandParams>(&id, params) {
            Ok(params) => params,
            Err(response) => return Some(response),
        };
        if params.daemon_instance_id != self.registry.daemon_instance_id() {
            return Some(session_error(id, SessionError::WrongDaemon));
        }
        let command = WorkspaceCommand::parse(&params.command);
        let writes_workspace = command.is_some_and(WorkspaceCommand::writes_workspace);
        if writes_workspace
            && !params
                .workspace_uri
                .as_deref()
                .is_some_and(|uri| self.workspace_grants_write(uri))
        {
            return Some(response_value(Response::error(
                id,
                -32021,
                "workspace write denied: the active workspace is not trusted",
            )));
        }
        let result = match command {
            Some(WorkspaceCommand::InitializeWorkspace) => {
                self.initialize_workspace_command(&params)
            }
            Some(WorkspaceCommand::EditTheme) => self.edit_theme_command(&params),
            Some(WorkspaceCommand::CreateNote) => self.create_note_command(&params),
            Some(WorkspaceCommand::ExportHtml) => self.export_html_command(&params),
            Some(WorkspaceCommand::AcknowledgeExport) => self.acknowledge_export_command(&params),
            Some(WorkspaceCommand::CollectAdmonitions) => self.collect_admonitions_command(&params),
            None => Err(format!("unknown FlexiMark command: {}", params.command)),
        };
        Some(match result {
            Ok(result) => response_value(Response::success(id, result)),
            Err(message) => response_value(Response::error(id, -32020, message)),
        })
    }

    fn export_html_command(
        &mut self,
        params: &ExecuteCommandParams,
    ) -> Result<fleximark_protocol::CommandResult, String> {
        let (session_id, workspace_uri, source_uri) = {
            let context = self.command_document(params, "exportHtml")?;
            (
                context.session_id.to_owned(),
                context.workspace_uri.to_owned(),
                context.document.uri.clone(),
            )
        };
        let destination_uri = params
            .destination_uri
            .clone()
            .map(Ok)
            .unwrap_or_else(|| default_export_destination(&source_uri))
            .map_err(|error| error.to_string())?;
        fleximark_service::preflight_export(&source_uri, &workspace_uri, &destination_uri)
            .map_err(|error| error.to_string())?;
        let export_context = fleximark_service::export_render_context(&workspace_uri)
            .map_err(|error| error.to_string())?;
        let mut assets = None;
        let output = self
            .registry
            .export_html(
                &params.daemon_instance_id,
                &session_id,
                &export_context,
                PREVIEW_CLIENT,
                |safe_html, style, render_assets, runtime| {
                    let resolved = fleximark_service::resolve_export_assets(
                        &source_uri,
                        &workspace_uri,
                        safe_html,
                        render_assets,
                    )
                    .map_err(|error| SessionError::Engine(error.to_string()))?;
                    assets = Some(resolved.assets);
                    fleximark_service::compose_portable_html(&resolved.html, style, runtime)
                        .map_err(|error| SessionError::Engine(error.to_string()))
                },
            )
            .map_err(|error| error.to_string())?;
        export_html_with_safety(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            &output.html,
            &assets.unwrap_or_default(),
            output.unsafe_output_used,
        )
        .map_err(|error| error.to_string())
    }

    fn acknowledge_export_command(
        &self,
        params: &ExecuteCommandParams,
    ) -> Result<fleximark_protocol::CommandResult, String> {
        let context = self.command_document(params, "acknowledgeExport")?;
        let source_uri = &context.document.uri;
        let destination_uri = params
            .destination_uri
            .clone()
            .map(Ok)
            .unwrap_or_else(|| default_export_destination(source_uri))
            .map_err(|error| error.to_string())?;
        acknowledge_export(source_uri, context.workspace_uri, &destination_uri)
            .map_err(|error| error.to_string())?;
        Ok(fleximark_protocol::CommandResult {
            message: Some(fleximark_protocol::CommandMessage {
                level: fleximark_protocol::CommandMessageLevel::Info,
                text: "Export opened and validated; recovery backup released".into(),
            }),
            open_uri: None,
        })
    }

    fn collect_admonitions_command(
        &self,
        params: &ExecuteCommandParams,
    ) -> Result<fleximark_protocol::CommandResult, String> {
        let context = self.command_document(params, "collectAdmonitions")?;
        collect_admonitions(
            context.document.document(),
            context.document.source(),
            context.workspace_uri,
        )
        .map_err(|error| error.to_string())
    }

    fn command_document<'a>(
        &'a self,
        params: &'a ExecuteCommandParams,
        command: &str,
    ) -> Result<CommandDocument<'a>, String> {
        let (Some(session_id), Some(version), Some(workspace_uri)) = (
            params.document_session_id.as_deref(),
            params.expected_document_version,
            params.workspace_uri.as_deref(),
        ) else {
            return Err(format!(
                "{command} requires an active document and workspaceUri"
            ));
        };
        let document = self
            .registry
            .document(&params.daemon_instance_id, session_id, version.get())
            .map_err(|error| error.to_string())?;
        if document.workspace_uri() != Some(workspace_uri) {
            return Err("active document belongs to a different workspace authority".into());
        }
        Ok(CommandDocument {
            document,
            session_id,
            workspace_uri,
        })
    }

    fn command_workspace<'a>(&self, params: &'a ExecuteCommandParams) -> Result<&'a str, String> {
        params
            .workspace_uri
            .as_deref()
            .ok_or_else(|| "command requires workspaceUri".to_owned())
    }

    fn initialize_workspace_command(
        &self,
        params: &ExecuteCommandParams,
    ) -> Result<fleximark_protocol::CommandResult, String> {
        self.command_workspace(params)
            .and_then(|uri| initialize_workspace(uri).map_err(|error| error.to_string()))
    }

    fn edit_theme_command(
        &self,
        params: &ExecuteCommandParams,
    ) -> Result<fleximark_protocol::CommandResult, String> {
        self.command_workspace(params)
            .and_then(|uri| edit_theme(uri).map_err(|error| error.to_string()))
    }

    fn create_note_command(
        &self,
        params: &ExecuteCommandParams,
    ) -> Result<fleximark_protocol::CommandResult, String> {
        self.command_workspace(params).and_then(|uri| {
            create_note_with_options(
                uri,
                params.note_category.as_deref(),
                params.note_template.as_deref(),
            )
            .map_err(|error| error.to_string())
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
        Some(match get_note_options(&params.workspace_uri) {
            Ok(options) => response_value(Response::success(id, options)),
            Err(error) => response_value(Response::error(id, -32020, error.to_string())),
        })
    }
}
