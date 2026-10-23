//! Role: Run shared architecture checks for the native and editor workspaces.

fn main() -> std::process::ExitCode {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("dev directory")
        .parent()
        .expect("repository root");
    xtask_core::main(root)
}
