//! Role: Run architecture checks and deterministic repository fixture generation.
mod fixtures;
mod render_fixtures;
mod syntax;
fn main() -> std::process::ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
fn run() -> Result<std::process::ExitCode, String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or_else(|| "xtask must reside in dev/xtask".to_owned())?;
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|arg| arg == "fixtures") {
        if args.len() != 2 {
            return Err("usage: cargo xtask fixtures <generate|check>".into());
        }
        fixtures::run(root, &args[1])?;
        return Ok(std::process::ExitCode::SUCCESS);
    }
    Ok(xtask_core::main(root))
}
