use super::*;

impl Server {
    #[cfg(test)]
    pub(crate) fn new(lsp_mode: bool) -> Self {
        Self::build(lsp_mode, None)
    }

    pub(crate) fn with_sender(lsp_mode: bool, sender: Sender<Value>) -> Self {
        Self::build(lsp_mode, Some(sender))
    }

    pub(crate) fn build(lsp_mode: bool, sender: Option<Sender<Value>>) -> Self {
        Self {
            lsp_mode,
            lsp_initialized: false,
            snippet_support: false,
            fleximark_initialized: false,
            selection_events: false,
            viewport_events: false,
            workspaces: HashMap::new(),
            disabled_workspaces: HashSet::new(),
            config_generation: 0,
            shutdown: false,
            exit: false,
            registry: SessionRegistry::new(PositionEncoding::Utf16),
            previews: PreviewServer::start(sender).expect("loopback preview server must start"),
            preview_states: HashMap::new(),
            outgoing_events: Vec::new(),
            work_cancellation: CancellationToken::default(),
            publication_cancellation: CancellationToken::default(),
        }
    }

    pub(crate) fn handle_cancellable(
        &mut self,
        message: IncomingMessage,
        cancellation: Option<CancellationToken>,
        publication_cancellation: Option<CancellationToken>,
    ) -> Vec<Value> {
        self.work_cancellation = cancellation.unwrap_or_default();
        self.publication_cancellation = publication_cancellation.unwrap_or_default();
        let outgoing = self.handle(message);
        self.work_cancellation = CancellationToken::default();
        self.publication_cancellation = CancellationToken::default();
        outgoing
    }

    pub(crate) fn bind_document_alias(
        &self,
        message: &IncomingMessage,
        coordinator: &CancellationCoordinator,
    ) {
        let uri = message
            .params
            .pointer("/textDocument/uri")
            .or_else(|| message.params.get("uri"))
            .and_then(Value::as_str);
        if let Some(uri) = uri {
            if let Some(session_id) = self.registry.session_id_for_uri(uri) {
                coordinator.bind(session_id, uri);
            }
        }
    }

    pub(crate) fn handle(&mut self, message: IncomingMessage) -> Vec<Value> {
        let id = message.id.clone().map(Value::from);
        let response = match message.method.as_str() {
            "initialize" if self.lsp_mode => self.lsp_initialize(id, &message.params),
            "initialized" if self.lsp_mode => None,
            "shutdown" if self.lsp_mode => {
                self.shutdown = true;
                id.map(|id| response_value(Response::success(id, Value::Null)))
            }
            "exit" if self.lsp_mode => {
                self.exit = true;
                None
            }
            "$/cancelRequest" => None,
            "textDocument/didOpen" if self.lsp_mode => self.open_lsp_document(id, message.params),
            "textDocument/didChange" if self.lsp_mode => self.change_document(id, message.params),
            "textDocument/didClose" if self.lsp_mode => self.close_document(id, message.params),
            "textDocument/completion" if self.lsp_mode => self.completion(id, &message.params),
            "textDocument/semanticTokens/full" if self.lsp_mode => {
                self.semantic_tokens(id, &message.params)
            }
            "textDocument/hover" if self.lsp_mode => self.hover(id, &message.params),
            "textDocument/documentSymbol" if self.lsp_mode => {
                self.document_symbols(id, &message.params)
            }
            "textDocument/diagnostic" if self.lsp_mode => self.diagnostics(id, &message.params),
            "textDocument/codeAction" if self.lsp_mode => self.code_actions(id, &message.params),
            method::INITIALIZE => self.fleximark_initialize(id, message.params),
            method::ATTACH_DOCUMENT => self.request(
                id,
                message.params,
                |registry, params: AttachDocumentParams| registry.attach(&params),
            ),
            method::CHECKPOINT_DOCUMENT => self.request(
                id,
                message.params,
                |registry, params: CheckpointDocumentParams| registry.checkpoint(&params),
            ),
            method::CREATE_PREVIEW => self.create_preview(id, message.params),
            method::READ_PREVIEW => self.read_preview(id, message.params),
            method::RERENDER_PREVIEW => self.rerender_preview(id, message.params),
            method::DISPOSE_PREVIEW => self.dispose_preview(id, message.params),
            method::SET_SELECTION => self.set_selection(id, message.params),
            method::SET_VIEWPORT => self.set_viewport(id, message.params),
            method::PREVIEW_EVENT => self.preview_event(id, message.params),
            method::EXECUTE_COMMAND => self.execute_command(id, message.params),
            method::GET_NOTE_OPTIONS => self.get_note_options(id, message.params),
            method::RECONFIGURE_WORKSPACE => self.reconfigure_workspace(id, message.params),
            method::OPEN_DOCUMENT => self.open_rpc_document(id, message.params),
            method::CHANGE_DOCUMENT if !self.lsp_mode => {
                self.change_rpc_document(id, message.params)
            }
            method::CLOSE_DOCUMENT if !self.lsp_mode => self.request(
                id,
                message.params,
                |registry, params: RpcCloseDocumentParams| registry.close_rpc(params),
            ),
            _ => id.map(|id| response_value(Response::error(id, -32601, "method not found"))),
        };

        let mut outgoing = response.into_iter().collect::<Vec<_>>();
        outgoing.extend(
            self.registry
                .take_full_text_requests()
                .into_iter()
                .map(|params| {
                    serde_json::to_value(Notification {
                        jsonrpc: "2.0",
                        method: method::REQUEST_FULL_TEXT,
                        params,
                    })
                    .unwrap()
                }),
        );
        outgoing.append(&mut self.outgoing_events);
        outgoing
    }
}
