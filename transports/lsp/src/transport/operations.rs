//! Role: Run owned workspace jobs while the actor retains replies and permissions.
use crate::cancellation::{CancellationCoordinator, WorkPermit};
use crate::preview_http::{WorkspaceRequest, WorkspaceResponse};
use crate::server::{
    CommandCompletion, CommandProgress, PreparedCommand, Server, operation_error, response_value,
};
use fleximark_application::ApplicationError;
use fleximark_protocol::{IncomingMessage, Response, WorkspaceCommandRequest};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, mpsc};
pub(super) type WorkspaceReply = mpsc::Sender<Result<WorkspaceResponse, ApplicationError>>;
pub(super) enum InputEvent {
    Browser(Value),
    Response(Value),
    Workspace {
        request: WorkspaceRequest,
        reply: WorkspaceReply,
    },
    Message {
        message: IncomingMessage,
        permit: Option<WorkPermit>,
    },
    OperationFinished {
        operation_id: u64,
        completion: Box<CommandCompletion>,
    },
    Invalid,
    Closed,
    Failed(String),
}
enum PendingReply {
    Lsp {
        id: Value,
        standard: bool,
        permit: Option<WorkPermit>,
    },
    Http {
        reply: WorkspaceReply,
        cancellation: fleximark_application::CancellationToken,
    },
}
pub(super) struct ActorOperations {
    next_id: u64,
    pending: HashMap<u64, PendingReply>,
    workspace_locks: HashMap<String, Arc<Mutex<()>>>,
    sender: mpsc::Sender<InputEvent>,
    coordinator: Arc<CancellationCoordinator>,
    #[cfg(test)]
    pub(super) before_execute: Option<Arc<dyn Fn() + Send + Sync>>,
    #[cfg(test)]
    pub(super) after_execute: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl ActorOperations {
    pub(super) fn new(
        sender: mpsc::Sender<InputEvent>,
        coordinator: Arc<CancellationCoordinator>,
    ) -> Self {
        Self {
            next_id: 0,
            pending: HashMap::new(),
            workspace_locks: HashMap::new(),
            sender,
            coordinator,
            #[cfg(test)]
            before_execute: None,
            #[cfg(test)]
            after_execute: None,
        }
    }
    fn start(
        &mut self,
        prepared: PreparedCommand,
        reply: PendingReply,
    ) -> Result<(), ApplicationError> {
        let operation_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| ApplicationError::InternalState("operation counter exhausted".into()))?;
        self.next_id = operation_id;
        self.pending.insert(operation_id, reply);
        if let Err(error) = self.dispatch(operation_id, prepared) {
            if let Some(PendingReply::Lsp {
                permit: Some(permit),
                ..
            }) = self.pending.remove(&operation_id)
            {
                self.coordinator.finish(&permit);
            }
            return Err(error);
        }
        Ok(())
    }
    fn dispatch(
        &mut self,
        operation_id: u64,
        prepared: PreparedCommand,
    ) -> Result<(), ApplicationError> {
        let lock = prepared
            .workspace_uri()
            .map(|uri| Arc::clone(self.workspace_locks.entry(uri.to_owned()).or_default()));
        let sender = self.sender.clone();
        #[cfg(test)]
        let before_execute = self.before_execute.clone();
        #[cfg(test)]
        let after_execute = self.after_execute.clone();
        let spawned = std::thread::Builder::new()
            .name("workspace-operation".into())
            .spawn(move || {
                let _guard = lock.as_ref().map(|lock| {
                    lock.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                });
                #[cfg(test)]
                if let Some(hook) = before_execute {
                    hook();
                }
                let completion = prepared.execute();
                #[cfg(test)]
                if let Some(hook) = after_execute {
                    hook();
                }
                let _ = sender.send(InputEvent::OperationFinished {
                    operation_id,
                    completion: Box::new(completion),
                });
            });
        spawned.map_err(ApplicationError::Io)?;
        Ok(())
    }
    pub(super) fn start_lsp(
        &mut self,
        server: &mut Server,
        message: IncomingMessage,
        permit: Option<WorkPermit>,
    ) -> Vec<Value> {
        let Some(id) = message.id.map(Value::from) else {
            if let Some(permit) = permit {
                self.coordinator.finish(&permit);
            }
            return vec![];
        };
        let standard = message.method == "workspace/executeCommand";
        let request = if standard {
            server.decode_standard_command(message.params)
        } else {
            serde_json::from_value::<WorkspaceCommandRequest>(message.params)
                .map_err(|error| ApplicationError::InvalidState(error.to_string()))
        };
        let cancellation = permit
            .as_ref()
            .map(|permit| permit.token.clone())
            .unwrap_or_default();
        let prepared = request
            .and_then(|request| server.prepare_background_command(request, false, cancellation));
        match prepared {
            Ok(prepared) => match self.start(
                prepared,
                PendingReply::Lsp {
                    id: id.clone(),
                    standard,
                    permit,
                },
            ) {
                Ok(()) => vec![],
                Err(error) => vec![operation_error(id, error)],
            },
            Err(error) => {
                if let Some(permit) = permit {
                    self.coordinator.finish(&permit);
                }
                vec![operation_error(id, error)]
            }
        }
    }
    pub(super) fn start_http(
        &mut self,
        server: &mut Server,
        request: fleximark_protocol::WorkspaceUiCommandRequest,
        reply: WorkspaceReply,
    ) {
        let cancellation = fleximark_application::CancellationToken::default();
        let prepared = server
            .resolve_ui_context(request.context)
            .and_then(|context| {
                server.prepare_background_command(
                    WorkspaceCommandRequest {
                        daemon_instance_id: server.daemon_instance_id().to_owned(),
                        context,
                        operation: request.operation,
                    },
                    true,
                    cancellation.clone(),
                )
            });
        match prepared {
            Ok(prepared) => {
                let failure_reply = reply.clone();
                if let Err(error) = self.start(
                    prepared,
                    PendingReply::Http {
                        reply,
                        cancellation,
                    },
                ) {
                    let _ = failure_reply.send(Err(error));
                }
            }
            Err(error) => {
                let _ = reply.send(Err(error));
            }
        }
    }
    pub(super) fn finish(
        &mut self,
        server: &mut Server,
        operation_id: u64,
        completion: CommandCompletion,
    ) -> Vec<Value> {
        if !self.pending.contains_key(&operation_id) {
            return vec![];
        }
        let result = match server.complete_command(completion) {
            Ok(CommandProgress::Continue(prepared)) => match self.dispatch(operation_id, *prepared)
            {
                Ok(()) => return server.take_outgoing(),
                Err(error) => Err(error),
            },
            Ok(CommandProgress::Complete(result)) => Ok(*result),
            Err(error) => Err(error),
        };
        let Some(reply) = self.pending.remove(&operation_id) else {
            return vec![];
        };
        let mut messages = match reply {
            PendingReply::Lsp {
                id,
                standard,
                permit,
            } => {
                if let Some(permit) = permit {
                    self.coordinator.finish(&permit);
                }
                let result = if standard {
                    result.and_then(|result| server.finish_standard_command(result))
                } else {
                    result
                };
                vec![match result {
                    Ok(result) => response_value(Response::success(id, result)),
                    Err(error) => operation_error(id, error),
                }]
            }
            PendingReply::Http { reply, .. } => {
                let _ = reply.send(result.map(WorkspaceResponse::Command));
                vec![]
            }
        };
        messages.extend(server.take_outgoing());
        messages
    }
    pub(super) fn shutdown(&mut self) -> Vec<Value> {
        self.pending
            .drain()
            .filter_map(|(_, reply)| match reply {
                PendingReply::Lsp { id, permit, .. } => {
                    if let Some(permit) = permit {
                        permit.token.cancel();
                        self.coordinator.finish(&permit);
                    }
                    Some(operation_error(
                        id,
                        ApplicationError::InvalidState("server is closed".into()),
                    ))
                }
                PendingReply::Http { cancellation, .. } => {
                    cancellation.cancel();
                    None
                }
            })
            .collect()
    }
}

impl Drop for ActorOperations {
    fn drop(&mut self) {
        self.shutdown();
    }
}
