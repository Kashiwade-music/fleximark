use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use fleximark_engine::{DocumentSession, PreviewSessionId};
use fleximark_model::{DocumentUri, PositionEncoding};
#[cfg(test)]
use fleximark_render_html::RenderContext;

const PREVIEW_CLIENT: &str = include_str!("../../../web/preview-client/browser-host.js");

fn main() {
    if let Err(error) = run() {
        eprintln!("fleximark: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut raw_args = env::args().skip(1).collect::<Vec<_>>();
    let trusted_workspace = raw_args
        .iter()
        .position(|argument| argument == "--trusted-workspace")
        .map(|index| {
            raw_args.remove(index);
            true
        })
        .unwrap_or(false);
    let mut args = raw_args.into_iter();
    let command = args.next().ok_or(
        "usage: fleximark <render|benchmark|init|edit-theme|create-note|collect-admonitions|export|ack-export> [path] [destination]",
    )?;
    let path = args.next();
    let destination = args.next();
    if args.next().is_some()
        || (!matches!(command.as_str(), "export" | "ack-export") && destination.is_some())
    {
        return Err("too many arguments".into());
    }
    if !trusted_workspace
        && matches!(
            command.as_str(),
            "init" | "edit-theme" | "create-note" | "collect-admonitions" | "export" | "ack-export"
        )
    {
        return Err("workspace writes require --trusted-workspace".into());
    }
    match command.as_str() {
        "benchmark" => {
            if path.as_deref() != Some("--json") {
                return Err("benchmark requires --json".into());
            }
            let mut results = Vec::new();
            for lines in [1_000_usize, 10_000, 100_000] {
                eprintln!("benchmark: start {lines} lines");
                let source = (0..lines)
                    .map(|index| format!("line {index}\n"))
                    .collect::<String>();
                let parse_started = Instant::now();
                let mut session = DocumentSession::open(
                    DocumentUri(format!("file:///benchmark-{lines}.md")),
                    1,
                    source.clone(),
                    PositionEncoding::Utf8,
                )?;
                let parse_ms = parse_started.elapsed().as_secs_f64() * 1_000.0;
                let render_started = Instant::now();
                let full = session.render(
                    PreviewSessionId(format!("benchmark-{lines}")),
                    &fleximark_render_html::RenderContext::default(),
                )?;
                let render_ms = render_started.elapsed().as_secs_f64() * 1_000.0;
                let full_bytes = serde_json::to_vec(&full)?.len();
                let mut patch_bytes = 0;
                let mut max_patch_bytes = 0;
                let mut full_fallbacks = 0;
                let mut edit_update_ms = 0.0;
                let mut edit_render_ms = 0.0;
                let edits_started = Instant::now();
                for edit in 0..20_u64 {
                    let replacement = if edit % 2 == 0 { "Line" } else { "line" };
                    let mut changed = source.clone();
                    changed.replace_range(..4, replacement);
                    let update_started = Instant::now();
                    session.change_full_text(edit + 2, changed)?;
                    edit_update_ms += update_started.elapsed().as_secs_f64() * 1_000.0;
                    let edit_render_started = Instant::now();
                    let publication = session.render(
                        PreviewSessionId(format!("benchmark-{lines}")),
                        &fleximark_render_html::RenderContext::default(),
                    )?;
                    edit_render_ms += edit_render_started.elapsed().as_secs_f64() * 1_000.0;
                    match publication {
                        fleximark_engine::RenderPublication::Patch(patch) => {
                            let bytes = serde_json::to_vec(&patch)?.len();
                            patch_bytes += bytes;
                            max_patch_bytes = max_patch_bytes.max(bytes);
                        }
                        fleximark_engine::RenderPublication::Full(_) => full_fallbacks += 1,
                    }
                }
                let edit_burst_ms = edits_started.elapsed().as_secs_f64() * 1_000.0;
                eprintln!(
                    "benchmark: complete {lines} lines (update={edit_update_ms:.1}ms, render={edit_render_ms:.1}ms)"
                );
                results.push(serde_json::json!({
                    "lines": lines,
                    "parseMs": parse_ms,
                    "initialRenderMs": render_ms,
                    "fullBytes": full_bytes,
                    "editCount": 20,
                    "editBurstMs": edit_burst_ms,
                    "editUpdateMs": edit_update_ms,
                    "editRenderMs": edit_render_ms,
                    "patchBytes": patch_bytes,
                    "maxPatchBytes": max_patch_bytes,
                    "fullFallbacks": full_fallbacks
                }));
            }
            let special_started = Instant::now();
            let mut special = DocumentSession::open(
                DocumentUri("file:///benchmark-special.md".into()),
                1,
                "```mermaid\ngraph TD; A-->B\n```\n```abc\nX:1\nK:C\nC\n```\n$E=mc^2$\n".into(),
                PositionEncoding::Utf8,
            )?;
            special.render(
                PreviewSessionId("benchmark-special".into()),
                &fleximark_render_html::RenderContext::default(),
            )?;
            let special_render_ms = special_started.elapsed().as_secs_f64() * 1_000.0;
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "schemaVersion": 1,
                    "peakMemoryBytes": peak_memory_bytes()?,
                    "specialRenderMs": special_render_ms,
                    "documents": results
                }))?
            );
        }
        "render" => {
            let path = path.ok_or("render requires a document")?;
            let source = fs::read_to_string(&path)?;
            let uri = path_to_uri(Path::new(&path))?;
            let mut session = open_session(uri, source, trusted_workspace)?;
            let publication = session.render_full_configured(
                PreviewSessionId("cli".into()),
                &fleximark_plugin_host::CancellationToken::default(),
            )?;
            let fleximark_engine::RenderPublication::Full(snapshot) = publication.publication
            else {
                unreachable!("configured full render returned a patch")
            };
            let html = snapshot.html;
            io::stdout().lock().write_all(html.as_bytes())?;
        }
        "init" | "edit-theme" | "create-note" => {
            let root = PathBuf::from(path.unwrap_or_else(|| ".".into()));
            let uri = fleximark_service::path_to_file_uri(&root)?;
            let result = match command.as_str() {
                "init" => fleximark_service::initialize_workspace(&uri)?,
                "edit-theme" => fleximark_service::edit_theme(&uri)?,
                _ => fleximark_service::create_note(&uri)?,
            };
            if let Some(message) = result.message {
                println!("{}", message.text);
            }
            if let Some(uri) = result.open_uri {
                println!("{uri}");
            }
        }
        "collect-admonitions" => {
            let path = path.ok_or("collect-admonitions requires a document")?;
            let source = fs::read_to_string(&path)?;
            let session = open_session(
                path_to_uri(Path::new(&path))?,
                source.clone(),
                trusted_workspace,
            )?;
            let workspace_uri = fleximark_service::workspace_for_document(&path_to_uri(
                &Path::new(&path).canonicalize()?,
            )?)?;
            if let Some(message) =
                fleximark_service::collect_admonitions(session.document(), &source, &workspace_uri)?
                    .message
            {
                println!("{}", message.text);
            }
        }
        "export" => {
            let path = path.ok_or("export requires a document")?;
            let canonical = Path::new(&path).canonicalize()?;
            let source_uri = path_to_uri(&canonical)?;
            let source = fs::read_to_string(&canonical)?;
            let workspace_uri = fleximark_service::workspace_for_document(&source_uri)?;
            let context = fleximark_service::export_render_context(&workspace_uri)?;
            let destination_uri = match destination {
                Some(path) => {
                    let path = PathBuf::from(path);
                    let absolute = if path.is_absolute() {
                        path
                    } else {
                        env::current_dir()?.join(path)
                    };
                    let parent = absolute.parent().ok_or("destination has no parent")?;
                    let name = absolute.file_name().ok_or("destination has no name")?;
                    fleximark_service::path_to_file_uri(&parent.canonicalize()?.join(name))?
                }
                None => fleximark_service::default_export_destination(&source_uri)?,
            };
            fleximark_service::preflight_export(&source_uri, &workspace_uri, &destination_uri)?;
            let session = open_session(source_uri.clone(), source.clone(), trusted_workspace)?;
            let prepared = session.prepare_safe_export(&context)?;
            let mut assets = None;
            let resolved = prepared.compose_portable(
                PREVIEW_CLIENT,
                |safe_html, style, render_assets, runtime| {
                    let resolved = fleximark_service::resolve_export_assets(
                        &source_uri,
                        &workspace_uri,
                        safe_html,
                        render_assets,
                    )?;
                    assets = Some(resolved.assets);
                    fleximark_service::compose_portable_html(&resolved.html, style, runtime)
                },
            )?;
            let output = session
                .apply_unsafe_export_html(
                    resolved,
                    &fleximark_plugin_host::CancellationToken::default(),
                )?
                .value;
            let result = fleximark_service::export_html_with_safety(
                &source_uri,
                &workspace_uri,
                &destination_uri,
                &output.html,
                &assets.unwrap_or_default(),
                output.unsafe_output_used,
            )?;
            if let Some(message) = result.message {
                println!("{}", message.text);
            }
            if let Some(uri) = result.open_uri {
                println!("{uri}");
            }
        }
        "ack-export" => {
            let path = path.ok_or("ack-export requires a document")?;
            let source_uri = path_to_uri(&Path::new(&path).canonicalize()?)?;
            let workspace_uri = fleximark_service::workspace_for_document(&source_uri)?;
            let destination_uri = match destination {
                Some(path) => {
                    let path = PathBuf::from(path);
                    let absolute = if path.is_absolute() {
                        path
                    } else {
                        env::current_dir()?.join(path)
                    };
                    let parent = absolute.parent().ok_or("destination has no parent")?;
                    let name = absolute.file_name().ok_or("destination has no name")?;
                    fleximark_service::path_to_file_uri(&parent.canonicalize()?.join(name))?
                }
                None => fleximark_service::default_export_destination(&source_uri)?,
            };
            fleximark_service::acknowledge_export(&source_uri, &workspace_uri, &destination_uri)?;
            println!("Export opened and validated; recovery backup released");
        }
        _ => return Err(format!("unknown command: {command}").into()),
    }
    Ok(())
}

