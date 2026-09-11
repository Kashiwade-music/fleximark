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
fn no_arguments_preserves_usage_stderr_and_failure_exit() {
    let output = run(&[]);
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        text(&output.stderr),
        "fleximark: usage: fleximark <render|benchmark|init|edit-theme|create-note|collect-admonitions|export|ack-export> [path] [destination]\n"
    );
}

#[test]
fn unknown_command_preserves_stderr_and_failure_exit() {
    let output = run(&["unknown-command"]);
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        text(&output.stderr),
        "fleximark: unknown command: unknown-command\n"
    );
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
