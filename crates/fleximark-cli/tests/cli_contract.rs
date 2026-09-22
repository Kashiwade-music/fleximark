use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fleximark"))
        .args(arguments)
        .output()
        .expect("CLI starts")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("CLI output is UTF-8")
}

#[test]
fn invalid_invocations_preserve_stderr_and_failure_exit() {
    for (arguments, expected) in [
        (
            &[][..],
            "fleximark: usage: fleximark <render|benchmark|init|edit-theme|create-note|collect-admonitions|export|ack-export> [path] [destination]\n",
        ),
        (
            &["unknown-command"][..],
            "fleximark: unknown command: unknown-command\n",
        ),
    ] {
        let output = run(arguments);
        assert!(!output.status.success(), "{arguments:?}");
        assert_eq!(output.status.code(), Some(1), "{arguments:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}");
        assert_eq!(text(&output.stderr), expected, "{arguments:?}");
    }
}

#[test]
fn render_writes_html_to_stdout_without_stderr() {
    let path = temporary_markdown_path();
    fs::write(&path, "# CLI contract\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_fleximark"))
        .arg("render")
        .arg(&path)
        .output()
        .expect("CLI starts");
    fs::remove_file(&path).unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = text(&output.stdout);
    assert!(stdout.starts_with("<main data-fleximark-node-id=\"document-root\">"));
    assert!(stdout.contains("CLI contract"));
    assert!(stdout.ends_with("</main>"));
    assert!(!stdout.ends_with('\n'));
}

fn temporary_markdown_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "fleximark-cli-contract-{}-{}.md",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
