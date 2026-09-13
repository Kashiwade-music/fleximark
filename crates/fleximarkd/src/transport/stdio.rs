use std::io::{self, BufReader, BufWriter};
use std::sync::{Arc, mpsc};
use std::thread;

use fleximark_protocol::{IncomingMessage, Response, read_frame, write_frame};
use serde_json::Value;

use crate::cancellation::{CancellationCoordinator, WorkPermit};
use crate::server::{Server, response_value};
use crate::telemetry::log_operational_event;

pub(crate) fn run(mode: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (outgoing, incoming) = mpsc::channel::<Value>();
    thread::spawn(move || {
        let mut writer = BufWriter::new(io::stdout());
        for message in incoming {
            if let Err(error) = write_frame(&mut writer, &message) {
                let _ = error;
                log_operational_event("stdout-write-failed", None, None, None);
                break;
            }
        }
    });
    let coordinator = Arc::new(CancellationCoordinator::default());
    let input_coordinator = Arc::clone(&coordinator);
    let (input_sender, input_receiver) = mpsc::channel::<InputEvent>();
    thread::spawn(move || {
        let stdin = io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        loop {
            match read_frame(&mut reader) {
                Ok(Some(body)) => {
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
    let mut server = Server::with_sender(mode == "lsp", outgoing.clone());

    loop {
        let (message, permit) = match input_receiver.recv()? {
            InputEvent::Message { message, permit } => (message, permit),
            InputEvent::Invalid => {
                outgoing.send(response_value(Response::error(
                    Value::Null,
                    -32700,
                    "invalid JSON-RPC message",
                )))?;
                continue;
            }
            InputEvent::Closed => break,
            InputEvent::Failed(error) => return Err(error.into()),
        };
        process_message(&mut server, &coordinator, &outgoing, message, permit)?;
        if server.exit {
            break;
        }
    }
    Ok(())
}

fn process_message(
    server: &mut Server,
    coordinator: &CancellationCoordinator,
    outgoing: &mpsc::Sender<Value>,
    message: IncomingMessage,
    permit: Option<WorkPermit>,
) -> Result<(), mpsc::SendError<Value>> {
    process_message_with_observer(server, coordinator, outgoing, message, permit, |_| {})
}

fn process_message_with_observer(
    server: &mut Server,
    coordinator: &CancellationCoordinator,
    outgoing: &mpsc::Sender<Value>,
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
                -32800,
                "request cancelled",
            )));
        }
    }
    if let Some(permit) = permit {
        coordinator.finish(&permit);
        observer("finished");
    }
    for outgoing_message in messages {
        outgoing.send(outgoing_message)?;
        observer("sent");
    }
    Ok(())
}

enum InputEvent {
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
    use super::*;
    use fleximark_lsp::content_hash;
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
    fn suppresses_cancelled_and_obsolete_work_before_publication() {
        let coordinator = CancellationCoordinator::default();
        let (outgoing, incoming) = mpsc::channel();
        let mut server = Server::new(false);

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
                "error":{"code":-32800,"message":"request cancelled"}
            })
        );

        let obsolete_request = message(
            Some(81),
            method::CHANGE_DOCUMENT,
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
                        method::CHANGE_DOCUMENT,
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
                "error":{"code":-32800,"message":"request cancelled"}
            })
        );
        coordinator.finish(&newer_permit.unwrap());
    }

    #[test]
    fn valid_rpc_change_is_applied_but_obsolete_response_is_cancelled() {
        let coordinator = CancellationCoordinator::default();
        let (outgoing, incoming) = mpsc::channel();
        let mut server = Server::new(false);
        let initialized = server.handle(message(
            Some(1),
            method::INITIALIZE,
            json!({
                "protocolVersion":1,
                "client":{"name":"test","version":"1"},
                "capabilities":{}
            }),
        ));
        let daemon_id = initialized[0]["result"]["daemonInstanceId"]
            .as_str()
            .unwrap()
            .to_owned();
        let opened = server.handle(message(
            Some(2),
            method::OPEN_DOCUMENT,
            json!({
                "daemonInstanceId":daemon_id.clone(),
                "uri":"file:///transport-obsolete.md",
                "documentVersion":1,
                "text":"one\n"
            }),
        ));
        let session_id = opened[0]["result"]["documentSessionId"]
            .as_str()
            .unwrap()
            .to_owned();
        let change_two = message(
            Some(4),
            method::CHANGE_DOCUMENT,
            json!({
                "daemonInstanceId":daemon_id.clone(),
                "documentSessionId":session_id.clone(),
                "baseDocumentVersion":1,
                "baseContentHash":content_hash("one\n"),
                "documentVersion":2,
                "text":"two\n"
            }),
        );
        let change_two_permit = coordinator.prepare(&change_two).unwrap();
        let mut change_three_permit = None;
        process_message_with_observer(
            &mut server,
            &coordinator,
            &outgoing,
            change_two,
            Some(change_two_permit),
            |event| {
                if event == "handled" {
                    change_three_permit = coordinator.prepare(&message(
                        Some(5),
                        method::CHANGE_DOCUMENT,
                        json!({
                            "daemonInstanceId":daemon_id.clone(),
                            "documentSessionId":session_id.clone(),
                            "baseDocumentVersion":2,
                            "baseContentHash":content_hash("two\n"),
                            "documentVersion":3,
                            "text":"three\n"
                        }),
                    ));
                }
            },
        )
        .unwrap();

        let document = server
            .registry
            .document(&daemon_id, &session_id, 2)
            .unwrap();
        assert_eq!(document.source(), "two\n");
        assert_eq!(
            incoming.recv().unwrap(),
            json!({
                "jsonrpc":"2.0",
                "id":4,
                "error":{"code":-32800,"message":"request cancelled"}
            })
        );
        assert!(
            incoming.try_recv().is_err(),
            "obsolete preview or diagnostic publications escaped the transport gate"
        );
        coordinator.finish(&change_three_permit.unwrap());
    }

    #[test]
    fn obsolete_lsp_change_applies_mutation_but_suppresses_all_publication() {
        let coordinator = CancellationCoordinator::default();
        let (outgoing, incoming) = mpsc::channel();
        let mut server = Server::new(true);
        assert_eq!(
            server
                .handle(message(Some(10), "initialize", json!({"capabilities":{}})))
                .len(),
            1
        );
        let initialized = server.handle(message(
            Some(11),
            method::INITIALIZE,
            json!({
                "protocolVersion":1,
                "client":{"name":"test","version":"1"},
                "capabilities":{}
            }),
        ));
        let daemon_id = initialized[0]["result"]["daemonInstanceId"]
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
        let initial_publications = server.previews.pages.lock().unwrap()[&token]
            .publications
            .len();

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

        let document = server
            .registry
            .document(&daemon_id, &session_id, 2)
            .unwrap();
        assert_eq!(document.source(), "two\n");
        assert_eq!(
            server.previews.pages.lock().unwrap()[&token]
                .publications
                .len(),
            initial_publications,
            "cancelled LSP publication must not append HTTP preview history"
        );
        assert!(
            matches!(incoming.try_recv(), Err(mpsc::TryRecvError::Empty)),
            "cancelled notification must emit no preview event, diagnostic, or response"
        );
        coordinator.finish(&change_three_permit);
    }
}