fn peak_memory_bytes() -> Result<u64, Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    {
        let status = fs::read_to_string("/proc/self/status")?;
        let kib = status
            .lines()
            .find_map(|line| line.strip_prefix("VmHWM:"))
            .and_then(|value| value.split_whitespace().next())
            .ok_or("/proc/self/status did not report VmHWM")?
            .parse::<u64>()?;
        Ok(kib * 1024)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::ProcessStatus::{
            K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut counters = unsafe { std::mem::zeroed::<PROCESS_MEMORY_COUNTERS>() };
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        if unsafe {
            K32GetProcessMemoryInfo(
                GetCurrentProcess(),
                &mut counters,
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(counters.PeakWorkingSetSize as u64)
    }
    #[cfg(target_os = "macos")]
    {
        let mut usage = unsafe { std::mem::zeroed::<libc::rusage>() };
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(usage.ru_maxrss as u64)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Err("peak RSS measurement is unsupported on this platform".into())
    }
}

fn open_session(
    uri: String,
    source: String,
    trusted_workspace: bool,
) -> Result<DocumentSession, Box<dyn std::error::Error>> {
    if !trusted_workspace {
        return Ok(DocumentSession::open(
            DocumentUri(uri),
            1,
            source,
            PositionEncoding::Utf8,
        )?);
    }
    match fleximark_service::workspace_for_document(&uri) {
        Ok(workspace_uri) => {
            let (host, render_config) =
                fleximark_service::load_plugin_host(&workspace_uri, true, 1)?;
            let host = Arc::new(host);
            let mut session = DocumentSession::open_configured(
                DocumentUri(uri.clone()),
                1,
                source,
                PositionEncoding::Utf8,
                render_config.clone(),
                host,
                &fleximark_plugin_host::CancellationToken::default(),
            )?;
            let assets =
                fleximark_service::resolve_render_assets(&uri, &workspace_uri, session.document())?;
            session.reconfigure_render(render_config.with_resolved_assets(assets)?)?;
            Ok(session)
        }
        Err(fleximark_service::ServiceError::NotInitialized) => Ok(DocumentSession::open(
            DocumentUri(uri),
            1,
            source,
            PositionEncoding::Utf8,
        )?),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
fn render_source(uri: String, source: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut session = DocumentSession::open(
        DocumentUri(uri),
        1,
        source.to_owned(),
        PositionEncoding::Utf8,
    )?;
    Ok(session
        .render_full(PreviewSessionId("cli".into()), &RenderContext::default())?
        .html)
}

fn path_to_uri(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let canonical = path.canonicalize()?;
    let normalized = canonical.to_string_lossy().replace('\\', "/");
    Ok(if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_markdown_without_an_editor() {
        let html = render_source("file:///test.md".into(), "# Hello\n").unwrap();
        assert!(html.contains("Hello"));
        assert!(!html.contains("pending-"));
        assert!(html.starts_with("<main data-fleximark-node-id=\"document-root\">"));
    }

    #[test]
    fn default_session_does_not_load_workspace_plugins() {
        let root = std::env::temp_dir().join(format!(
            "fleximark-cli-untrusted-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let workspace_uri = fleximark_service::path_to_file_uri(&root).unwrap();
        fleximark_service::initialize_workspace(&workspace_uri).unwrap();
        fs::write(
            root.join(".fleximark/config.toml"),
            "schema_version = 1\n[[plugins]]\nid='malicious'\nwasm='missing.wasm'\nmanifest='missing.toml'\nsignature='missing.sig'\nmanifest_sha256='0000000000000000000000000000000000000000000000000000000000000000'\nsigner_public_key='0000000000000000000000000000000000000000000000000000000000000000'\n",
        )
        .unwrap();
        let document = root.join("doc.md");
        fs::write(&document, "# Safe\n").unwrap();
        let session = open_session(path_to_uri(&document).unwrap(), "# Safe\n".into(), false)
            .expect("untrusted CLI never loads configured plugins");
        assert_eq!(session.document().blocks.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
