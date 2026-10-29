//! Role: Deliver LSP and RPC requests and session events over the daemon transport.
use std::collections::VecDeque;
use std::io::{self, BufReader, BufWriter};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread;

use fleximark_protocol::{IncomingMessage, Response, read_frame, write_frame};
use serde_json::Value;

use crate::cancellation::{CancellationCoordinator, WorkPermit};
use crate::server::{Server, response_value};
use crate::telemetry::log_operational_failure;

pub fn run(
    assets: crate::preview_http::BrowserAssets,
    resources: Arc<fleximark_preview_assets::PreviewAssets>,
) -> Result<(), Box<dyn std::error::Error>> {
    let output = CoalescingOutput::default();
    let writer_output = output.clone();
    thread::spawn(move || {
        let mut writer = BufWriter::new(io::stdout());
        loop {
            let message = writer_output.pop();
            if let Err(error) = write_frame(&mut writer, &message) {
                log_operational_failure("stdout-write-failed", &error, None, None, None);
                break;
            }
        }
    });
    let (browser_outgoing, browser_incoming) = mpsc::channel::<Value>();
    let coordinator = Arc::new(CancellationCoordinator::default());
    let input_coordinator = Arc::clone(&coordinator);
    let (input_sender, input_receiver) = mpsc::channel::<InputEvent>();
    let reader_sender = input_sender.clone();
    thread::spawn(move || {
        let input_sender = reader_sender;
        let stdin = io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        loop {
            match read_frame(&mut reader) {
                Ok(Some(body)) => {
                    if let Ok(response) = serde_json::from_slice::<Value>(&body) {
                        if response.get("method").is_none()
                            && response.get("id").is_some()
                            && (response.get("result").is_some() || response.get("error").is_some())
                        {
                            if input_sender.send(InputEvent::Response(response)).is_err() {
                                break;
                            }
                            continue;
                        }
                    }
                    let message = match serde_json::from_slice::<IncomingMessage>(&body) {
                        Ok(message) if message.jsonrpc == "2.0" => message,
                        _ => {
                            if input_sender.send(InputEvent::Invalid).is_err() {
                                break;
                            }
                            continue;
                        }
                    };
                    let permit = input_coordinator.prepare(&message);
                    if input_sender
                        .send(InputEvent::Message { message, permit })
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(None) => {
                    let _ = input_sender.send(InputEvent::Closed);
                    break;
                }
                Err(error) => {
                    let _ = input_sender.send(InputEvent::Failed(error.to_string()));
                    break;
                }
            }
        }
    });
    let browser_sender = input_sender.clone();
    thread::spawn(move || {
        for message in browser_incoming {
            if browser_sender.send(InputEvent::Browser(message)).is_err() {
                break;
            }
        }
    });
    let mut server = Server::with_assets(Some(browser_outgoing), assets)?;
    server.set_preview_assets(resources);
    server.set_workspace_handler(Arc::new(move |request| {
        let (reply, response) = mpsc::channel();
        input_sender
            .send(InputEvent::Workspace { request, reply })
            .map_err(fleximark_application::ApplicationError::internal)?;
        response
            .recv_timeout(std::time::Duration::from_secs(1))
            .map_err(fleximark_application::ApplicationError::internal)?
    }));

    loop {
        let (message, permit) = match input_receiver.recv()? {
            InputEvent::Message { message, permit } => (message, permit),
            InputEvent::Browser(message) => {
                for event in server.browser_notification(message) {
                    output.push(event);
                }
                continue;
            }
            InputEvent::Response(response) => {
                server.client_response(&response);
                continue;
            }
            InputEvent::Workspace { request, reply } => {
                let response = server.workspace_request(request);
                for event in server.take_outgoing() {
                    output.push(event);
                }
                let _ = reply.send(response);
                continue;
            }
            InputEvent::Invalid => {
                output.push(response_value(Response::error(
                    Value::Null,
                    fleximark_protocol::error_code::PARSE_ERROR,
                    "invalid JSON-RPC message",
                )));
                continue;
            }
            InputEvent::Closed => break,
            InputEvent::Failed(error) => return Err(error.into()),
        };
        process_message(&mut server, &coordinator, &output, message, permit)?;
        if server.exit {
            break;
        }
    }
    Ok(())
}

#[derive(Clone, Default)]
struct CoalescingOutput {
    state: Arc<(Mutex<VecDeque<Value>>, Condvar)>,
}

impl CoalescingOutput {
    fn push(&self, message: Value) {
        let (queue, ready) = &*self.state;
        let mut queue = queue.lock().expect("output queue lock");
        if let Some(preview_id) = preview_changed_key(&message).map(str::to_owned) {
            if let Some(pending) = queue
                .iter_mut()
                .find(|pending| preview_changed_key(pending) == Some(preview_id.as_str()))
            {
                *pending = message;
                ready.notify_one();
                return;
            }
        }
        queue.push_back(message);
        ready.notify_one();
    }

    fn pop(&self) -> Value {
        let (queue, ready) = &*self.state;
        let mut queue = queue.lock().expect("output queue lock");
        loop {
            if let Some(message) = queue.pop_front() {
                return message;
            }
            queue = ready.wait(queue).expect("output queue wait");
        }
    }

    #[cfg(test)]
    fn drain(&self) -> Vec<Value> {
        self.state
            .0
            .lock()
            .expect("output queue lock")
            .drain(..)
            .collect()
    }
}

fn preview_changed_key(message: &Value) -> Option<&str> {
    (message.get("method")?.as_str()? == fleximark_protocol::method::PREVIEW_CHANGED)
        .then(|| {
            message
                .pointer("/params/previewSessionId")
                .and_then(Value::as_str)
        })
        .flatten()
}

trait OutputSink {
    fn send_output(&self, message: Value) -> Result<(), mpsc::SendError<Value>>;
}

impl OutputSink for CoalescingOutput {
    fn send_output(&self, message: Value) -> Result<(), mpsc::SendError<Value>> {
        self.push(message);
        Ok(())
    }
}

impl OutputSink for mpsc::Sender<Value> {
    fn send_output(&self, message: Value) -> Result<(), mpsc::SendError<Value>> {
        self.send(message)
    }
}

fn process_message(
    server: &mut Server,
    coordinator: &CancellationCoordinator,
    outgoing: &impl OutputSink,
    message: IncomingMessage,
    permit: Option<WorkPermit>,
) -> Result<(), mpsc::SendError<Value>> {
    process_message_with_observer(server, coordinator, outgoing, message, permit, |_| {})
}

fn process_message_with_observer(
    server: &mut Server,
    coordinator: &CancellationCoordinator,
    outgoing: &impl OutputSink,
    message: IncomingMessage,
    permit: Option<WorkPermit>,
    mut observer: impl FnMut(&'static str),
) -> Result<(), mpsc::SendError<Value>> {
    let cancelled_before_start = permit
        .as_ref()
        .is_some_and(|permit| !coordinator.should_execute(permit));
    let mut messages = if cancelled_before_start {
        Vec::new()
    } else {
        let messages = server.handle_cancellable(
            message.clone(),
            permit.as_ref().map(|p| p.token.clone()),
            permit.as_ref().map(|p| p.publication_token.clone()),
        );
        observer("handled");
        messages
    };
    server.bind_document_alias(&message, coordinator);
    observer("bound");
    let cancelled = permit
        .as_ref()
        .is_some_and(|permit| !coordinator.is_current(permit));
    observer("checked");
    if cancelled {
        messages.clear();
        if let Some(id) = message.id.clone() {
            messages.push(response_value(Response::error(
                id.into(),
                fleximark_protocol::error_code::REQUEST_CANCELLED,
                "request cancelled",
            )));
        }
    }
    if let Some(permit) = permit {
        coordinator.finish(&permit);
        observer("finished");
    }
    for outgoing_message in messages {
        outgoing.send_output(outgoing_message)?;
        observer("sent");
    }
    Ok(())
}

enum InputEvent {
    Browser(Value),
    Response(Value),
    Workspace {
        request: crate::preview_http::WorkspaceRequest,
        reply: mpsc::Sender<
            Result<crate::preview_http::WorkspaceResponse, fleximark_application::ApplicationError>,
        >,
    },
    Message {
        message: IncomingMessage,
        permit: Option<WorkPermit>,
    },
    Invalid,
    Closed,
    Failed(String),
}

#[cfg(test)]
mod tests {
    //! Role: Verify stdio backpressure and cancellation publication gates.
    use super::*;
    use fleximark_protocol::{RpcId, method};
    use serde_json::json;

    fn message(id: Option<i64>, method: &str, params: Value) -> IncomingMessage {
        IncomingMessage {
            jsonrpc: "2.0".into(),
            id: id.map(|id| RpcId::try_from(id).unwrap()),
            method: method.into(),
            params,
        }
    }

    #[test]
    fn slow_output_coalesces_preview_changes_without_reordering_other_messages() {
        let output = CoalescingOutput::default();
        output.push(json!({"jsonrpc":"2.0","id":1,"result":null}));
        for revision in 1..=1_000 {
            output.push(json!({
                "jsonrpc":"2.0",
                "method":method::PREVIEW_CHANGED,
                "params":{
                    "daemonInstanceId":"daemon",
                    "previewSessionId":"preview-a",
                    "renderRevision":revision
                }
            }));
        }
        output.push(json!({"jsonrpc":"2.0","id":2,"result":null}));
        output.push(json!({
            "jsonrpc":"2.0",
            "method":method::PREVIEW_CHANGED,
            "params":{
                "daemonInstanceId":"daemon",
                "previewSessionId":"preview-b",
                "renderRevision":4
            }
        }));

        let queued = output.drain();
        assert_eq!(queued.len(), 4);
        assert_eq!(queued[0]["id"], 1);
        assert_eq!(queued[1]["params"]["previewSessionId"], "preview-a");
        assert_eq!(queued[1]["params"]["renderRevision"], 1_000);
        assert_eq!(queued[2]["id"], 2);
        assert_eq!(queued[3]["params"]["previewSessionId"], "preview-b");
    }

    #[test]
    fn suppresses_cancelled_and_obsolete_work_before_publication() {
        let coordinator = CancellationCoordinator::default();
        let (outgoing, incoming) = mpsc::channel();
        let mut server = Server::new(crate::test_services::assets()).unwrap();

        let cancelled_request = message(Some(80), "unknown/request", json!({}));
        let cancelled_permit = coordinator.prepare(&cancelled_request).unwrap();
        assert!(
            coordinator
                .prepare(&message(None, "$/cancelRequest", json!({"id":80})))
                .is_none()
        );
        let mut cancelled_events = Vec::new();
        process_message_with_observer(
            &mut server,
            &coordinator,
            &outgoing,
            cancelled_request,
            Some(cancelled_permit),
            |event| cancelled_events.push(event),
        )
        .unwrap();
        assert_eq!(cancelled_events, ["bound", "checked", "finished", "sent"]);
        assert_eq!(
            incoming.recv().unwrap(),
            json!({
                "jsonrpc":"2.0",
                "id":80,
                "error":{"code":fleximark_protocol::error_code::REQUEST_CANCELLED,"message":"request cancelled"}
            })
        );

        let obsolete_request = message(
            Some(81),
            "textDocument/didChange",
            json!({"documentSessionId":"session-obsolete"}),
        );
        let obsolete_permit = coordinator.prepare(&obsolete_request).unwrap();
        let mut newer_permit = None;
        let mut obsolete_events = Vec::new();
        process_message_with_observer(
            &mut server,
            &coordinator,
            &outgoing,
            obsolete_request,
            Some(obsolete_permit),
            |event| {
                obsolete_events.push(event);
                if event == "handled" {
                    newer_permit = coordinator.prepare(&message(
                        None,
                        "textDocument/didChange",
                        json!({"documentSessionId":"session-obsolete"}),
                    ));
                }
            },
        )
        .unwrap();
        assert_eq!(
            obsolete_events,
            ["handled", "bound", "checked", "finished", "sent"]
        );
        assert_eq!(
            incoming.recv().unwrap(),
            json!({
                "jsonrpc":"2.0",
                "id":81,
                "error":{"code":fleximark_protocol::error_code::REQUEST_CANCELLED,"message":"request cancelled"}
            })
        );
        coordinator.finish(&newer_permit.unwrap());
    }

    #[test]
    fn obsolete_lsp_change_applies_mutation_but_suppresses_all_publication() {
        let coordinator = CancellationCoordinator::default();
        let (outgoing, incoming) = mpsc::channel();
        let mut server = Server::new(crate::test_services::assets()).unwrap();
        let initialized =
            server.handle(message(Some(11), "initialize", json!({"capabilities":{}})));
        let daemon_id = initialized[0]["result"]["capabilities"]["experimental"]["fleximark"]["daemonInstanceId"]
            .as_str()
            .unwrap()
            .to_owned();
        let uri = "file:///transport-obsolete-lsp.md";
        server.handle(message(
            None,
            "textDocument/didOpen",
            json!({
                "textDocument":{"uri":uri,"version":1,"text":"one\n"}
            }),
        ));
        let session_id = server.registry.session_id_for_uri(uri).unwrap().to_owned();
        let preview = server.handle(message(
            Some(12),
            method::CREATE_PREVIEW,
            json!({
                "daemonInstanceId":daemon_id.clone(),
                "documentSessionId":session_id.clone(),
                "expectedDocumentVersion":1,
                "target":"externalBrowser"
            }),
        ));
        let preview_id = preview[0]["result"]["previewSessionId"].as_str().unwrap();
        let token = server.preview_states[preview_id]
            .token
            .as_deref()
            .unwrap()
            .to_owned();
        let initial_revision = server.previews.pages.lock().unwrap()[&token]
            .frame
            .render_revision;

        let change_two = message(
            None,
            "textDocument/didChange",
            json!({
                "textDocument":{"uri":uri,"version":2},
                "contentChanges":[{"text":"two\n"}]
            }),
        );
        let change_two_permit = coordinator.prepare(&change_two).unwrap();
        assert!(coordinator.should_execute(&change_two_permit));
        let change_three = message(
            None,
            "textDocument/didChange",
            json!({
                "textDocument":{"uri":uri,"version":3},
                "contentChanges":[{"text":"three\n"}]
            }),
        );
        let change_three_permit = coordinator.prepare(&change_three).unwrap();
        assert!(
            coordinator.should_execute(&change_two_permit),
            "an older mutation still applies even after its publication becomes obsolete"
        );
        assert!(!coordinator.is_current(&change_two_permit));

        process_message(
            &mut server,
            &coordinator,
            &outgoing,
            change_two,
            Some(change_two_permit),
        )
        .unwrap();

        let document = server.registry.document(&session_id, 2).unwrap();
        assert_eq!(document.source(), "two\n");
        assert_eq!(
            server.previews.pages.lock().unwrap()[&token]
                .frame
                .render_revision,
            initial_revision,
            "cancelled LSP publication must not replace the latest HTTP frame"
        );
        assert!(
            matches!(incoming.try_recv(), Err(mpsc::TryRecvError::Empty)),
            "cancelled notification must emit no preview event, diagnostic, or response"
        );
        coordinator.finish(&change_three_permit);
    }
}
