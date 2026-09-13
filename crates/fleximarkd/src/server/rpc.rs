use super::*;

impl Server {
    pub(super) fn fleximark_initialize(
        &mut self,
        id: Option<Value>,
        params: Value,
    ) -> Option<Value> {
        let id = id?;
        if self.lsp_mode && !self.lsp_initialized {
            return Some(response_value(Response::error(
                id,
                -32002,
                "LSP initialize must complete first",
            )));
        }
        let params = match deserialize_params::<InitializeParams>(&id, params) {
            Ok(params) => params,
            Err(response) => return Some(response),
        };
        if params.protocol_version != PROTOCOL_VERSION {
            return Some(response_value(Response::error(
                id,
                -32001,
                format!(
                    "unsupported protocol version {}; expected {PROTOCOL_VERSION}",
                    params.protocol_version
                ),
            )));
        }
        let generation = self.config_generation + 1;
        let mut configured = Vec::new();
        let mut grants = HashMap::new();
        let mut workspace_statuses = Vec::new();
        let mut disabled_workspaces = HashSet::new();
        for workspace in params.workspaces {
            if grants
                .insert(workspace.uri.clone(), workspace.trusted)
                .is_some()
            {
                return Some(response_value(Response::error(
                    id,
                    -32602,
                    "workspace URI is duplicated",
                )));
            }
            match fleximark_service::load_plugin_host(&workspace.uri, workspace.trusted, generation)
            {
                Ok((host, render_config)) => {
                    configured.push((workspace.uri.clone(), host, render_config));
                    workspace_statuses.push(WorkspaceStatus {
                        uri: workspace.uri,
                        enabled: true,
                        error: None,
                    });
                }
                Err(error) => {
                    grants.insert(workspace.uri.clone(), false);
                    disabled_workspaces.insert(workspace.uri.clone());
                    workspace_statuses.push(WorkspaceStatus {
                        uri: workspace.uri,
                        enabled: false,
                        error: Some(error.to_string()),
                    });
                }
            }
        }
        if let Err(error) = self.registry.configure_workspaces(configured) {
            return Some(session_error(id, error));
        }
        self.fleximark_initialized = true;
        self.config_generation = generation;
        self.selection_events = params.capabilities.selection_events;
        self.viewport_events = params.capabilities.viewport_events;
        self.workspaces = grants;
        self.disabled_workspaces = disabled_workspaces;
        Some(response_value(Response::success(
            id,
            InitializeResult {
                protocol_version: PROTOCOL_VERSION,
                daemon_instance_id: self.registry.daemon_instance_id().to_owned(),
                workspace_statuses,
                capabilities: ServerCapabilities {
                    html_render: true,
                    document_checkpoint: true,
                    selection_events: self.selection_events,
                    viewport_events: self.viewport_events,
                    workspace_commands: vec![
                        "initializeWorkspace",
                        "editTheme",
                        "collectAdmonitions",
                        "createNote",
                        "exportHtml",
                    ],
                },
            },
        )))
    }

    pub(super) fn request<P, R>(
        &mut self,
        id: Option<Value>,
        params: Value,
        action: impl FnOnce(&mut SessionRegistry, P) -> Result<R, SessionError>,
    ) -> Option<Value>
    where
        P: serde::de::DeserializeOwned,
        R: serde::Serialize,
    {
        let id = id?;
        if !self.fleximark_initialized {
            return Some(response_value(Response::error(
                id,
                -32002,
                "FlexiMark connection is not initialized",
            )));
        }
        let params = match deserialize_params(&id, params) {
            Ok(params) => params,
            Err(response) => return Some(response),
        };
        Some(match action(&mut self.registry, params) {
            Ok(result) => response_value(Response::success(id, result)),
            Err(error) => session_error(id, error),
        })
    }

    pub(super) fn render(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let id = id?;
        if !self.fleximark_initialized {
            return Some(response_value(Response::error(
                id,
                -32002,
                "FlexiMark connection is not initialized",
            )));
        }
        let params = match deserialize_params::<RenderParams>(&id, params) {
            Ok(params) => params,
            Err(response) => return Some(response),
        };
        let preview_id = format!("render-{}", params.document_session_id);
        let publication = match self.registry.render_with_cancellation(
            &params.daemon_instance_id,
            &params.document_session_id,
            params.document_version.get(),
            &preview_id,
            &self.work_cancellation,
        ) {
            Ok(publication) => publication,
            Err(error) => return Some(session_error(id, error)),
        };
        if self.publication_cancellation.is_cancelled() {
            return None;
        }
        Some(response_value(Response::success(id, publication)))
    }

