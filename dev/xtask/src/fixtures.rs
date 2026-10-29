//! Role: Generate deterministic renderer fixtures and browser syntax constants.
use crate::{render_fixtures, syntax};
use std::path::Path;
pub(crate) fn run(root: &Path, mode: &str) -> Result<(), String> {
    if !matches!(mode, "generate" | "check") {
        return Err("usage: cargo xtask fixtures <generate|check>".into());
    }
    let outputs = [
        (
            "tests/fixtures/protocol/rust-render-frame.json",
            render_fixtures::generate_render_frame_fixture()?,
        ),
        (
            "tests/fixtures/protocol/rust-navigation-frame.json",
            render_fixtures::render_fixture(
                "file:///rust-navigation.md",
                1,
                std::fs::read_to_string(root.join("tests/fixtures/protocol/rust-navigation.md"))
                    .map_err(|error| error.to_string())?,
                "serve-preview",
            )?,
        ),
        (
            "contracts/typescript/syntax.generated.mts",
            syntax::typescript()?,
        ),
    ];
    for (relative, contents) in outputs {
        let path = root.join(relative);
        if mode == "generate" {
            std::fs::write(&path, contents)
                .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
            println!("generated {relative}");
        } else if std::fs::read_to_string(&path).map_err(|error| error.to_string())? != contents {
            return Err(format!("generated output is stale: {relative}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Role: Check fixture and syntax generation without mutating repository files.
    use super::*;
    #[test]
    fn generators_are_deterministic_and_produce_current_contracts() {
        let first = render_fixtures::generate_render_frame_fixture().unwrap();
        assert_eq!(
            first,
            render_fixtures::generate_render_frame_fixture().unwrap()
        );
        let frame: fleximark_protocol::RenderFrame = serde_json::from_str(&first).unwrap();
        assert!(!frame.blocks.is_empty());
        let kinds = syntax::typescript().unwrap();
        assert_eq!(kinds, syntax::typescript().unwrap());
        assert!(kinds.contains("BUILTIN_RENDER_KINDS"));
    }
    #[test]
    fn invalid_mode_fails_before_accessing_the_workspace() {
        assert!(run(Path::new("missing-workspace"), "unsupported").is_err());
    }
}
