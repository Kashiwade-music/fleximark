# Runtime distribution contract

`release.toml` is the product version and protocol version source. A release has
one `fleximark-<version>-<target>.zip` per supported Rust target, plus its
`.zip.sha256` sidecar. ZIPs have no enclosing directory. All paths use `/`.

The archive contains `manifest.json`, `bin/fleximarkd[.exe]`,
`bin/fleximark[.exe]`, `resources/preview/browser-host.js`,
`resources/preview/fleximark.css`, `resources/preview/katex.css`,
`resources/workspace/workspace.js`, and `resources/workspace/workspace.css`.
Windows targets use `.exe`; Unix binaries have executable mode bits.

The manifest follows `runtime-manifest.schema.json`. Its `files` map lists every
payload path and lowercase SHA-256 digest; the manifest does not hash itself.
Consumers verify version, target, protocol version, the exact payload set and
every digest before launch. Paths outside the archive root, duplicate entries,
special files, oversized payloads and mismatched targets are rejected.

Each target VSIX embeds this exact archive's contents under `runtime/`. Release
verification compares every extracted byte with the separately published ZIP.
Zed downloads the same ZIP. GitHub attestations bind archives, VSIX files and
VSIX identity sidecars to their source commit. Hash sidecars detect corruption;
attestation establishes provenance. Publishing only follows platform integration
and isolated installation smoke checks.

The universal Zed editor product is released once as
`fleximark-zed-<version>.zip`, with `extension.toml`, `extension.wasm`, and its
SHA-256 sidecar. The extension downloads the matching target runtime ZIP after
installation. Its product archive is covered by the same provenance attestation.

Source files remain in their owning packages. `.build/<component>` holds browser
and adapter output, `.artifacts/<version>/<target>/<product>` holds assembly, and
all Cargo output remains beneath the root `target/` directory.
