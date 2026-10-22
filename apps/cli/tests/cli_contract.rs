//! Role: Verify CLI command behavior and filesystem contracts.
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
            "fleximark: usage: fleximark <render|benchmark|init|edit-theme|create-note|collect-admonitions|export|ack-export|migrate> [path] [destination]; migrate [workspace] [--dry-run] [--settings file]\n",
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

#[test]
fn migration_dry_run_is_explicit_and_apply_preserves_legacy_inputs() {
    let root = temporary_markdown_path().with_extension("workspace");
    let control = root.join(".fleximark");
    fs::create_dir_all(&control).unwrap();
    fs::write(
        control.join("fleximark.json"),
        r#"{"noteFileNamePrefix":"daily-"}"#,
    )
    .unwrap();
    fs::write(control.join("parserPlugin.js"), "throw 'never execute';").unwrap();
    let root_text = root.to_str().unwrap();
    let dry = run(&["migrate", root_text, "--dry-run"]);
    assert!(dry.status.success(), "{}", text(&dry.stderr));
    let plan: serde_json::Value = serde_json::from_slice(&dry.stdout).unwrap();
    assert_eq!(plan["status"], "ready");
    assert!(text(&dry.stdout).contains("Unsupported JavaScript"));
    assert!(!control.join("config.toml").exists());
    let apply = run(&["migrate", root_text]);
    assert!(apply.status.success(), "{}", text(&apply.stderr));
    assert!(control.join("config.toml").is_file());
    assert_eq!(
        fs::read_to_string(control.join("parserPlugin.js")).unwrap(),
        "throw 'never execute';"
    );
    assert!(fs::read_dir(&root).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".fleximark-backup-")
    }));
    let again = run(&["migrate", root_text, "--dry-run"]);
    assert!(again.status.success());
    let plan: serde_json::Value = serde_json::from_slice(&again.stdout).unwrap();
    assert_eq!(plan["status"], "alreadyCurrent");
    fs::remove_dir_all(root).unwrap();
}
