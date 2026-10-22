//! Role: Deliver LSP and RPC requests and session events over the daemon transport.
use super::diagnostics::{collect_heading_symbols, find_block};
use super::protocol::native_position;
use super::state::*;
use super::wire::*;

impl Server {
    pub(super) fn lsp_initialize(&mut self, id: Option<Value>, params: &Value) -> Option<Value> {
        let id = id?;
        if self.lsp_initialized {
            return Some(invalid_params(id, "server is already initialized"));
        }
        if let Some(version) = params.pointer("/initializationOptions/fleximark/protocolVersion") {
            if version.as_u64() != Some(u64::from(PROTOCOL_VERSION)) {
                return Some(response_value(Response::error(
                    id,
                    -32001,
                    format!("unsupported protocol version {version}; expected {PROTOCOL_VERSION}"),
                )));
            }
        }
        self.snippet_support = params
            .pointer("/capabilities/textDocument/completion/completionItem/snippetSupport")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let encoding = params
            .pointer("/capabilities/general/positionEncodings")
            .and_then(Value::as_array)
            .and_then(|encodings| {
                encodings
                    .iter()
                    .filter_map(Value::as_str)
                    .find_map(|value| match value {
                        "utf-8" => Some(PositionEncoding::Utf8),
                        "utf-16" => Some(PositionEncoding::Utf16),
                        "utf-32" => Some(PositionEncoding::Utf32),
                        _ => None,
                    })
            })
            .unwrap_or(PositionEncoding::Utf16);
        self.registry.set_position_encoding(encoding);
        self.lsp_initialized = true;
        let options = params
            .pointer("/initializationOptions/fleximark")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let trusted = options
            .get("trusted")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let roots = options.get("workspaces").cloned().unwrap_or_else(|| {
            let mut roots = params
                .get("workspaceFolders")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|folder| {
                    folder
                        .get("uri")
                        .cloned()
                        .map(|uri| json!({"uri":uri,"trusted":trusted}))
                })
                .collect::<Vec<_>>();
            if roots.is_empty() {
                if let Some(uri) = params.get("rootUri").filter(|uri| uri.is_string()) {
                    roots.push(json!({"uri":uri,"trusted":trusted}));
                }
            }
            Value::Array(roots)
        });
        let configured = self.configure_session(Some(id.clone()), json!({"protocolVersion":PROTOCOL_VERSION,"client":{"name":params.pointer("/clientInfo/name").and_then(Value::as_str).unwrap_or("lsp-client"),"version":params.pointer("/clientInfo/version").and_then(Value::as_str).unwrap_or("")},"workspaces":roots,"capabilities":options.get("capabilities").cloned().unwrap_or_else(|| json!({"selectionEvents":false,"viewportEvents":false}))})).expect("initialization response");
        if configured.get("error").is_some() {
            self.lsp_initialized = false;
            return Some(configured);
        }
        let experimental = configured.get("result").cloned().unwrap_or(Value::Null);
        let position_encoding = match encoding {
            PositionEncoding::Utf8 => "utf-8",
            PositionEncoding::Utf16 => "utf-16",
            PositionEncoding::Utf32 => "utf-32",
        };
        Some(response_value(Response::success(
            id,
            json!({
                "capabilities": {
                    "positionEncoding": position_encoding,
                    "textDocumentSync": { "openClose": true, "change": 1 },
                    "completionProvider": {"triggerCharacters":[":","`",">"]},
                    "hoverProvider": true,
                    "documentSymbolProvider": true,
                    "codeActionProvider": true,
                    "executeCommandProvider":{"commands":super::standard_commands::COMMANDS},
                    "experimental":{"fleximark":experimental},
                    "semanticTokensProvider": {
                        "legend": {
                            "tokenTypes": fleximark_application::SEMANTIC_TOKEN_TYPES,
                            "tokenModifiers": []
                        },
                        "full": true
                    },
                    "diagnosticProvider": {"interFileDependencies":false,"workspaceDiagnostics":false}
                },
                "serverInfo": { "name": "fleximarkd", "version": env!("CARGO_PKG_VERSION") }
            }),
        )))
    }

    pub(super) fn completion(&self, id: Option<Value>, params: &Value) -> Option<Value> {
        let id = id?;
        let uri = params.pointer("/textDocument/uri").and_then(Value::as_str);
        let line = params.pointer("/position/line").and_then(Value::as_u64);
        let character = params
            .pointer("/position/character")
            .and_then(Value::as_u64);
        let (Some(uri), Some(line), Some(character)) = (uri, line, character) else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "completion requires textDocument.uri and position",
            )));
        };
        let (Ok(line32), Ok(character32)) = (u32::try_from(line), u32::try_from(character)) else {
            return Some(invalid_params(
                id,
                "position exceeds the document index range",
            ));
        };
        let Ok(prefix) = self.registry.line_prefix(
            uri,
            fleximark_application::Position {
                line: line32,
                character: character32,
            },
        ) else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "completion position is not in an open document",
            )));
        };
        let items = fleximark_language::completion_items(
            prefix,
            fleximark_language::Position { line, character },
            self.registry.position_encoding(),
            self.snippet_support,
        )
        .into_iter()
        .map(super::language::completion)
        .collect::<Vec<_>>();
        Some(response_value(Response::success(
            id,
            json!({"isIncomplete":false,"items":items}),
        )))
    }

    pub(super) fn semantic_tokens(&self, id: Option<Value>, params: &Value) -> Option<Value> {
        let id = id?;
        let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "semantic tokens require textDocument.uri",
            )));
        };
        let Ok(source) = self.registry.source_for_uri(uri) else {
            return Some(response_value(Response::success(id, Value::Null)));
        };
        Some(response_value(Response::success(
            id,
            json!({
                "data": super::language::semantic_data(
                    source,
                    self.registry.position_encoding(),
                )
            }),
        )))
    }

    pub(super) fn hover(&self, id: Option<Value>, params: &Value) -> Option<Value> {
        let id = id?;
        let Some((uri, line, character)) = params
            .pointer("/textDocument/uri")
            .and_then(Value::as_str)
            .zip(params.pointer("/position/line").and_then(Value::as_u64))
            .zip(
                params
                    .pointer("/position/character")
                    .and_then(Value::as_u64),
            )
            .map(|((uri, line), character)| (uri, line, character))
        else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "invalid hover parameters",
            )));
        };
        let (Ok(line), Ok(character)) = (JsSafeU64::new(line), JsSafeU64::new(character)) else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "hover position is outside the JavaScript safe integer range",
            )));
        };
        let Some(session_id) = self.registry.session_id_for_uri(uri) else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "document is not open",
            )));
        };
        let Ok(version) = self
            .registry
            .current_version(self.registry.daemon_instance_id(), session_id)
        else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "document is not current",
            )));
        };
        let position = match native_position(&fleximark_protocol::TextPosition { line, character })
        {
            Ok(position) => position,
            Err(error) => return Some(session_error(id, error)),
        };
        let Ok(Some(entry)) = self.registry.navigation_at_position(
            self.registry.daemon_instance_id(),
            session_id,
            version,
            &position,
        ) else {
            return Some(response_value(Response::success(id, Value::Null)));
        };
        let Ok(document) =
            self.registry
                .document(self.registry.daemon_instance_id(), session_id, version)
        else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "document is unavailable",
            )));
        };
        let Some(kind) = find_block(document.document(), &entry.node_id)
            .map(|block| format!("{:?}", block.kind))
        else {
            return Some(response_value(Response::success(id, Value::Null)));
        };
        Some(response_value(Response::success(
            id,
            json!({"contents":{"kind":"markdown","value":format!("**FlexiMark block**: `{kind}`\n\nUTF-8 source bytes {}..{}", entry.source_range.byte_start, entry.source_range.byte_end)}}),
        )))
    }

    pub(super) fn document_symbols(&self, id: Option<Value>, params: &Value) -> Option<Value> {
        let id = id?;
        let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "invalid symbol parameters",
            )));
        };
        let Some(session_id) = self.registry.session_id_for_uri(uri) else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "document is not open",
            )));
        };
        let Ok(version) = self
            .registry
            .current_version(self.registry.daemon_instance_id(), session_id)
        else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "document is not current",
            )));
        };
        let Ok(document) =
            self.registry
                .document(self.registry.daemon_instance_id(), session_id, version)
        else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "document is unavailable",
            )));
        };
        let mut symbols = Vec::new();
        collect_heading_symbols(
            &document.document().blocks,
            &mut symbols,
            &fleximark_model::SourceIndex::new(document.source()),
            document.position_encoding(),
        );
        Some(response_value(Response::success(id, symbols)))
    }

    pub(super) fn close_document(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        if id.is_some() {
            return id.map(|id| {
                response_value(Response::error(
                    id,
                    -32600,
                    "LSP notification must not have an id",
                ))
            });
        }
        let params = match serde_json::from_value::<DidCloseParams>(params) {
            Ok(params) => params,
            Err(error) => {
                let _ = error;
                log_operational_event("did-close-invalid", None, None, None);
                return None;
            }
        };
        let uri = params.text_document.uri.clone();
        if let Some(session_id) = self.registry.session_id_for_uri(&uri) {
            let expired = self
                .preview_states
                .iter()
                .filter(|(_, state)| state.document_session_id == session_id)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            for preview_id in expired {
                if let Some(state) = self.preview_states.remove(&preview_id) {
                    let daemon_instance_id = self.registry.daemon_instance_id().to_owned();
                    let _ = self.registry.dispose_preview(
                        &daemon_instance_id,
                        &state.document_session_id,
                        &preview_id,
                    );
                    if let Some(token) = &state.token {
                        self.previews.remove(token);
                    }
                }
            }
        }
        self.registry.close(params);
        {
            self.outgoing_events.push(json!({
                "jsonrpc":"2.0","method":"textDocument/publishDiagnostics",
                "params":{"uri":uri,"diagnostics":[]}
            }));
        }
        None
    }

    pub(super) fn open_lsp_document(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        if id.is_some() {
            return id.map(|id| {
                response_value(Response::error(
                    id,
                    -32600,
                    "LSP notification must not have an id",
                ))
            });
        }
        let params = match serde_json::from_value::<DidOpenParams>(params) {
            Ok(params) => params,
            Err(error) => {
                let _ = error;
                log_operational_event("did-open-invalid", None, None, None);
                return None;
            }
        };
        if fleximark_protocol::JsSafeI64::new(params.text_document.version).is_err() {
            return None;
        }
        let uri = params.text_document.uri.clone();
        if let Err(error) = self
            .registry
            .open_with_cancellation(params, &self.work_cancellation)
            .and_then(|_| self.refresh_assets(&uri))
        {
            let _ = error;
            log_operational_event("document-open-failed", Some(&uri), None, None);
        } else {
            self.publish_lsp_diagnostics(&uri);
        }
        None
    }

    pub(super) fn change_document(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        if id.is_some() {
            return id.map(|id| {
                response_value(Response::error(
                    id,
                    -32600,
                    "LSP notification must not have an id",
                ))
            });
        }
        let params = match serde_json::from_value::<DidChangeParams>(params) {
            Ok(params) => params,
            Err(error) => {
                let _ = error;
                log_operational_event("did-change-invalid", None, None, None);
                return None;
            }
        };
        let uri = params.text_document.uri.clone();
        let document_version = params.text_document.version;
        if fleximark_protocol::JsSafeI64::new(document_version).is_err() {
            return None;
        }
        let trace = OperationalTrace::new(Some(&uri), None, Some(document_version));
        trace.log("change-received");
        let transform_started = Instant::now();
        if let Err(error) = self.apply_document_change(params) {
            let _ = error;
            eprintln!(
                "{}",
                trace.event(
                    "document-sync-failed",
                    transform_started.elapsed(),
                    None,
                    true,
                    false,
                )
            );
            return None;
        }
        if let Some(session_id) = self.registry.session_id_for_uri(&uri).map(str::to_owned) {
            if let Ok(version) = self
                .registry
                .current_version(self.registry.daemon_instance_id(), &session_id)
                .and_then(|version| {
                    u64::try_from(version)
                        .map_err(|_| SessionError::VersionMismatch)
                        .and_then(|version| {
                            JsSafeU64::new(version).map_err(|_| SessionError::VersionMismatch)
                        })
                })
            {
                self.mark_preview_document_version(&session_id, version);
            }
        }
        if self.publication_cancellation.is_cancelled() {
            return None;
        }
        eprintln!(
            "{}",
            trace.event(
                "transform-complete",
                transform_started.elapsed(),
                None,
                false,
                false,
            )
        );
        if !self.refresh_changed_document_assets(&uri, &trace) {
            return None;
        }
        self.publish_changed_document(&uri);
        None
    }

    fn apply_document_change(&mut self, params: DidChangeParams) -> Result<(), SessionError> {
        self.registry
            .change_with_cancellation(params, &self.work_cancellation)
    }

    fn refresh_changed_document_assets(&mut self, uri: &str, trace: &OperationalTrace) -> bool {
        let asset_started = Instant::now();
        if let Err(error) = self.refresh_assets(uri) {
            let _ = error;
            eprintln!(
                "{}",
                trace.event(
                    "asset-refresh-failed",
                    asset_started.elapsed(),
                    None,
                    false,
                    false,
                )
            );
            return false;
        }
        true
    }

    fn publish_changed_document(&mut self, uri: &str) {
        self.publish_lsp_diagnostics(uri);
        let Some(session_id) = self.registry.session_id_for_uri(uri).map(str::to_owned) else {
            return;
        };
        let daemon_id = self.registry.daemon_instance_id().to_owned();
        let version = self
            .registry
            .current_version(&daemon_id, &session_id)
            .expect("changed document remains current");
        let preview_ids = self
            .preview_states
            .iter()
            .filter(|(_, state)| state.document_session_id == session_id)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for preview_id in preview_ids {
            let frame = match self.registry.render_with_cancellation(
                &daemon_id,
                &session_id,
                version,
                &preview_id,
                &self.work_cancellation,
            ) {
                Ok(frame) => frame,
                Err(error) => {
                    let _ = error;
                    log_operational_event(
                        "preview-update-failed",
                        Some(uri),
                        Some(&session_id),
                        Some(version),
                    );
                    continue;
                }
            };
            if let Err(error) = self.publish_frame(&preview_id, &frame) {
                self.outgoing_events.push(json!({"jsonrpc":"2.0","method":"window/logMessage","params":{"type":1,"message":error.to_string()}}));
                return;
            }
        }
    }
}