    pub(super) fn create_preview(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let id = id?;
        if !self.fleximark_initialized {
            return Some(response_value(Response::error(
                id,
                -32002,
                "FlexiMark connection is not initialized",
            )));
        }
        let params = match deserialize_params::<CreatePreviewParams>(&id, params) {
            Ok(params) => params,
            Err(response) => return Some(response),
        };
        let preview_nonce = match random_token() {
            Ok(preview_nonce) => preview_nonce,
            Err(_) => {
                return Some(response_value(Response::error(
                    id,
                    -32603,
                    "operating system random source is unavailable",
                )));
            }
        };
        let preview_id = format!("preview-{preview_nonce}");
        let publication = match self.registry.render_with_cancellation(
            &params.daemon_instance_id,
            &params.document_session_id,
            params.expected_document_version.get(),
            &preview_id,
            &self.work_cancellation,
        ) {
            Ok(publication) => publication,
            Err(error) => return Some(session_error(id, error)),
        };
        let RenderPublication::Full(snapshot) = &publication else {
            return Some(response_value(Response::error(
                id,
                -32603,
                "new preview did not produce a full snapshot",
            )));
        };
        if self.publication_cancellation.is_cancelled() {
            return None;
        }
        let (token, url) = if params.target == PreviewTarget::ExternalBrowser {
            let token = match random_token() {
                Ok(token) => token,
                Err(_) => {
                    return Some(response_value(Response::error(
                        id,
                        -32603,
                        "operating system random source is unavailable",
                    )));
                }
            };
            let url = self.previews.publish(
                &token,
                &params.daemon_instance_id,
                &preview_id,
                &publication,
            );
            (Some(token), Some(url))
        } else {
            (None, None)
        };
        self.preview_states.insert(
            preview_id.clone(),
            PreviewState {
                token,
                document_session_id: params.document_session_id,
                delivered_revision: snapshot.result_render_revision,
            },
        );
        Some(response_value(Response::success(
            id,
            CreatePreviewResult {
                preview_session_id: preview_id,
                url,
                initial_publication: publication,
            },
        )))
    }

    pub(super) fn dispose_preview(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let params = match serde_json::from_value::<DisposePreviewParams>(params) {
            Ok(params) => params,
            Err(error) => return id.map(|id| invalid_params(id, error)),
        };
        if params.daemon_instance_id != self.registry.daemon_instance_id() {
            return id.map(|id| session_error(id, SessionError::WrongDaemon));
        }
        if let Some(state) = self.preview_states.remove(&params.preview_session_id) {
            if let Err(error) = self.registry.dispose_preview(
                &params.daemon_instance_id,
                &state.document_session_id,
                &params.preview_session_id,
            ) {
                return id.map(|id| session_error(id, error));
            }
            if let Some(token) = &state.token {
                self.previews.remove(token);
            }
        }
        id.map(|id| response_value(Response::success(id, Value::Null)))
    }

    pub(super) fn open_rpc_document(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let id = id?;
        let params = match deserialize_params::<RpcOpenDocumentParams>(&id, params) {
            Ok(params) => params,
            Err(response) => return Some(response),
        };
        let uri = params.uri.clone();
        if self
            .workspace_for_document(&uri)
            .is_some_and(|(workspace_uri, _)| self.disabled_workspaces.contains(&workspace_uri))
        {
            return Some(response_value(Response::error(
                id,
                -32022,
                "workspace configuration is disabled",
            )));
        }
        Some(
            match self
                .registry
                .open_rpc_with_cancellation(params, &self.work_cancellation)
                .and_then(|result| {
                    self.refresh_assets(&uri)?;
                    Ok(result)
                }) {
                Ok(result) => response_value(Response::success(id, result)),
                Err(error) => session_error(id, error),
            },
        )
    }

    pub(super) fn change_rpc_document(
        &mut self,
        id: Option<Value>,
        params: Value,
    ) -> Option<Value> {
        let id = id?;
        if !self.fleximark_initialized {
            return Some(response_value(Response::error(
                id,
                -32002,
                "FlexiMark connection is not initialized",
            )));
        }
        let params = match deserialize_params::<RpcChangeDocumentParams>(&id, params) {
            Ok(params) => params,
            Err(response) => return Some(response),
        };
        match self
            .registry
            .change_rpc_with_cancellation(params, &self.work_cancellation)
        {
            Ok(result) if !self.publication_cancellation.is_cancelled() => {
                Some(response_value(Response::success(id, result)))
            }
            Ok(_) => None,
            Err(error) => Some(session_error(id, error)),
        }
    }

