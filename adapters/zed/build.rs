//! Role: Embed shared release versions in the Zed runtime installer.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../../release.toml");
    let metadata = fleximark_distribution::release_metadata()?;
    println!(
        "cargo:rustc-env=FLEXIMARK_RELEASE_VERSION={}",
        metadata.version
    );
    println!(
        "cargo:rustc-env=FLEXIMARK_PROTOCOL_VERSION={}",
        metadata.protocol_version
    );
    Ok(())
}
