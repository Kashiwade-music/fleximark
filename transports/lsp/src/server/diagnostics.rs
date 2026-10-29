//! Role: Publish source diagnostics and collect hierarchical document symbols.
use super::lsp_types::*;
use super::state::*;
use super::wire::*;

impl Server {
    pub(super) fn diagnostics(&self, id: Option<Value>, params: &Value) -> Option<Value> {
        let id = id?;
        let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) else {
            return Some(response_value(Response::error(
                id,
                fleximark_protocol::error_code::INVALID_PARAMS,
                "invalid diagnostic parameters",
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
        let diagnostics = collect_session_diagnostics(document);
        Some(response_value(Response::success(
            id,
            DiagnosticReport {
                kind: "full",
                items: diagnostics,
            },
        )))
    }

    pub(super) fn publish_lsp_diagnostics(&mut self, uri: &str) {
        if self.publication_cancellation.is_cancelled() {
            return;
        }
        let Some(session_id) = self.registry.session_id_for_uri(uri) else {
            return;
        };
        let Ok(version) = self.registry.current_version(session_id) else {
            return;
        };
        let Ok(document) = self.registry.document(session_id, version) else {
            return;
        };
        self.outgoing_events.push(super::wire::notification_value(
            "textDocument/publishDiagnostics",
            PublishDiagnostics {
                uri,
                version: Some(version),
                diagnostics: collect_session_diagnostics(document),
            },
        ));
    }
}

pub(super) fn find_block<'a>(document: &'a Document, node_id: &NodeId) -> Option<&'a Block> {
    fn visit<'a>(blocks: &'a [Block], node_id: &NodeId) -> Option<&'a Block> {
        for block in blocks {
            if &block.id == node_id {
                return Some(block);
            }
            for child in &block.children {
                if let Node::Block(child) = child {
                    if let Some(found) = visit(std::slice::from_ref(child), node_id) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }
    visit(&document.blocks, node_id)
}

fn inline_text(inline: &Inline, output: &mut String) {
    match &inline.kind {
        InlineKind::Text { value } | InlineKind::Code { value } => output.push_str(value),
        InlineKind::Emphasis { children }
        | InlineKind::Strong { children }
        | InlineKind::Strikethrough { children } => {
            for child in children {
                inline_text(child, output);
            }
        }
        InlineKind::Link { children, .. } | InlineKind::Image { children, .. } => {
            for child in children {
                inline_text(child, output);
            }
        }
        InlineKind::Math { source } => output.push_str(source),
        InlineKind::SoftBreak | InlineKind::HardBreak => output.push(' '),
        InlineKind::RawHtml { .. } => {}
    }
}

pub(super) fn collect_heading_symbols(
    blocks: &[Block],
    output: &mut Vec<Symbol>,
    source: &fleximark_model::SourceIndex,
    encoding: PositionEncoding,
) {
    for block in blocks {
        if let BlockKind::Heading { .. } = block.kind {
            {
                let range = &block.range;
                let Some(start) = source.byte_to_position(range.byte_start as usize, encoding)
                else {
                    continue;
                };
                let Some(end) = source.byte_to_position(range.byte_end as usize, encoding) else {
                    continue;
                };
                let mut name = String::new();
                for child in &block.children {
                    if let Node::Inline(inline) = child {
                        inline_text(inline, &mut name);
                    }
                }
                let range = Range {
                    start: start.into(),
                    end: end.into(),
                };
                output.push(Symbol {
                    name: if name.is_empty() {
                        "Heading".into()
                    } else {
                        name
                    },
                    kind: 13,
                    range: range.clone(),
                    selection_range: range,
                });
            }
        }
        let children = block
            .children
            .iter()
            .filter_map(|child| match child {
                Node::Block(block) => Some(block.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        collect_heading_symbols(&children, output, source, encoding);
    }
}

fn collect_session_diagnostics(
    document: &fleximark_application::DocumentSession,
) -> Vec<Diagnostic> {
    let source = fleximark_model::SourceIndex::new(document.source());
    document
        .asset_diagnostics()
        .iter()
        .filter_map(|diagnostic| {
            let range = if let Some(range) = &diagnostic.source_range {
                Range {
                    start: source
                        .byte_to_position(range.byte_start as usize, document.position_encoding())?
                        .into(),
                    end: source
                        .byte_to_position(range.byte_end as usize, document.position_encoding())?
                        .into(),
                }
            } else {
                Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 0,
                    },
                }
            };
            Some(Diagnostic {
                range,
                severity: 1,
                code: diagnostic.kind.code(),
                source: "fleximark",
                message: diagnostic.message.clone(),
            })
        })
        .collect()
}