    pub(super) fn refresh_assets(&mut self, uri: &str) -> Result<(), SessionError> {
        let session_id = self
            .registry
            .session_id_for_uri(uri)
            .ok_or(SessionError::UnknownSession)?
            .to_owned();
        let daemon = self.registry.daemon_instance_id().to_owned();
        let version = self.registry.current_version(&daemon, &session_id)?;
        let session = self.registry.document(&daemon, &session_id, version)?;
        let Some(workspace_uri) = session.workspace_uri().map(str::to_owned) else {
            return Ok(());
        };
        if !self
            .workspaces
            .get(&workspace_uri)
            .copied()
            .unwrap_or(false)
        {
            return Ok(());
        }
        let document = session.document().clone();
        let assets = fleximark_service::resolve_render_assets(uri, &workspace_uri, &document)
            .map_err(|error| SessionError::Engine(error.to_string()))?;
        self.registry.reconfigure_document_assets(uri, assets)
    }

    pub(super) fn set_selection(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let params = match serde_json::from_value::<SetSelectionParams>(params) {
            Ok(params) => params,
            Err(error) => return id.map(|id| invalid_params(id, error)),
        };
        let mut node_ids = Vec::new();
        for selection in &params.selections {
            match self.registry.navigation_at_position(
                &params.daemon_instance_id,
                &params.document_session_id,
                params.expected_document_version.get(),
                &selection.active,
            ) {
                Ok(Some(entry)) if !node_ids.contains(&entry.node_id) => {
                    node_ids.push(entry.node_id)
                }
                Ok(_) => {}
                Err(error) => return id.map(|id| session_error(id, error)),
            }
        }
        if !self.selection_events {
            return id.map(|id| response_value(Response::success(id, Value::Null)));
        }
        for (preview_id, state) in &self.preview_states {
            if state.document_session_id == params.document_session_id {
                let event = RenderNavigationEvent::Selection {
                    preview_session_id: preview_id.clone(),
                    render_revision: state.delivered_revision,
                    node_ids: node_ids.clone(),
                    active_position: params
                        .selections
                        .first()
                        .map(|selection| selection.active.clone()),
                };
                if let Some(token) = &state.token {
                    self.previews.navigate(token, &event);
                }
                self.outgoing_events
                    .push(preview_event_notification(ServerPreviewEventParams {
                        daemon_instance_id: params.daemon_instance_id.clone(),
                        preview_session_id: preview_id.clone(),
                        render_revision: state.delivered_revision,
                        event: ServerPreviewEvent::RenderNavigation(event),
                    }));
            }
        }
        id.map(|id| response_value(Response::success(id, Value::Null)))
    }

    pub(super) fn set_viewport(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let params = match serde_json::from_value::<SetViewportParams>(params) {
            Ok(params) => params,
            Err(error) => return id.map(|id| invalid_params(id, error)),
        };
        let node_id = match params.ranges.first() {
            Some(range) => match self.registry.navigation_at_position(
                &params.daemon_instance_id,
                &params.document_session_id,
                params.expected_document_version.get(),
                &range.start,
            ) {
                Ok(Some(entry)) => Some(entry.node_id),
                Ok(None) => None,
                Err(error) => return id.map(|id| session_error(id, error)),
            },
            None => None,
        };
        if !self.viewport_events {
            return id.map(|id| response_value(Response::success(id, Value::Null)));
        }
        let Some(node_id) = node_id else {
            return id.map(|id| response_value(Response::success(id, Value::Null)));
        };
        for (preview_id, state) in &self.preview_states {
            if state.document_session_id == params.document_session_id {
                let event = RenderNavigationEvent::Viewport {
                    preview_session_id: preview_id.clone(),
                    render_revision: state.delivered_revision,
                    node_id: node_id.clone(),
                };
                if let Some(token) = &state.token {
                    self.previews.navigate(token, &event);
                }
                self.outgoing_events
                    .push(preview_event_notification(ServerPreviewEventParams {
                        daemon_instance_id: params.daemon_instance_id.clone(),
                        preview_session_id: preview_id.clone(),
                        render_revision: state.delivered_revision,
                        event: ServerPreviewEvent::RenderNavigation(event),
                    }));
            }
        }
        id.map(|id| response_value(Response::success(id, Value::Null)))
    }

