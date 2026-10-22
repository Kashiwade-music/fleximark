//! Role: Adapt browser forms to document-aware workspace operations.
use super::commands;
use super::state::*;

fn without_nulls(mut value: Value) -> Value {
    if let Some(fields) = value.as_object_mut() {
        fields.retain(|_, value| !value.is_null());
    }
    value
}

fn decode_query(value: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut input = value.bytes();
    while let Some(byte) = input.next() {
        bytes.push(match byte {
            b'%' => {
                let a = (input.next()? as char).to_digit(16)?;
                let b = (input.next()? as char).to_digit(16)?;
                (a * 16 + b) as u8
            }
            b'+' => b' ',
            byte => byte,
        });
    }
    String::from_utf8(bytes).ok()
}

impl Server {
    pub fn workspace_request(&mut self, request: Value) -> Result<Value, String> {
        if !self.lsp_initialized {
            return Err("server is not initialized".into());
        }
        match request.get("kind").and_then(Value::as_str) {
            Some("state") => self.workspace_state(&request),
            Some("command") => {
                self.workspace_command(request.get("params").cloned().unwrap_or(Value::Null))
            }
            _ => Err("unknown workspace request".into()),
        }
    }

    fn workspace_state(&self, request: &Value) -> Result<Value, String> {
        let context = request.get("context").cloned().unwrap_or_else(|| json!({}));
        let query_workspace = request
            .get("query")
            .and_then(Value::as_str)
            .and_then(|query| {
                query.split('&').find_map(|pair| {
                    let (key, value) = pair.split_once('=')?;
                    if key == "workspaceUri" {
                        decode_query(value)
                    } else {
                        None
                    }
                })
            });
        let selected = query_workspace.or_else(|| {
            context
                .get("workspaceUri")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    context
                        .get("documentUri")
                        .and_then(Value::as_str)
                        .and_then(|uri| self.workspace_for_document(uri).map(|(uri, _)| uri))
                })
        });
        let mut workspaces = self.workspaces.iter().map(|(uri, trusted)| {
            let options = if *trusted { self.services.get_note_options(uri).ok().map(commands::note_options_value) } else { None };
            json!({"uri":uri,"name":uri.rsplit('/').next().unwrap_or(uri),"trusted":trusted,"noteOptions":options})
        }).collect::<Vec<_>>();
        workspaces.sort_by(|a, b| a["uri"].as_str().cmp(&b["uri"].as_str()));
        let documents = self
            .registry
            .open_documents()
            .into_iter()
            .map(|(uri, document)| {
                let workspace = self.workspace_for_document(&uri).map(|(uri, _)| uri);
                without_nulls(
                    json!({"uri":uri,"version":document.document_version,"workspaceUri":workspace}),
                )
            })
            .collect::<Vec<_>>();
        let options = selected
            .as_deref()
            .filter(|uri| self.workspace_grants_write(uri))
            .and_then(|uri| self.services.get_note_options(uri).ok())
            .map(commands::note_options_value);
        Ok(without_nulls(
            json!({"workspaces":workspaces,"documents":documents,"noteOptions":options,"command":request.get("command"),"context":context}),
        ))
    }

    fn workspace_command(&mut self, request: Value) -> Result<Value, String> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Command {
            command: String,
            document_uri: Option<String>,
            document_version: Option<i64>,
            workspace_uri: Option<String>,
            note_category_path: Option<Vec<String>>,
            note_template: Option<String>,
            note_file_name: Option<String>,
            destination_uri: Option<String>,
        }
        let request: Command =
            serde_json::from_value(request).map_err(|error| error.to_string())?;
        let command = request.command.trim_start_matches("fleximark.");
        if !matches!(
            command,
            "initializeWorkspace"
                | "editTheme"
                | "createNote"
                | "exportHtml"
                | "collectAdmonitions"
        ) {
            return Err("unknown workspace command".into());
        }
        let session = request
            .document_uri
            .as_deref()
            .map(|uri| {
                self.registry
                    .session_id_for_uri(uri)
                    .map(str::to_owned)
                    .ok_or_else(|| "selected document is no longer open".to_owned())
            })
            .transpose()?;
        let workspace = request.workspace_uri.or_else(|| {
            request
                .document_uri
                .as_deref()
                .and_then(|uri| self.workspace_for_document(uri).map(|(uri, _)| uri))
        });
        if matches!(command, "exportHtml" | "collectAdmonitions")
            && (session.is_none() || request.document_version.is_none())
        {
            return Err("select an open document and its current version".into());
        }
        let expected_document_version = request
            .document_version
            .map(TryInto::try_into)
            .transpose()
            .map_err(|_| "document version exceeds the transport integer range")?;
        let params = ExecuteCommandParams {
            daemon_instance_id: self.registry.daemon_instance_id().into(),
            command: command.into(),
            workspace_uri: workspace,
            document_session_id: session,
            expected_document_version,
            note_category_path: request.note_category_path,
            note_template: request.note_template,
            note_file_name: request.note_file_name,
            destination_uri: request.destination_uri,
            arguments: Vec::new(),
        };
        let result = self
            .execute_operation(&params)
            .map_err(|(_, message)| message)?;
        if let Some(uri) = &result.open_uri {
            self.show_document(uri, false, None);
        }
        Ok(without_nulls(
            json!({"message":result.message.map(|message|message.text),"openUri":result.open_uri,"data":result.data}),
        ))
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
        let Ok(Some(frame)) = self.registry.read_preview(
            self.registry.daemon_instance_id(),
            &state.document_session_id,
            preview,
            None,
        ) else {
            return vec![];
        };
        let Some(range) = message
            .pointer("/params/event/event/sourceRange")
            .or_else(|| message.pointer("/params/event/sourceRange"))
        else {
            return vec![];
        };
        let version = match self.registry.current_version(
            self.registry.daemon_instance_id(),
            &state.document_session_id,
        ) {
            Ok(version) => version,
            Err(_) => return vec![],
        };
        if frame.document_version != version as u64 {
            return vec![];
        }
        let document = match self.registry.document(
            self.registry.daemon_instance_id(),
            &state.document_session_id,
            version,
        ) {
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
