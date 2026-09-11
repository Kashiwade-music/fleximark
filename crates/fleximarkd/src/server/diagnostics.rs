use super::*;

impl Server {
    pub(super) fn diagnostics(&self, id: Option<Value>, params: &Value) -> Option<Value> {
        let id = id?;
        let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "invalid diagnostic parameters",
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
        let diagnostics = collect_session_diagnostics(document);
        Some(response_value(Response::success(
            id,
            json!({"kind":"full","items":diagnostics}),
        )))
    }

    pub(super) fn publish_lsp_diagnostics(&mut self, uri: &str) {
        if !self.lsp_mode || self.publication_cancellation.is_cancelled() {
            return;
        }
        let Some(session_id) = self.registry.session_id_for_uri(uri) else {
            return;
        };
        let daemon = self.registry.daemon_instance_id();
        let Ok(version) = self.registry.current_version(daemon, session_id) else {
            return;
        };
        let Ok(document) = self.registry.document(daemon, session_id, version) else {
            return;
        };
        self.outgoing_events.push(json!({
            "jsonrpc":"2.0",
            "method":"textDocument/publishDiagnostics",
            "params":{"uri":uri,"version":version,"diagnostics":collect_session_diagnostics(document)}
        }));
    }
}

pub(super) fn find_block<'a>(document: &'a Document, node_id: &NodeId) -> Option<&'a Block> {
    fn visit<'a>(blocks: &'a [Block], node_id: &NodeId) -> Option<&'a Block> {
        for block in blocks {
            if &block.id == node_id {
                return Some(block);
            }
            for child in &block.children {
                if let Node::Block(child) = child
                    && let Some(found) = visit(std::slice::from_ref(child), node_id)
                {
                    return Some(found);
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

pub(super) fn collect_heading_symbols(blocks: &[Block], output: &mut Vec<Value>) {
    for block in blocks {
        if let BlockKind::Heading { .. } = block.kind
            && let Some(range) = block.provenance.navigation_range()
        {
            let mut name = String::new();
            for child in &block.children {
                if let Node::Inline(inline) = child {
                    inline_text(inline, &mut name);
                }
            }
            output.push(json!({
                "name": if name.is_empty() { "Heading" } else { &name },
                "kind": 13,
                "range": {"start":{"line":range.start.line,"character":range.start.character},"end":{"line":range.end.line,"character":range.end.character}},
                "selectionRange": {"start":{"line":range.start.line,"character":range.start.character},"end":{"line":range.end.line,"character":range.end.character}}
            }));
        }
        let children = block
            .children
            .iter()
            .filter_map(|child| match child {
                Node::Block(block) => Some(block.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        collect_heading_symbols(&children, output);
    }
}

fn collect_raw_html_diagnostics(blocks: &[Block], output: &mut Vec<Value>) {
    for block in blocks {
        if let BlockKind::RawHtml { html } = &block.kind
            && let Some(range) = block.provenance.navigation_range()
        {
            output.push(json!({
                "range":{"start":{"line":range.start.line,"character":range.start.character},"end":{"line":range.end.line,"character":range.end.character}},
                "severity":2,
                "code":"raw-html",
                "source":"fleximark",
                "message":"Raw HTML is governed by the preview security policy",
                "data":{"escapedText":html.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")}
            }));
        }
        let children = block
            .children
            .iter()
            .filter_map(|child| match child {
                Node::Block(block) => Some(block.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        collect_raw_html_diagnostics(&children, output);
    }
}

fn collect_session_diagnostics(document: &fleximark_lsp::DocumentSession) -> Vec<Value> {
    let mut output = Vec::new();
    collect_raw_html_diagnostics(&document.document().blocks, &mut output);
    for diagnostic in document.asset_diagnostics() {
        let range = diagnostic.source_range.as_ref().map_or_else(
            || json!({"start":{"line":0,"character":0},"end":{"line":0,"character":0}}),
            |range| {
                json!({
                    "start":{"line":range.start.line,"character":range.start.character},
                    "end":{"line":range.end.line,"character":range.end.character}
                })
            },
        );
        output.push(json!({
            "range":range,
            "severity":1,
            "code":"asset",
            "source":"fleximark",
            "message":diagnostic.message
        }));
    }
    for diagnostic in document.plugin_diagnostics() {
        output.push(json!({
            "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},
            "severity":1,
            "code":"plugin",
            "source":format!("fleximark:{}", diagnostic.plugin_id),
            "message":diagnostic.message
        }));
    }
    output
}
