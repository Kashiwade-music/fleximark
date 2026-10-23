//! Role: Embed the shared release metadata into the Zed runtime installer.

fn main() {
    println!("cargo:rerun-if-changed=../../release.toml");
    let metadata = std::fs::read_to_string("../../release.toml").expect("release metadata");
    let version = metadata
        .lines()
        .find_map(|line| {
            line.strip_prefix("version = ")
                .map(|value| value.trim_matches('"'))
        })
        .expect("release version");
    println!("cargo:rustc-env=FLEXIMARK_RELEASE_VERSION={version}");
    let protocol = metadata
        .lines()
        .find_map(|line| line.strip_prefix("protocol_version = ").map(str::trim))
        .expect("protocol version");
    println!("cargo:rustc-env=FLEXIMARK_PROTOCOL_VERSION={protocol}");
}
