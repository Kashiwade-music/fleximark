//! Role: Verify stdio protocol framing rejects unsafe request identifiers.
mod support;
use std::io::{BufReader, Cursor, Write};
use std::process::{Command, Stdio};

use fleximark_protocol::{MAX_SAFE_INTEGER, read_frame, write_frame};
use serde_json::{Value, json};

#[test]
fn unsafe_integer_id_is_rejected_without_being_echoed() {
    let resources = support::installed_resources();
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_fleximarkd"))
        .arg("lsp")
        .env("FLEXIMARK_RUNTIME_ROOT", &*resources)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start fleximarkd rpc transport");

    let burst_ids = (0..128)
        .map(|index| format!("queued-{index}-{}", "x".repeat(4096)))
        .collect::<Vec<_>>();
    {
        let stdin = daemon.stdin.as_mut().expect("daemon stdin");
        write_frame(stdin, &json!({"jsonrpc":"2.0","id":"initialize","method":"initialize","params":{"capabilities":{}}})).unwrap();
        write_frame(
            stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": MAX_SAFE_INTEGER + 1,
                "method": "unknown/request",
                "params": {}
            }),
        )
        .unwrap();
        write_frame(
            stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": "safe-request",
                "method": "unknown/request",
                "params": {}
            }),
        )
        .unwrap();
        for id in &burst_ids {
            write_frame(
                stdin,
                &json!({"jsonrpc":"2.0","id":id,
                "method":"unknown/request","params":{}}),
            )
            .unwrap();
        }
        stdin.flush().unwrap();
    }
    drop(daemon.stdin.take());

    let output = daemon.wait_with_output().expect("wait for fleximarkd");
    assert!(
        output.status.success(),
        "daemon failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mut reader = BufReader::new(Cursor::new(output.stdout));
    let mut responses = Vec::new();
    while let Some(body) = read_frame(&mut reader).unwrap() {
        responses.push(serde_json::from_slice::<Value>(&body).unwrap());
    }
    let initialized = responses.remove(0);
    assert_eq!(initialized["id"], "initialize");
    assert!(initialized.get("result").is_some());
    let mut expected = vec![
        json!({
            "jsonrpc": "2.0",
            "id": null,
            "error": { "code": fleximark_protocol::error_code::PARSE_ERROR, "message": "invalid JSON-RPC message" }
        }),
        json!({
            "jsonrpc": "2.0",
            "id": "safe-request",
            "error": { "code": fleximark_protocol::error_code::METHOD_NOT_FOUND, "message": "method not found" }
        }),
    ];
    expected.extend(burst_ids.iter().map(|id| json!({
        "jsonrpc":"2.0","id":id,
        "error":{"code":fleximark_protocol::error_code::METHOD_NOT_FOUND,"message":"method not found"}
    })));
    assert_eq!(responses, expected);
    assert!(
        responses
            .iter()
            .all(|response| response["id"] != json!(MAX_SAFE_INTEGER + 1))
    );
}
