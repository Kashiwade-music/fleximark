//! Role: Adapt custom preview methods and native workspace reconfiguration.
use super::protocol::{counter, native_position, range, wire_frame};
use super::state::*;
use super::wire::*;

impl Server {
    pub(super) fn initialize_workspaces(
        &mut self,
        roots: Vec<(String, bool)>,
        options: &Value,
    ) -> Result<InitializeResult, ApplicationError> {
        let capabilities: fleximark_protocol::ClientCapabilities = options
            .get("capabilities")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        let statuses = self.registry.configure_native_workspaces(roots)?;
        self.selection_events = capabilities.selection_events;
        self.viewport_events = capabilities.viewport_events;
        Ok(InitializeResult {
            protocol_version: PROTOCOL_VERSION,
            daemon_instance_id: self.daemon_instance_id().to_owned(),
            workspace_statuses: statuses
                .into_iter()
                .map(|status| WorkspaceStatus {
                    uri: status.uri,
                    enabled: !matches!(
                        status.state,
                        fleximark_application::WorkspaceState::Invalid(_)
                    ),
                    error: match status.state {
                        fleximark_application::WorkspaceState::Invalid(error) => Some(error),
                        _ => None,
                    },
                })
                .collect(),
            capabilities: ServerCapabilities {
                html_render: true,
                selection_events: self.selection_events,
                viewport_events: self.viewport_events,
                workspace_commands: fleximark_protocol::COMMAND_DESCRIPTORS
                    .iter()
                    .map(|descriptor| descriptor.name)
                    .collect(),
            },
        })
    }

    pub(super) fn create_preview(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let id = id?;
        let params = match deserialize_params::<CreatePreviewParams>(&id, params) {
            Ok(params) => params,
            Err(response) => return Some(response),
        };
        let result = self
            .verify_daemon(&params.daemon_instance_id)
            .and_then(|_| {
                let version = u64::try_from(params.expected_document_version.get())
                    .map_err(|_| ApplicationError::VersionMismatch)?;
                self.open_preview(&params.document_session_id, version, params.target, None)
            });
        if self.publication_cancellation.is_cancelled() {
            return None;
        }
        Some(match result {
            Ok(result) => response_value(Response::success(id, result)),
            Err(error) => session_error(id, error),
        })
    }
    pub(super) fn open_preview(
        &mut self,
        session: &str,
        version: u64,
        target: PreviewTarget,
        source_offset: Option<u64>,
    ) -> Result<CreatePreviewResult, ApplicationError> {
        let preview_id = format!("preview-{}", random_token()?);
        let frame = self.registry.render_with_cancellation(
            session,
            version,
            &preview_id,
            &self.work_cancellation,
        )?;
        if self.publication_cancellation.is_cancelled() {
            return Err(ApplicationError::Cancelled);
        }
        let (token, url) = if target == PreviewTarget::ExternalBrowser {
            let token = random_token()?;
            let url =
                self.previews
                    .publish(&token, self.daemon_instance_id(), &preview_id, &frame)?;
            (Some(token), Some(url))
        } else {
            (None, None)
        };
        let explicit_jump = source_offset
            .map(|offset| {
                let document = self.registry.document(session, version)?;
                let position = fleximark_model::SourceIndex::new(document.source())
                    .byte_to_position(
                        usize::try_from(offset).map_err(|_| ApplicationError::InvalidRange)?,
                        document.position_encoding(),
                    )
                    .ok_or(ApplicationError::InvalidRange)?;
                Ok::<_, ApplicationError>(super::lsp_types::Position {
                    line: position.line,
                    character: position.character,
                })
            })
            .transpose()?;
        self.preview_states.insert(
            preview_id.clone(),
            PreviewState {
                token,
                document_session_id: session.to_owned(),
                explicit_jump,
                delivered_revision: frame.render_revision,
            },
        );
        self.refresh_explicit_jump(&preview_id)?;
        Ok(CreatePreviewResult {
            preview_session_id: preview_id,
            frame: wire_frame(&frame)?,
            url,
        })
    }

