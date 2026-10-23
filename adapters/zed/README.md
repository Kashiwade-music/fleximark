# FlexiMark for official Zed

This independent WASM extension uses the published `zed_extension_api = 0.7.0`.
Install it with Zed's **Install Dev Extension** command, selecting this directory,
then open a Markdown file. It registers the FlexiMark language server for Zed's
existing Markdown language; it supplies no grammar or editor fork.

Set this in Zed settings (the binary entry is optional):

```json
{
  "languages": {
    "Markdown": { "language_servers": ["fleximark", "..."] }
  },
  "lsp": {
    "fleximark": {
      "binary": { "path": "/absolute/runtime/bin/fleximarkd" },
      "initialization_options": {
        "fleximark": {
          "workspaces": [{ "uri": "file:///absolute/workspace", "trusted": true }],
          "capabilities": { "selectionEvents": false, "viewportEvents": false }
        }
      }
    }
  }
}
```

Use a Windows absolute executable path with escaped backslashes when applicable.
`initialization_options` passes unchanged through the official extension API.
Zed supplies ordinary LSP workspace folders/root URI; the server treats roots as
untrusted unless explicitly configured. Configure trust only for workspaces you
intend to permit rendering and workspace writes in.

Binary selection is explicit `lsp.fleximark.binary.path`, then `fleximarkd` on the
worktree shell's PATH, then the versioned GitHub release matching `release.toml`.
The extension always starts that binary with `lsp` and checks `--version` first.
It does not interpret custom binary arguments. Downloads use the official Zed
GitHub/download API and ZIP extraction. The archive name is
`fleximark-{version}-{Rust target}.zip`; the installer verifies manifest schema,
version, platform target, current protocol, required binaries/browser resources, and
every listed SHA-256 hash before launch. It rejects incompatible local binaries.
The manifest permits exactly its five canonical fields and seven distribution
files. Empty files, links, nonregular files, malformed checksums and unlisted
installation entries are rejected.
The archive includes the CLI alongside the daemon so explicit migration can
invoke it. Release availability is required for automatic installation.

Use Zed's code actions on a Markdown document or selected range:

| Action | Result |
| --- | --- |
| Preview document / Preview at selection | Open the live shared preview in the external browser |
| Open FlexiMark workspace | Open document/workspace selection and management |
| Create note | Open category, template and note-name forms |
| Export HTML | Open the export form |
| Initialize workspace | Open workspace initialization |
| Edit theme | Open theme editing |
| Collect admonitions | Open admonition collection |

Unsaved buffer changes update the preview through `textDocument/didChange`.
Preview at selection sends URI, version and range with the action. Browser
source links return to Zed through standard `window/showDocument` requests.
The browser is the complete workspace UI; continuous editor viewport tracking
is outside this integration. Migration is an explicit server command invoking
the shared CLI; the extension contains no legacy data reader or startup import.

Build and check independently:

```sh
cargo test --manifest-path adapters/zed/Cargo.toml --locked
cargo build --manifest-path adapters/zed/Cargo.toml --target wasm32-wasip2 --locked
cargo xtask check --root adapters/zed
```

The last command runs the repository's shared xtask against this independent
workspace and its own `dev/xtask/policy/workspace.toml`, generating tracked
adapter-local `architecture/` diagrams. The root workspace excludes this crate.
The policy models only `zed::register_extension!` as external generated ABI glue,
isolated in `src/registration.rs`. All handwritten logic in `src/adapter.rs` and
`src/release.rs` is analyzed normally, including module references and cycles.

API references: [published API](https://docs.rs/zed_extension_api/0.7.0/zed_extension_api/),
[LSP settings](https://docs.rs/zed_extension_api/0.7.0/zed_extension_api/settings/struct.LspSettings.html),
[extension development](https://zed.dev/docs/extensions/developing-extensions).
