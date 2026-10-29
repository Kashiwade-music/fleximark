//! Role: Dispatch CLI document and workspace commands through application APIs.
use fleximark_application::{
    CancellationToken, CommandOutcome, DocumentStore, DocumentUpdate, WorkspaceAuthority,
    WorkspaceOps,
};
use fleximark_model::PositionEncoding;
use fleximark_preview_assets::PreviewAssets;
use fleximark_protocol::{CommandContext, DocumentCommandContext, WorkspaceCommand};
use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

type CliResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const USAGE: &str = "usage: fleximark <render|init|edit-theme|create-note|collect-admonitions|export|ack-export|migrate> [path] [destination]; create-note [workspace] [--category segment] [--template name] [--name file-name]; migrate [workspace] [--dry-run] [--recover] [--settings file]";

fn main() {
    if let Err(error) = run() {
        eprintln!("fleximark: {error}");
        std::process::exit(1);
    }
}

fn run() -> CliResult {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args == ["--version"] {
        println!("fleximark {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let trusted = args
        .iter()
        .position(|argument| argument == "--trusted-workspace")
        .map(|index| {
            args.remove(index);
            true
        })
        .unwrap_or(false);
    let command = if args.is_empty() {
        return Err(USAGE.into());
    } else {
        args.remove(0)
    };
    if command == "migrate" {
        return migrate(args, trusted);
    }
    if command == "create-note" {
        return create_note(args, trusted);
    }
    let mut args = args.into_iter();
    let path = args.next();
    let destination = args.next();
    if args.next().is_some()
        || (!matches!(command.as_str(), "export" | "ack-export") && destination.is_some())
    {
        return Err("too many arguments".into());
    }
    let token = CancellationToken::default();
    let mut store = DocumentStore::new(PositionEncoding::Utf8);
    match command.as_str() {
        "render" => {
            let path = path.ok_or("render requires a document")?;
            let uri = WorkspaceAuthority::path_to_file_uri(Path::new(&path))?;
            store.open(
                DocumentUpdate {
                    uri: uri.clone(),
                    version: 1,
                    source: fs::read_to_string(path)?,
                },
                &token,
            )?;
            let attachment = store.attach(&uri, 1)?;
            let document = store.document(&attachment.document_session_id, 1)?;
            let snapshot = document
                .engine()
                .render(&document.engine().render_config().context)?;
            io::stdout().lock().write_all(snapshot.html().as_bytes())?;
        }
        "init" | "edit-theme" => {
            let root = PathBuf::from(path.unwrap_or_else(|| ".".into()));
            let uri = WorkspaceAuthority::path_to_file_uri(&root)?;
            store.configure_native_workspaces(vec![(uri.clone(), trusted)])?;
            let operation = if command == "init" {
                WorkspaceCommand::InitializeWorkspace {}
            } else {
                WorkspaceCommand::EditTheme {}
            };
            let outcome = execute(&mut store, operation, workspace_context(uri), None, &token)?;
            match outcome {
                CommandOutcome::Initialized(result) => println!("{}", result.config_uri),
                CommandOutcome::Theme(result) => println!("{}", result.uri),
                _ => return Err("unexpected workspace operation outcome".into()),
            }
        }
        "collect-admonitions" | "export" | "ack-export" => {
            let path = path.ok_or("command requires a document")?;
            let path = Path::new(&path).canonicalize()?;
            let source_uri = WorkspaceAuthority::path_to_file_uri(&path)?;
            let workspace_uri = WorkspaceAuthority::discover_workspace(&source_uri)?;
            store.configure_native_workspaces(vec![(workspace_uri.clone(), trusted)])?;
            store.open(
                DocumentUpdate {
                    uri: source_uri.clone(),
                    version: 1,
                    source: fs::read_to_string(path)?,
                },
                &token,
            )?;
            let attachment = store.attach(&source_uri, 1)?;
            let context = CommandContext {
                workspace_uri: Some(workspace_uri),
                document: Some(DocumentCommandContext {
                    document_session_id: attachment.document_session_id,
                    expected_document_version: 1_i64.try_into()?,
                }),
            };
            let operation = match command.as_str() {
                "export" => WorkspaceCommand::ExportHtml {
                    destination_uri: Some(export_destination(&source_uri, destination)?),
                },
                "ack-export" => WorkspaceCommand::AcknowledgeExport {
                    destination_uri: Some(export_destination(&source_uri, destination)?),
                },
                _ => WorkspaceCommand::CollectAdmonitions {},
            };
            let resources = if command == "export" {
                Some(Arc::new(PreviewAssets::installed()?))
            } else {
                None
            };
            match execute(&mut store, operation, context, resources, &token)? {
                CommandOutcome::Exported(result) => println!("{}", result.output_uri),
                CommandOutcome::ExportAcknowledged => {
                    println!("Export opened and validated; recovery backup released")
                }
                CommandOutcome::Admonitions(result) => {
                    println!("Collected {} admonitions: {}", result.count, result.uri)
                }
                _ => return Err("unexpected document operation outcome".into()),
            }
        }
        _ => return Err(format!("unknown command: {command}").into()),
    }
    Ok(())
}

fn workspace_context(uri: String) -> CommandContext {
    CommandContext {
        workspace_uri: Some(uri),
        document: None,
    }
}

fn execute(
    store: &mut DocumentStore,
    operation: WorkspaceCommand,
    context: CommandContext,
    resources: Option<Arc<PreviewAssets>>,
    token: &CancellationToken,
) -> CliResult<CommandOutcome> {
    // Apply the protocol's argument constraints to values built by the CLI too.
    let operation = serde_json::from_value(serde_json::to_value(operation)?)?;
    Ok(WorkspaceOps::new(store, resources).execute(operation, context, token)?)
}

fn export_destination(source_uri: &str, destination: Option<String>) -> CliResult<String> {
    let Some(destination) = destination else {
        return Ok(WorkspaceAuthority::default_export_destination(source_uri)?);
    };
    let path = PathBuf::from(destination);
    let absolute = if path.is_absolute() {
        path
    } else {
        env::current_dir()?.join(path)
    };
    let parent = absolute.parent().ok_or("destination has no parent")?;
    let name = absolute.file_name().ok_or("destination has no name")?;
    Ok(WorkspaceAuthority::path_to_file_uri(
        &parent.canonicalize()?.join(name),
    )?)
}

fn create_note(args: Vec<String>, trusted: bool) -> CliResult {
    let mut root = None;
    let mut categories = Vec::new();
    let mut template = None;
    let mut name = None;
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--category" => categories.push(args.next().ok_or("--category requires a segment")?),
            "--template" => {
                let value = args.next().ok_or("--template requires a name")?;
                if template.replace(value).is_some() {
                    return Err("--template may only be supplied once".into());
                }
            }
            "--name" => {
                let value = args.next().ok_or("--name requires a file name")?;
                if name.replace(value).is_some() {
                    return Err("--name may only be supplied once".into());
                }
            }
            value if value.starts_with("--") => {
                return Err(format!("unknown create-note option: {value}").into());
            }
            _ => {
                if root.replace(PathBuf::from(argument)).is_some() {
                    return Err("create-note accepts one workspace".into());
                }
            }
        }
    }
    let root = root.unwrap_or_else(|| PathBuf::from("."));
    let uri = WorkspaceAuthority::path_to_file_uri(&root)?;
    let mut store = DocumentStore::new(PositionEncoding::Utf8);
    store.configure_native_workspaces(vec![(uri.clone(), trusted)])?;
    let operation = WorkspaceCommand::CreateNote {
        category_path: if categories.is_empty() {
            None
        } else {
            Some(categories)
        },
        template,
        name,
    };
    match execute(
        &mut store,
        operation,
        workspace_context(uri),
        None,
        &CancellationToken::default(),
    )? {
        CommandOutcome::Note(result) => println!("{}", result.uri),
        _ => return Err("unexpected note operation outcome".into()),
    }
    Ok(())
}

