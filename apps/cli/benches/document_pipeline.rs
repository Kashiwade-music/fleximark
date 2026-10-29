//! Role: Measure document parsing, rendering, and complete HTML update costs.
use fleximark_engine::DocumentEngine;
use fleximark_model::{DocumentUri, PositionEncoding};
use std::{env, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments
        .last()
        .is_some_and(|argument| argument == "--bench")
    {
        arguments.pop();
    }
    if arguments.is_empty() || arguments == ["--test"] {
        return smoke();
    }
    if arguments != ["--json"] {
        return Err("document_pipeline requires --json".into());
    }
    let mut results = Vec::new();
    for lines in [1_000_usize, 10_000, 100_000] {
        eprintln!("benchmark: start {lines} lines");
        let source = (0..lines)
            .map(|index| format!("line {index}\n"))
            .collect::<String>();
        let parse_started = Instant::now();
        let mut session = DocumentEngine::open(
            DocumentUri(format!("file:///benchmark-{lines}.md")),
            1,
            source.clone(),
            PositionEncoding::Utf8,
            Default::default(),
        )?;
        let parse_ms = parse_started.elapsed().as_secs_f64() * 1_000.0;
        let render_started = Instant::now();
        let full = session.render(&fleximark_render_html::RenderContext::default())?;
        let render_ms = render_started.elapsed().as_secs_f64() * 1_000.0;
        let full_bytes = full.html().len();
        let mut frame_bytes = 0;
        let mut max_frame_bytes = 0;
        let mut edit_update_ms = 0.0;
        let mut edit_render_ms = 0.0;
        let edits_started = Instant::now();
        for edit in 0..20_u64 {
            let replacement = if edit % 2 == 0 { "Line" } else { "line" };
            let mut changed = source.clone();
            changed.replace_range(..4, replacement);
            let update_started = Instant::now();
            session.change(edit + 2, changed)?;
            edit_update_ms += update_started.elapsed().as_secs_f64() * 1_000.0;
            let edit_render_started = Instant::now();
            let frame = session.render(&fleximark_render_html::RenderContext::default())?;
            edit_render_ms += edit_render_started.elapsed().as_secs_f64() * 1_000.0;
            let bytes = frame.html().len();
            frame_bytes += bytes;
            max_frame_bytes = max_frame_bytes.max(bytes);
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
            "frameBytes": frame_bytes,
            "maxFrameBytes": max_frame_bytes
        }));
    }
    let special_started = Instant::now();
    let special = DocumentEngine::open(
        DocumentUri("file:///benchmark-special.md".into()),
        1,
        "```mermaid\ngraph TD; A-->B\n```\n```abc\nX:1\nK:C\nC\n```\n$E=mc^2$\n".into(),
        PositionEncoding::Utf8,
        Default::default(),
    )?;
    special.render(&fleximark_render_html::RenderContext::default())?;
    let special_render_ms = special_started.elapsed().as_secs_f64() * 1_000.0;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "schemaVersion": 1,
            "byteMetric": "renderedHtmlUtf8",
            "peakMemoryBytes": peak_memory_bytes()?,
            "specialRenderMs": special_render_ms,
            "documents": results
        }))?
    );
    Ok(())
}

fn smoke() -> Result<(), Box<dyn std::error::Error>> {
    let source = (0..1_000)
        .map(|index| format!("line {index}\n"))
        .collect::<String>();
    let mut session = DocumentEngine::open(
        DocumentUri("file:///document-pipeline-smoke.md".to_owned()),
        1,
        source.clone(),
        PositionEncoding::Utf8,
        Default::default(),
    )?;
    let context = fleximark_render_html::RenderContext::default();
    let before = session.render(&context)?;
    if before.document_version != 1
        || !before.html().contains("line 999")
        || before.blocks.is_empty()
    {
        return Err("pipeline smoke failed to render its initial document".into());
    }
    let mut changed = source;
    changed.replace_range(..4, "Line");
    session.change(2, changed)?;
    let after = session.render(&context)?;
    if after.document_version != 2
        || !after.html().contains("Line 0")
        || before.blocks[0].id != after.blocks[0].id
        || before.html().contains("Line 0")
    {
        return Err("pipeline smoke failed to preserve identity and snapshot independence".into());
    }
    Ok(())
}

fn peak_memory_bytes() -> Result<u64, Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status")?;
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
