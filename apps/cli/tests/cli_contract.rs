//! Role: Verify CLI command behavior and filesystem contracts.
use fleximark_test_support::TempWorkspace;
use std::fs;
use std::process::{Command, Output};

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
            "fleximark: usage: fleximark <render|init|edit-theme|create-note|collect-admonitions|export|ack-export|migrate> [path] [destination]; create-note [workspace] [--category segment] [--template name] [--name file-name]; migrate [workspace] [--dry-run] [--recover] [--settings file]\n",
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
    let fixture = TempWorkspace::new("cli-render").unwrap();
    let path = fixture.join("document.md");
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

#[test]
fn migration_dry_run_is_explicit_and_apply_preserves_legacy_inputs() {
    let root = TempWorkspace::new("cli-contract").unwrap();
    let control = root.join(".fleximark");
    fs::create_dir_all(&control).unwrap();
    fs::write(
        control.join("fleximark.json"),
        r#"{"noteFileNamePrefix":"daily-"}"#,
    )
    .unwrap();
    fs::write(control.join("parserPlugin.js"), "throw 'never execute';").unwrap();
    let root_text = root.to_str().unwrap();
    let dry = run(&["migrate", root_text, "--dry-run", "--trusted-workspace"]);
    assert!(dry.status.success(), "{}", text(&dry.stderr));
    let plan: serde_json::Value = serde_json::from_slice(&dry.stdout).unwrap();
    assert_eq!(plan["status"], "ready");
    assert!(text(&dry.stdout).contains("Unsupported JavaScript"));
    assert!(!control.join("config.toml").exists());
    let apply = run(&["migrate", root_text, "--trusted-workspace"]);
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
    let again = run(&["migrate", root_text, "--dry-run", "--trusted-workspace"]);
    assert!(again.status.success());
    let plan: serde_json::Value = serde_json::from_slice(&again.stdout).unwrap();
    assert_eq!(plan["status"], "alreadyCurrent");
}

#[test]
fn restricted_render_ignores_invalid_configuration_and_workspace_writes_are_denied() {
    let root = TempWorkspace::new("cli-contract").unwrap();
    fs::create_dir_all(root.join(".fleximark")).unwrap();
    fs::write(root.join(".fleximark/config.toml"), "invalid = [").unwrap();
    let document = root.join("doc.md");
    fs::write(&document, "# Safe\n").unwrap();
    let output = run(&["render", document.to_str().unwrap()]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stdout).contains("Safe"));
    for command in ["init", "edit-theme", "create-note"] {
        let denied = run(&[command, root.to_str().unwrap()]);
        assert!(!denied.status.success(), "{command}");
        assert!(denied.stdout.is_empty());
    }
    assert_eq!(
        fs::read_to_string(root.join(".fleximark/config.toml")).unwrap(),
        "invalid = ["
    );
    assert!(!root.join(".fleximark/theme.css").exists());
}

#[test]
fn create_note_applies_nested_category_template_and_name_and_rejects_unsafe_options() {
    let root = TempWorkspace::new("cli-contract").unwrap();
    fs::create_dir_all(root.join(".fleximark")).unwrap();
    fs::write(
        root.join(".fleximark/config.toml"),
        r##"schema_version = 1
[notes]
root = "notes"
[notes.categories.Work.Reports]
[notes.templates]
daily = ["# ${FILE_NAME}", "${1:Body}"]
"##,
    )
    .unwrap();
    let root_text = root.to_str().unwrap();
    let result = run(&[
        "create-note",
        root_text,
        "--trusted-workspace",
        "--category",
        "Work",
        "--category",
        "Reports",
        "--template",
        "daily",
        "--name",
        "session",
    ]);
    assert!(result.status.success(), "{}", text(&result.stderr));
    assert_eq!(
        fs::read_to_string(root.join("notes/Work/Reports/session.md")).unwrap(),
        "# session\nBody\n"
    );
    let before = fs::read_dir(root.join("notes/Work/Reports"))
        .unwrap()
        .count();
    for options in [
        vec!["--name", "../escape"],
        vec!["--name", ""],
        vec!["--category", ".."],
        vec!["--category", ""],
        vec!["--template", "missing"],
    ] {
        let mut arguments = vec!["create-note", root_text, "--trusted-workspace"];
        arguments.extend(options);
        let denied = run(&arguments);
        assert!(!denied.status.success(), "{arguments:?}");
        assert!(denied.stdout.is_empty());
    }
    assert_eq!(
        fs::read_dir(root.join("notes/Work/Reports"))
            .unwrap()
            .count(),
        before
    );
    assert!(!root.join("escape.md").exists());
}

#[test]
fn product_binary_rejects_benchmark_command() {
    let output = run(&["benchmark", "--json"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(text(&output.stderr).contains("unknown command: benchmark"));
}

#[test]
fn migration_settings_are_explicit_and_recovery_rejects_conflicting_options() {
    let root = TempWorkspace::new("cli-contract").unwrap();
    fs::create_dir_all(root.join(".fleximark")).unwrap();
    fs::write(
        root.join(".fleximark/fleximark.json"),
        r#"{"noteFileNamePrefix":"legacy-"}"#,
    )
    .unwrap();
    let settings = root.join("explicit-settings.json");
    let settings_text = r#"{"fleximark.noteFileNamePrefix":"explicit-"}"#;
    fs::write(&settings, settings_text).unwrap();
    let root_text = root.to_str().unwrap();
    let settings_path = settings.to_str().unwrap();
    let dry = run(&[
        "migrate",
        root_text,
        "--trusted-workspace",
        "--dry-run",
        "--settings",
        settings_path,
    ]);
    assert!(dry.status.success(), "{}", text(&dry.stderr));
    let summary: serde_json::Value = serde_json::from_slice(&dry.stdout).unwrap();
    assert!(summary["config"].as_str().unwrap().contains("explicit-"));
    assert!(!root.join(".fleximark/config.toml").exists());
    let denied = run(&["migrate", root_text, "--settings", settings_path]);
    assert!(!denied.status.success());
    assert!(denied.stdout.is_empty());
    for conflicting in [vec!["--dry-run"], vec!["--settings", settings_path]] {
        let mut arguments = vec!["migrate", root_text, "--trusted-workspace", "--recover"];
        arguments.extend(conflicting);
        let output = run(&arguments);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(text(&output.stderr).contains("--recover cannot be combined"));
    }
    assert_eq!(fs::read_to_string(settings).unwrap(), settings_text);
    assert!(!root.join(".fleximark/config.toml").exists());
}