    pub(super) fn preview_event(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let params = match serde_json::from_value::<PreviewEventParams>(params) {
            Ok(params) => params,
            Err(error) => return id.map(|id| invalid_params(id, error)),
        };
        if params.daemon_instance_id != self.registry.daemon_instance_id() {
            return id.map(|id| session_error(id, SessionError::WrongDaemon));
        }
        let Some(state) = self.preview_states.get(&params.preview_session_id) else {
            return id
                .map(|id| response_value(Response::error(id, -32602, "unknown preview session")));
        };
        let navigation = params.event;
        let (event_preview_id, event_revision, node_id) = match &navigation {
            PreviewNavigationEvent::SelectNode {
                preview_session_id,
                render_revision,
                node_id,
            }
            | PreviewNavigationEvent::RevealNode {
                preview_session_id,
                render_revision,
                node_id,
            } => (preview_session_id, *render_revision, node_id),
        };
        if params.render_revision != state.delivered_revision
            || event_preview_id != &params.preview_session_id
            || event_revision != state.delivered_revision
        {
            return id.map(|id| {
                response_value(Response::error(
                    id,
                    CONTENT_MODIFIED,
                    "render revision does not match the displayed preview",
                ))
            });
        }
        let version = match self
            .registry
            .current_version(&params.daemon_instance_id, &state.document_session_id)
        {
            Ok(version) => version,
            Err(error) => return id.map(|id| session_error(id, error)),
        };
        let entry = match self.registry.navigation_for_node(
            &params.daemon_instance_id,
            &state.document_session_id,
            version,
            node_id,
        ) {
            Ok(Some(entry)) => entry,
            Ok(None) => {
                return id.map(|id| {
                    response_value(Response::error(id, -32602, "unknown rendered node"))
                });
            }
            Err(error) => return id.map(|id| session_error(id, error)),
        };
        let event = match navigation {
            PreviewNavigationEvent::SelectNode { .. } => SourceNavigationEvent::SelectSource {
                source_range: entry.source_range,
            },
            PreviewNavigationEvent::RevealNode { .. } => SourceNavigationEvent::RevealSource {
                source_range: entry.source_range,
            },
        };
        self.outgoing_events
            .push(preview_event_notification(ServerPreviewEventParams {
                daemon_instance_id: params.daemon_instance_id,
                preview_session_id: params.preview_session_id,
                render_revision: state.delivered_revision,
                event: ServerPreviewEvent::SourceNavigation(event),
            }));
        id.map(|id| response_value(Response::success(id, Value::Null)))
    }

    pub(super) fn reload_preview(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let params = match serde_json::from_value::<ReloadPreviewParams>(params) {
            Ok(params) => params,
            Err(error) => return id.map(|id| invalid_params(id, error)),
        };
        if params.daemon_instance_id != self.registry.daemon_instance_id() {
            return id.map(|id| session_error(id, SessionError::WrongDaemon));
        }
        let Some(state) = self.preview_states.get(&params.preview_session_id).cloned() else {
            return id
                .map(|id| response_value(Response::error(id, -32602, "unknown preview session")));
        };
        let document_version = match self
            .registry
            .current_version(&params.daemon_instance_id, &state.document_session_id)
        {
            Ok(version) => version,
            Err(error) => return id.map(|id| session_error(id, error)),
        };
        let snapshot = match self.registry.render_full_with_cancellation(
            &params.daemon_instance_id,
            &state.document_session_id,
            document_version,
            &params.preview_session_id,
            &self.work_cancellation,
        ) {
            Ok(snapshot) => snapshot,
            Err(error) => return id.map(|id| session_error(id, error)),
        };
        if self.publication_cancellation.is_cancelled() {
            return None;
        }
        let revision = snapshot.result_render_revision;
        if let Some(token) = &state.token {
            self.previews
                .update(token, &RenderPublication::Full(snapshot.clone()));
        }
        self.preview_states
            .get_mut(&params.preview_session_id)
            .unwrap()
            .delivered_revision = revision;
        self.outgoing_events
            .push(preview_event_notification(ServerPreviewEventParams {
                daemon_instance_id: params.daemon_instance_id,
                preview_session_id: params.preview_session_id,
                render_revision: revision,
                event: ServerPreviewEvent::Publication(RenderPublication::Full(snapshot)),
            }));
        id.map(|id| response_value(Response::success(id, Value::Null)))
    }