fn migrate(args: Vec<String>, trusted: bool) -> CliResult {
    let mut dry_run = false;
    let mut recover = false;
    let mut root = None;
    let mut settings = None;
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--dry-run" => dry_run = true,
            "--recover" => recover = true,
            "--settings" => {
                settings = Some(PathBuf::from(
                    args.next().ok_or("--settings requires a JSON file")?,
                ))
            }
            value if value.starts_with("--") => {
                return Err(format!("unknown migration option: {value}").into());
            }
            _ => {
                if root.replace(PathBuf::from(argument)).is_some() {
                    return Err("migrate accepts one workspace".into());
                }
            }
        }
    }
    if recover && (dry_run || settings.is_some()) {
        return Err("--recover cannot be combined with --dry-run or --settings".into());
    }
    let uri = WorkspaceAuthority::path_to_file_uri(&root.unwrap_or_else(|| PathBuf::from(".")))?;
    let settings_uri = settings
        .as_deref()
        .map(WorkspaceAuthority::path_to_file_uri)
        .transpose()?;
    let mut store = DocumentStore::new(PositionEncoding::Utf8);
    store.configure_native_workspaces(vec![(uri.clone(), trusted)])?;
    let context = workspace_context(uri);
    let token = CancellationToken::default();
    let plan = match execute(
        &mut store,
        WorkspaceCommand::PlanMigration {
            settings_uri,
            recover,
        },
        context.clone(),
        None,
        &token,
    )? {
        CommandOutcome::MigrationPlanned(plan) => plan,
        _ => return Err("unexpected migration plan outcome".into()),
    };
    println!("{}", serde_json::to_string_pretty(&plan)?);
    if !dry_run {
        match execute(
            &mut store,
            WorkspaceCommand::ApplyMigration {
                plan_id: plan.plan_id,
            },
            context,
            None,
            &token,
        )? {
            CommandOutcome::MigrationApplied(result) => {
                let fleximark_application::MigrationAppliedOutcome {
                    backup_uri,
                    recovered,
                } = result;
                if recovered {
                    println!("Migration recovery completed");
                }
                if let Some(backup_uri) = backup_uri {
                    println!(
                        "Migration applied; original workspace data backed up at {}",
                        WorkspaceAuthority::workspace_path(&backup_uri)?.display()
                    );
                }
            }
            _ => return Err("unexpected migration apply outcome".into()),
        }
    }
    Ok(())
}