    pub(super) fn rerender_preview(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let id = id?;
        let params = match deserialize_params::<RerenderPreviewParams>(&id, params) {
            Ok(params) => params,
            Err(response) => return Some(response),
        };
        if let Err(error) = self.verify_daemon(&params.daemon_instance_id) {
            return Some(session_error(id, error));
        }
        let Some(state) = self.preview_states.get(&params.preview_session_id).cloned() else {
            return Some(response_value(Response::error(
                id,
                fleximark_protocol::error_code::INVALID_PARAMS,
                "unknown preview session",
            )));
        };
        let version = match self.registry.current_version(&state.document_session_id) {
            Ok(version) => version,
            Err(error) => return Some(session_error(id, error)),
        };
        let frame = match self.registry.render_with_cancellation(
            &state.document_session_id,
            version,
            &params.preview_session_id,
            &self.work_cancellation,
        ) {
            Ok(frame) => frame,
            Err(error) => return Some(session_error(id, error)),
        };
        match self.publish_frame(&params.preview_session_id, &frame) {
            Ok(true) => {}
            Ok(false) => return None,
            Err(error) => return Some(boundary_error(id, error)),
        }
        Some(response_value(Response::success(id, Value::Null)))
    }

    pub(super) fn dispose_preview(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let params = match serde_json::from_value::<DisposePreviewParams>(params) {
            Ok(params) => params,
            Err(error) => return id.map(|id| invalid_params(id, error)),
        };
        if let Err(error) = self.verify_daemon(&params.daemon_instance_id) {
            return id.map(|id| session_error(id, error));
        }
        if let Some(state) = self.preview_states.remove(&params.preview_session_id) {
            if let Err(error) = self
                .registry
                .dispose_preview(&state.document_session_id, &params.preview_session_id)
            {
                return id.map(|id| session_error(id, error));
            }
            if let Some(token) = &state.token {
                self.previews.remove(token);
            }
        }
        id.map(|id| response_value(Response::success(id, Value::Null)))
    }

    pub(super) fn refresh_assets(&mut self, uri: &str) -> Result<(), ApplicationError> {
        self.registry.refresh_document_assets(uri)
    }

