//! Role: Negotiate LSP capabilities and serve document language queries.
use super::diagnostics::{collect_heading_symbols, find_block};
use super::lsp_types::*;
use super::positions::native_position;
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
                    fleximark_protocol::error_code::UNSUPPORTED_PROTOCOL,
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
        let roots: Result<Vec<(String, bool)>, ApplicationError> = (|| {
            if let Some(roots) = options.get("workspaces") {
                let roots: Vec<fleximark_protocol::WorkspaceGrant> =
                    serde_json::from_value(roots.clone())?;
                return Ok(roots
                    .into_iter()
                    .map(|root| (root.uri, root.trusted))
                    .collect());
            }
            let mut roots = Vec::new();
            if let Some(folders) = params.get("workspaceFolders").and_then(Value::as_array) {
                for folder in folders {
                    if let Some(uri) = folder.get("uri").and_then(Value::as_str) {
                        roots.push((uri.to_owned(), trusted));
                    }
                }
            }
            if roots.is_empty() {
                if let Some(uri) = params.get("rootUri").and_then(Value::as_str) {
                    roots.push((uri.to_owned(), trusted));
                }
            }
            Ok(roots)
        })();
        let experimental = match roots.and_then(|roots| self.initialize_workspaces(roots, &options))
        {
            Ok(result) => result,
            Err(error) => {
                self.lsp_initialized = false;
                return Some(session_error(id, error));
            }
        };
        let position_encoding = match encoding {
            PositionEncoding::Utf8 => "utf-8",
            PositionEncoding::Utf16 => "utf-16",
            PositionEncoding::Utf32 => "utf-32",
        };
        Some(response_value(Response::success(
            id,
            Initialization {
                capabilities: Capabilities {
                    workspace: WorkspaceCapabilities {
                        workspace_folders: WorkspaceFolderCapabilities {
                            supported: true,
                            change_notifications: true,
                        },
                    },
                    position_encoding,
                    text_document_sync: DocumentSync {
                        open_close: true,
                        change: 1,
                    },
                    completion_provider: CompletionProvider {
                        trigger_characters: vec![":", "`", ">"],
                    },
                    hover_provider: true,
                    document_symbol_provider: true,
                    code_action_provider: true,
                    execute_command_provider: ExecuteCommandProvider {
                        commands: fleximark_protocol::COMMAND_DESCRIPTORS
                            .iter()
                            .map(|descriptor| descriptor.lsp_command)
                            .collect(),
                    },
                    experimental: Experimental {
                        fleximark: experimental,
                    },
                    semantic_tokens_provider: SemanticTokensProvider {
                        legend: Legend {
                            token_types: &super::language::SEMANTIC_TOKEN_TYPES,
                            token_modifiers: fleximark_protocol::SEMANTIC_TOKEN_MODIFIERS.to_vec(),
                        },
                        full: true,
                    },
                    diagnostic_provider: DiagnosticProvider {
                        inter_file_dependencies: false,
                        workspace_diagnostics: false,
                    },
                },
                server_info: ServerInfo {
                    name: "fleximarkd",
                    version: env!("CARGO_PKG_VERSION"),
                },
            },
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
                fleximark_protocol::error_code::INVALID_PARAMS,
                "completion requires textDocument.uri and position",
            )));
        };
        let (Ok(line32), Ok(character32)) = (u32::try_from(line), u32::try_from(character)) else {
            return Some(invalid_params(
                id,
                "position exceeds the document index range",
            ));
        };
        let result = (|| {
            let offset = self.position_offset(
                uri,
                &super::lsp_types::Position {
                    line: u64::from(line32),
                    character: u64::from(character32),
                },
            )?;
            let prefix = self.registry.line_prefix(uri, offset)?;
            let source = self.registry.source_for_uri(uri)?;
            let index = fleximark_model::SourceIndex::new(source);
            fleximark_language::completion_items(prefix, offset, self.snippet_support)
                .into_iter()
                .map(|item| {
                    super::language::completion(item, &index, self.registry.position_encoding())
                })
                .collect::<Result<Vec<_>, ApplicationError>>()
        })();
        let items = match result {
            Ok(items) => items,
            Err(error) => return Some(session_error(id, error)),
        };
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
                fleximark_protocol::error_code::INVALID_PARAMS,
                "semantic tokens require textDocument.uri",
            )));
        };
        let Ok(source) = self.registry.source_for_uri(uri) else {
            return Some(response_value(Response::success(id, Value::Null)));
        };
        Some(
            match super::language::semantic_data(source, self.registry.position_encoding()) {
                Ok(data) => response_value(Response::success(id, json!({"data":data}))),
                Err(error) => session_error(id, error),
            },
        )
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
                fleximark_protocol::error_code::INVALID_PARAMS,
                "invalid hover parameters",
            )));
        };
        let (Ok(line), Ok(character)) = (JsSafeU64::new(line), JsSafeU64::new(character)) else {
            return Some(response_value(Response::error(
                id,
                fleximark_protocol::error_code::INVALID_PARAMS,
                "hover position is outside the JavaScript safe integer range",
            )));
        };
        let Some(session_id) = self.registry.session_id_for_uri(uri) else {
            return Some(response_value(Response::error(
                id,
                fleximark_protocol::error_code::INVALID_PARAMS,
                "document is not open",
            )));
        };
        let Ok(version) = self.registry.current_version(session_id) else {
            return Some(response_value(Response::error(
                id,
                fleximark_protocol::error_code::INVALID_PARAMS,
                "document is not current",
            )));
        };
        let position = match native_position(&fleximark_protocol::TextPosition { line, character })
        {
            Ok(position) => position,
            Err(error) => return Some(session_error(id, error)),
        };
        let Ok(Some(entry)) = self.navigation_at_position(session_id, version, &position) else {
            return Some(response_value(Response::success(id, Value::Null)));
        };
        let Ok(document) = self.registry.document(session_id, version) else {
            return Some(response_value(Response::error(
                id,
                fleximark_protocol::error_code::INVALID_PARAMS,
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
            Hover {
                contents: MarkupContent {
                    kind: "markdown",
                    value: format!(
                        "**FlexiMark block**: `{kind}`\n\nUTF-8 source bytes {}..{}",
                        entry.source_range.byte_start, entry.source_range.byte_end
                    ),
                },
            },
        )))
    }

    pub(super) fn document_symbols(&self, id: Option<Value>, params: &Value) -> Option<Value> {
        let id = id?;
        let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) else {
            return Some(response_value(Response::error(
                id,
                fleximark_protocol::error_code::INVALID_PARAMS,
                "invalid symbol parameters",
            )));
        };
        let Some(session_id) = self.registry.session_id_for_uri(uri) else {
            return Some(response_value(Response::error(
                id,
                fleximark_protocol::error_code::INVALID_PARAMS,
                "document is not open",
            )));
        };
        let Ok(version) = self.registry.current_version(session_id) else {
            return Some(response_value(Response::error(
                id,
                fleximark_protocol::error_code::INVALID_PARAMS,
                "document is not current",
            )));
        };
        let Ok(document) = self.registry.document(session_id, version) else {
            return Some(response_value(Response::error(
                id,
                fleximark_protocol::error_code::INVALID_PARAMS,
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

    pub(super) fn refresh_changed_document_assets(
        &mut self,
        uri: &str,
        trace: &OperationalTrace,
    ) -> bool {
        let asset_started = Instant::now();
        if let Err(error) = self.refresh_assets(uri) {
            crate::telemetry::log_operational_failure("operation-failed", &error, None, None, None);
            eprintln!(
                "{}",
                trace.event("asset-refresh-failed", asset_started.elapsed(), None, false,)
            );
            return false;
        }
        true
    }
}
