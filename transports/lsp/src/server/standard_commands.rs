//! Role: Implement standard editor code actions and document-aware browser commands.
use super::protocol::counter;
use super::state::*;
use super::wire::*;

pub(super) const COMMANDS: &[&str] = &[
    "fleximark.preview",
    "fleximark.previewAt",
    "fleximark.workspace",
    "fleximark.createNote",
    "fleximark.exportHtml",
    "fleximark.initializeWorkspace",
    "fleximark.editTheme",
    "fleximark.collectAdmonitions",
    "fleximark.migrate",
];

impl Server {
    pub(super) fn code_actions(&self, id: Option<Value>, params: &Value) -> Option<Value> {
        let id = id?;
        let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) else {
            return Some(invalid_params(id, "textDocument.uri is required"));
        };
        let Some(session) = self.registry.session_id_for_uri(uri) else {
            return Some(response_value(Response::success(id, json!([]))));
        };
        let version = match self
            .registry
            .current_version(self.registry.daemon_instance_id(), session)
        {
            Ok(version) => version,
            Err(error) => return Some(session_error(id, error)),
        };
        let range = params.get("range").cloned().unwrap_or_else(
            || json!({"start":{"line":0,"character":0},"end":{"line":0,"character":0}}),
        );
        let actions = [
            ("Preview document", "fleximark.preview"),
            ("Preview at selection", "fleximark.previewAt"),
            ("Open FlexiMark workspace", "fleximark.workspace"),
            ("Create note", "fleximark.createNote"),
            ("Export HTML", "fleximark.exportHtml"),
            ("Initialize workspace", "fleximark.initializeWorkspace"),
            ("Edit theme", "fleximark.editTheme"),
            ("Collect admonitions", "fleximark.collectAdmonitions"),
            ("Migrate legacy workspace", "fleximark.migrate"),
        ]
        .into_iter()
        .map(|(title, command)| {
            json!({
                "title":title,"kind":"source","command":{"title":title,"command":command,
                "arguments":[{"documentUri":uri,"documentVersion":version,"range":range}]}
            })
        })
        .collect::<Vec<_>>();
        Some(response_value(Response::success(id, actions)))
    }

    pub(super) fn standard_execute_command(
        &mut self,
        id: Option<Value>,
        params: Value,
    ) -> Option<Value> {
        let id = id?;
        let Some(command) = params.get("command").and_then(Value::as_str) else {
            return Some(invalid_params(id, "command is required"));
        };
        if !COMMANDS.contains(&command) {
            return Some(invalid_params(id, "unknown command"));
        }
        let context = params
            .pointer("/arguments/0")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if command == "fleximark.migrate" {
            let workspace = context
                .get("workspaceUri")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    context
                        .get("documentUri")
                        .and_then(Value::as_str)
                        .and_then(|uri| self.workspace_for_document(uri).map(|(uri, _)| uri))
                });
            let Some(workspace) = workspace else {
                return Some(invalid_params(
                    id,
                    "migration requires an explicit workspace",
                ));
            };
            if !self.workspace_grants_write(&workspace) {
                return Some(session_error(id, ApplicationError::WriteDenied));
            }
            let result = (|| -> Result<Value, ApplicationError> {
                let output = fleximark_application::native::migrate_workspace(&workspace)?;
                Ok(json!({"message":{"level":"info","text":output.stdout}}))
            })();
            return Some(match result {
                Ok(value) => response_value(Response::success(id, value)),
                Err(error) => session_error(id, error),
            });
        }
        if matches!(command, "fleximark.preview" | "fleximark.previewAt") {
            let Some(uri) = context.get("documentUri").and_then(Value::as_str) else {
                return Some(invalid_params(id, "preview requires documentUri"));
            };
            let Some(session) = self.registry.session_id_for_uri(uri).map(str::to_owned) else {
                return Some(session_error(id, ApplicationError::NotOpen));
            };
            let current = match self
                .registry
                .current_version(self.registry.daemon_instance_id(), &session)
            {
                Ok(version) => version,
                Err(error) => return Some(session_error(id, error)),
            };
            let version = context
                .get("documentVersion")
                .and_then(Value::as_i64)
                .unwrap_or(current);
            let response = self.create_preview(Some(id.clone()), json!({
                "daemonInstanceId":self.registry.daemon_instance_id(),"documentSessionId":session,
                "expectedDocumentVersion":version,"target":"externalBrowser"
            }))?;
            if let Some(url) = response.pointer("/result/url").and_then(Value::as_str) {
                let url = url.to_owned();
                if command == "fleximark.previewAt" {
                    let position = context
                        .pointer("/range/start")
                        .cloned()
                        .unwrap_or(Value::Null);
                    let position: fleximark_application::Position =
                        match serde_json::from_value(position) {
                            Ok(position) => position,
                            Err(error) => return Some(invalid_params(id, error)),
                        };
                    if let Some(preview) = response
                        .pointer("/result/previewSessionId")
                        .and_then(Value::as_str)
                    {
                        if let Some(state) = self.preview_states.get_mut(preview) {
                            state.explicit_jump = Some(position);
                        }
                        if let Err(error) = self.refresh_explicit_jump(preview) {
                            return Some(session_error(id, error));
                        }
                    }
                }
                self.show_document(&url, true, None);
            }
            return Some(response);
        }
        let token = match random_token() {
            Ok(token) => token,
            Err(error) => return Some(invalid_params(id, error)),
        };
        let url = self.previews.publish_workspace(
            &token,
            command.trim_start_matches("fleximark."),
            &context,
        );
        self.show_document(&url, true, None);
        Some(response_value(Response::success(id, json!({"url":url}))))
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
        let daemon = self.registry.daemon_instance_id().to_owned();
        let version = self
            .registry
            .current_version(&daemon, &state.document_session_id)?;
        let Some(entry) = self.registry.navigation_at_position(
            &daemon,
            &state.document_session_id,
            version,
            &position,
        )?
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
