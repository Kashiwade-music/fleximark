//! Role: Deliver LSP and RPC requests and session events over the daemon transport.
use super::state::*;
use super::wire::*;

impl Server {
    pub fn sessions(&self) -> &SessionRegistry {
        &self.registry
    }

    pub fn set_workspace_handler(
        &self,
        handler: Arc<dyn Fn(Value) -> Result<Value, String> + Send + Sync>,
    ) {
        self.previews.set_workspace_handler(handler);
    }

    pub fn new(assets: crate::preview_http::BrowserAssets) -> Self {
        Self::with_assets(None, assets)
    }

    pub fn with_assets(
        sender: Option<Sender<Value>>,
        assets: crate::preview_http::BrowserAssets,
    ) -> Self {
        Self {
            next_client_request: 1,
            pending_client_requests: HashSet::new(),
            lsp_initialized: false,
            snippet_support: false,
            selection_events: false,
            viewport_events: false,
            shutdown: false,
            exit: false,
            registry: SessionRegistry::new(PositionEncoding::Utf16),
            previews: PreviewServer::with_assets(sender, assets)
                .expect("loopback preview server must start"),
            preview_states: HashMap::new(),
            outgoing_events: Vec::new(),
            work_cancellation: CancellationToken::default(),
            publication_cancellation: CancellationToken::default(),
        }
    }

    pub fn handle_cancellable(
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

    pub fn handle(&mut self, message: IncomingMessage) -> Vec<Value> {
        let id = message.id.clone().map(Value::from);
        if self.shutdown && message.method != "exit" {
            return id
                .map(|id| response_value(Response::error(id, -32600, "server has shut down")))
                .into_iter()
                .collect();
        }
        if !self.lsp_initialized
            && !matches!(
                message.method.as_str(),
                "initialize" | "exit" | "$/cancelRequest"
            )
        {
            return id
                .map(|id| response_value(Response::error(id, -32002, "server is not initialized")))
                .into_iter()
                .collect();
        }
        let response = match message.method.as_str() {
            "initialize" => self.lsp_initialize(id, &message.params),
            "initialized" => None,
            "shutdown" => {
                self.shutdown = true;
                id.map(|id| response_value(Response::success(id, Value::Null)))
            }
            "exit" => {
                self.exit = true;
                None
            }
            "$/cancelRequest" => None,
            "textDocument/didOpen" => self.open_lsp_document(id, message.params),
            "textDocument/didChange" => self.change_document(id, message.params),
            "textDocument/didClose" => self.close_document(id, message.params),
            "textDocument/completion" => self.completion(id, &message.params),
            "textDocument/semanticTokens/full" => self.semantic_tokens(id, &message.params),
            "textDocument/hover" => self.hover(id, &message.params),
            "textDocument/documentSymbol" => self.document_symbols(id, &message.params),
            "textDocument/diagnostic" => self.diagnostics(id, &message.params),
            "textDocument/codeAction" => self.code_actions(id, &message.params),
            "workspace/executeCommand" => self.standard_execute_command(id, message.params),
            method::ATTACH_DOCUMENT => self.attach_document(id, message.params),
            method::CREATE_PREVIEW => self.create_preview(id, message.params),
            method::RERENDER_PREVIEW => self.rerender_preview(id, message.params),
            method::DISPOSE_PREVIEW => self.dispose_preview(id, message.params),
            method::SET_SELECTION => self.set_selection(id, message.params),
            method::SET_VIEWPORT => self.set_viewport(id, message.params),
            method::PREVIEW_EVENT => self.preview_event(id, message.params),
            method::EXECUTE_COMMAND => self.execute_command(id, message.params),
            method::GET_NOTE_OPTIONS => self.get_note_options(id, message.params),
            method::RECONFIGURE_WORKSPACE => self.reconfigure_workspace(id, message.params),
            _ => id.map(|id| response_value(Response::error(id, -32601, "method not found"))),
        };

        let mut outgoing = response.into_iter().collect::<Vec<_>>();
        outgoing.append(&mut self.outgoing_events);
        outgoing
    }
}
