//! Role: Embed canonical release metadata without browser build inputs.
fn main() {
    println!(
        "cargo:rustc-env=FLEXIMARK_BUILD_TARGET={}",
        std::env::var("TARGET").expect("Cargo target")
    );
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let metadata = root.join("release.toml");
    println!("cargo:rerun-if-changed={}", metadata.display());
    let source = std::fs::read_to_string(metadata).expect("canonical release metadata");
    for (key, env_name) in [
        ("version", "FLEXIMARK_RELEASE_VERSION"),
        ("protocol_version", "FLEXIMARK_PROTOCOL_VERSION"),
    ] {
        let value = source
            .lines()
            .filter_map(|line| line.split_once('='))
            .find(|(name, _)| name.trim() == key)
            .map(|(_, value)| value.trim().trim_matches('"'))
            .expect("release metadata field");
        println!("cargo:rustc-env={env_name}={value}");
    }
}