    pub(super) fn set_selection(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let params = match serde_json::from_value::<SetSelectionParams>(params) {
            Ok(params) => params,
            Err(error) => return id.map(|id| invalid_params(id, error)),
        };
        if let Err(error) = self.verify_daemon(&params.daemon_instance_id) {
            return id.map(|id| session_error(id, error));
        }
        let mut node_ids = Vec::new();
        for selection in &params.selections {
            let position = match native_position(&selection.active) {
                Ok(position) => position,
                Err(error) => return id.map(|id| session_error(id, error)),
            };
            match self.navigation_at_position(
                &params.document_session_id,
                match u64::try_from(params.expected_document_version.get()) {
                    Ok(version) => version,
                    Err(_) => {
                        return id.map(|id| session_error(id, ApplicationError::VersionMismatch));
                    }
                },
                &position,
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
                    render_revision: match counter(state.delivered_revision) {
                        Ok(revision) => revision,
                        Err(error) => return id.map(|id| boundary_error(id, error)),
                    },
                    node_ids: node_ids
                        .iter()
                        .map(|id| fleximark_protocol::NodeId(id.0.clone()))
                        .collect(),
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
                        render_revision: match counter(state.delivered_revision) {
                            Ok(revision) => revision,
                            Err(error) => return id.map(|id| boundary_error(id, error)),
                        },
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
        if let Err(error) = self.verify_daemon(&params.daemon_instance_id) {
            return id.map(|id| session_error(id, error));
        }
        let position = match params
            .ranges
            .first()
            .map(|range| native_position(&range.start))
            .transpose()
        {
            Ok(position) => position,
            Err(error) => return id.map(|id| session_error(id, error)),
        };
        let node_id = match position {
            Some(position) => match self.navigation_at_position(
                &params.document_session_id,
                match u64::try_from(params.expected_document_version.get()) {
                    Ok(version) => version,
                    Err(_) => {
                        return id.map(|id| session_error(id, ApplicationError::VersionMismatch));
                    }
                },
                &position,
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
                    render_revision: match counter(state.delivered_revision) {
                        Ok(revision) => revision,
                        Err(error) => return id.map(|id| boundary_error(id, error)),
                    },
                    node_id: fleximark_protocol::NodeId(node_id.0.clone()),
                };
                if let Some(token) = &state.token {
                    self.previews.navigate(token, &event);
                }
                self.outgoing_events
                    .push(preview_event_notification(ServerPreviewEventParams {
                        daemon_instance_id: params.daemon_instance_id.clone(),
                        preview_session_id: preview_id.clone(),
                        render_revision: match counter(state.delivered_revision) {
                            Ok(revision) => revision,
                            Err(error) => return id.map(|id| boundary_error(id, error)),
                        },
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
        if let Err(error) = self.verify_daemon(&params.daemon_instance_id) {
            return id.map(|id| session_error(id, error));
        }
        let Some(state) = self.preview_states.get(&params.preview_session_id) else {
            return id.map(|id| {
                response_value(Response::error(
                    id,
                    fleximark_protocol::error_code::INVALID_PARAMS,
                    "unknown preview session",
                ))
            });
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
        if params.render_revision.get() != state.delivered_revision
            || event_preview_id != &params.preview_session_id
            || event_revision.get() != state.delivered_revision
        {
            return id.map(|id| {
                response_value(Response::error(
                    id,
                    CONTENT_MODIFIED,
                    "render revision does not match the displayed preview",
                ))
            });
        }
        let entry = match self.registry.navigate_preview(
            &state.document_session_id,
            &params.preview_session_id,
            state.delivered_revision,
            &NodeId(node_id.0.clone()),
        ) {
            Ok(Some(entry)) => entry,
            Ok(None) => {
                return id.map(|id| {
                    response_value(Response::error(
                        id,
                        fleximark_protocol::error_code::INVALID_PARAMS,
                        "unknown rendered node",
                    ))
                });
            }
            Err(error) => return id.map(|id| session_error(id, error)),
        };
        let source_frame = match self.registry.read_preview(
            &state.document_session_id,
            &params.preview_session_id,
            None,
        ) {
            Ok(Some(frame)) => frame,
            Ok(None) => return id.map(|id| session_error(id, ApplicationError::UnknownSession)),
            Err(error) => return id.map(|id| session_error(id, error)),
        };
        let event = match navigation {
            PreviewNavigationEvent::SelectNode { .. } => SourceNavigationEvent::SelectSource {
                source_range: match range(
                    &entry.source_range,
                    &source_frame.source,
                    source_frame.position_encoding,
                ) {
                    Ok(range) => range,
                    Err(error) => return id.map(|id| boundary_error(id, error)),
                },
            },
            PreviewNavigationEvent::RevealNode { .. } => SourceNavigationEvent::RevealSource {
                source_range: match range(
                    &entry.source_range,
                    &source_frame.source,
                    source_frame.position_encoding,
                ) {
                    Ok(range) => range,
                    Err(error) => return id.map(|id| boundary_error(id, error)),
                },
            },
        };
        self.outgoing_events
            .push(preview_event_notification(ServerPreviewEventParams {
                daemon_instance_id: params.daemon_instance_id,
                preview_session_id: params.preview_session_id,
                render_revision: match counter(state.delivered_revision) {
                    Ok(revision) => revision,
                    Err(error) => return id.map(|id| boundary_error(id, error)),
                },
                event: ServerPreviewEvent::SourceNavigation(event),
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
        if let Err(error) = self.verify_daemon(&params.daemon_instance_id) {
            return Some(session_error(id, error));
        }
        if !self
            .registry
            .workspace_statuses()
            .iter()
            .any(|status| status.uri == params.workspace_uri)
        {
            return Some(response_value(Response::error(
                id,
                fleximark_protocol::error_code::INVALID_PARAMS,
                "workspace is not registered",
            )));
        }
        if let Err(error) = self.registry.reconfigure_native_workspace(
            &params.workspace_uri,
            params.trusted,
            &self.work_cancellation,
        ) {
            return Some(session_error(id, error));
        }

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
            let version = match self.registry.current_version(&state.document_session_id) {
                Ok(version) => version,
                Err(error) => return Some(session_error(id, error)),
            };
            let frame = match self.registry.render_with_cancellation(
                &state.document_session_id,
                version,
                &preview_id,
                &self.work_cancellation,
            ) {
                Ok(frame) => frame,
                Err(error) => return Some(session_error(id, error)),
            };
            match self.publish_frame(&preview_id, &frame) {
                Ok(true) => {}
                Ok(false) => return None,
                Err(error) => return Some(boundary_error(id, error)),
            }
        }
        Some(response_value(Response::success(id, Value::Null)))
    }

    pub(super) fn workspace_for_document(&self, document_uri: &str) -> Option<(String, bool)> {
        self.registry
            .workspace_for_document(document_uri)
            .map(|membership| (membership.uri, membership.trusted))
    }
}
impl Server {
    pub(super) fn mark_preview_document_version(
        &self,
        document_session_id: &str,
        document_version: JsSafeU64,
    ) {
        for state in self.preview_states.values() {
            if state.document_session_id == document_session_id {
                if let Some(token) = &state.token {
                    self.previews.mark_document_version(token, document_version);
                }
            }
        }
    }
}