    pub(super) fn reconfigure_workspace(
        &mut self,
        id: Option<Value>,
        params: Value,
    ) -> Option<Value> {
        let id = id?;
        let params = match deserialize_params::<ReconfigureWorkspaceParams>(&id, params) {
            Ok(params) => params,
            Err(response) => return Some(response),
        };
        if params.daemon_instance_id != self.registry.daemon_instance_id() {
            return Some(session_error(id, SessionError::WrongDaemon));
        }
        if !self.workspaces.contains_key(&params.workspace_uri) {
            return Some(response_value(Response::error(
                id,
                -32602,
                "workspace is not registered",
            )));
        }
        let generation = self.config_generation + 1;
        let (host, render_config) = match fleximark_service::load_plugin_host(
            &params.workspace_uri,
            params.trusted,
            generation,
        ) {
            Ok(configured) => configured,
            Err(error) => {
                return Some(response_value(Response::error(
                    id,
                    -32022,
                    error.to_string(),
                )));
            }
        };
        let mut assets = HashMap::new();
        for (uri, document) in self.registry.workspace_documents(&params.workspace_uri) {
            let resolved = if params.trusted {
                match fleximark_service::resolve_render_assets(
                    &uri,
                    &params.workspace_uri,
                    &document,
                ) {
                    Ok(assets) => assets,
                    Err(error) => {
                        return Some(response_value(Response::error(
                            id,
                            -32022,
                            error.to_string(),
                        )));
                    }
                }
            } else {
                Vec::new()
            };
            assets.insert(uri, resolved);
        }
        if let Err(error) =
            self.registry
                .reconfigure_workspace(&params.workspace_uri, host, render_config, assets)
        {
            return Some(session_error(id, error));
        }
        self.config_generation = generation;
        self.workspaces
            .insert(params.workspace_uri.clone(), params.trusted);
        self.disabled_workspaces.remove(&params.workspace_uri);

        let daemon = self.registry.daemon_instance_id().to_owned();
        let sessions = self
            .registry
            .workspace_documents(&params.workspace_uri)
            .into_iter()
            .filter_map(|(uri, _)| self.registry.session_id_for_uri(&uri).map(str::to_owned))
            .collect::<Vec<_>>();
        for (preview_id, state) in self.preview_states.clone() {
            if !sessions.contains(&state.document_session_id) {
                continue;
            }
            let version = match self
                .registry
                .current_version(&daemon, &state.document_session_id)
            {
                Ok(version) => version,
                Err(error) => return Some(session_error(id, error)),
            };
            let snapshot = match self.registry.render_full_with_cancellation(
                &daemon,
                &state.document_session_id,
                version,
                &preview_id,
                &self.work_cancellation,
            ) {
                Ok(snapshot) => snapshot,
                Err(error) => return Some(session_error(id, error)),
            };
            if self.publication_cancellation.is_cancelled() {
                return None;
            }
            if let Some(token) = &state.token {
                self.previews
                    .update(token, &RenderPublication::Full(snapshot.clone()));
            }
            if let Some(current) = self.preview_states.get_mut(&preview_id) {
                current.delivered_revision = snapshot.result_render_revision;
            }
            self.outgoing_events
                .push(preview_event_notification(ServerPreviewEventParams {
                    daemon_instance_id: daemon.clone(),
                    preview_session_id: preview_id,
                    render_revision: snapshot.result_render_revision,
                    event: ServerPreviewEvent::Publication(RenderPublication::Full(snapshot)),
                }));
        }
        Some(response_value(Response::success(id, Value::Null)))
    }

    pub(super) fn workspace_grants_write(&self, requested_uri: &str) -> bool {
        self.workspaces.iter().any(|(granted_uri, trusted)| {
            *trusted
                && match (
                    fleximark_service::workspace_path(granted_uri),
                    fleximark_service::workspace_path(requested_uri),
                ) {
                    (Ok(granted), Ok(requested)) => granted == requested,
                    _ => false,
                }
        })
    }

    pub(super) fn workspace_for_document(&self, document_uri: &str) -> Option<(String, bool)> {
        self.workspaces
            .iter()
            .filter(|(workspace_uri, _)| {
                fleximark_service::document_is_in_workspace(document_uri, workspace_uri)
                    .unwrap_or(false)
            })
            .max_by_key(|(workspace_uri, _)| workspace_uri.len())
            .map(|(uri, trusted)| (uri.clone(), *trusted))
    }
}
