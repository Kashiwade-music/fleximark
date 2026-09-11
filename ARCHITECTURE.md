# FlexiMark architecture

This document records the architecture that is implemented in this repository. It is a
baseline for compatibility-preserving changes, not a description of a future design.
Capability-level ownership is machine-readable in
[`capabilities/feature-inventory.json`](capabilities/feature-inventory.json), and wire shapes
are defined in [`schemas/protocol.schema.json`](schemas/protocol.schema.json).

## Runtime composition and dependency direction

The VS Code extension is the normal composition root. One adapter process owns one
`fleximarkd lsp` child process for all folders in a VS Code window. The daemon owns the
authoritative document sessions and serves both embedded and external previews.

```text
VS Code
  -> adapters/vscode (window lifecycle, UI, commands, daemon lifecycle)
     -> fleximarkd lsp over Content-Length framed JSON-RPC/LSP
        -> fleximark-lsp (authoritative document/session registry)
           -> fleximark-engine (parse/transform/validate/render pipeline)
        -> fleximark_service (workspace commands, assets, export transactions)
        -> loopback preview HTTP/SSE server
           -> web/preview-client

fleximark CLI
  -> fleximark-engine + fleximark_service directly (no JSON-RPC transport)
```

The acyclic internal Rust crate graph below lists direct normal/runtime repository dependencies.
Third-party and test-only development dependencies are omitted; the latter add parser test
fixtures to `fleximark-plugin-host` and `fleximark-render-html` without changing the runtime DAG.

```text
fleximark-model
  <- fleximark-parser
  <- fleximark-render-html
  <- fleximark-plugin-sdk
  <- fleximark-protocol

fleximark-model + fleximark-plugin-sdk
  <- fleximark-plugin-host

model + parser + render-html + plugin-sdk + plugin-host
  <- fleximark-engine

model + render-html + plugin-host + engine + protocol
  <- fleximark-lsp

model + render-html + plugin-sdk + plugin-host + engine + lsp + protocol
  <- fleximarkd (the fleximark_service library and fleximarkd binary)

model + parser + render-html + plugin-host + engine + fleximark_service
  <- fleximark CLI
```

The TypeScript adapter imports protocol and RPC modules plus preview DTO types. The two preview
hosts import the shared preview document, host, navigation, enhancement, and runtime modules.
Neither the shared preview client nor the Rust core imports VS Code APIs.

## Component owners

| Owner | Source | Responsibility |
| --- | --- | --- |
| Adapter | `adapters/vscode` | VS Code activation, settings, commands, editor/workspace events, daemon recovery, panels and browser launch |
| Protocol | `crates/fleximark-protocol`, `schemas/protocol.schema.json`, `adapters/vscode/src/protocol.mts` | Protocol version, custom method names, JSON DTOs and stdio framing |
| Daemon | `crates/fleximarkd/src/main.rs` | LSP/custom RPC routing, cancellation, preview server and process-level composition |
| Service | `crates/fleximarkd/src/lib.rs` (`fleximark_service`) | Trusted workspace configuration, notes, themes, local assets and recoverable export filesystem transactions |
| LSP/session | `crates/fleximark-lsp` | URI-to-session authority, document versions, diagnostics/navigation and preview publication state |
| Engine | `crates/fleximark-engine` | Plugin-aware parse/transform/validation pipeline and full/patch render policy |
| Model | `crates/fleximark-model` | IR, node identity, source provenance and navigation data |
| Parser | `crates/fleximark-parser` | Markdown/Comrak AST to validated FlexiMark IR |
| HTML renderer | `crates/fleximark-render-html` | Safe HTML and render-model serialization |
| Plugin SDK/host | `crates/fleximark-plugin-sdk`, `crates/fleximark-plugin-host` | Manifest/WIT contract, package verification, Wasmtime sandbox and hook transactions |
| Preview client | `web/preview-client` | Atomic full/patch DOM application, navigation and opt-in enhancement runtimes |
| CLI | `crates/fleximark-cli` | Direct render, benchmark and workspace/service commands |
| Release | `scripts`, `.github/workflows`, `bin/manifest.json` | Build order, six-platform daemon assembly, checksums, VSIX validation and publishing |

The capability inventory is the detailed owner map. In particular, adapter settings remain in
`package.json`; note, asset, plugin, theme and export policy remain service-owned workspace
configuration; Markdown semantics remain engine-owned; Mermaid/ABC enhancement remains
preview-client-owned.

## Entry points

- `package.json` activates `dist/extension.cjs`, bundled from
  `adapters/vscode/src/extension.mts`, for Markdown documents or
  `.fleximark/config.toml` workspaces.
- `fleximarkd lsp` is the VS Code transport and combines standard LSP with `fleximark/*`
  methods. `fleximarkd rpc` exposes the custom protocol without LSP, and
  `fleximarkd serve <document>` starts a standalone loopback preview.
- The `fleximark` binary supports `render`, `benchmark`, `init`, `edit-theme`, `create-note`,
  `collect-admonitions`, `export`, and `ack-export`.
- `web/preview-client/vscode-host.mts` is bundled for the webview;
  `web/preview-client/browser-host.mts` is bundled for the loopback browser preview and embedded
  into the daemon.
- `mise.toml` is the developer-facing build/test/package entry point. `scripts/tasks.py` and
  `scripts/build.py` are the orchestration and JavaScript bundle entry points.

## Trust boundaries

1. **VS Code to daemon.** Stdio contains untrusted JSON. Messages use JSON-RPC 2.0 with LSP
   `Content-Length` framing and a 16 MiB daemon limit. Rust request DTOs reject unknown fields.
   The TypeScript connection parses envelopes but currently relies on requested generic types
   for result shapes; the protocol schema and characterization tests are therefore part of the
   compatibility boundary.
2. **Workspace authority.** The adapter forwards VS Code Workspace Trust for every root at
   initialization and reconfiguration. The daemon records per-root grants. Workspace writes,
   local asset reads and plugins are service-side operations and are constrained to the granted,
   canonical workspace. Symlink and path-escape checks are security invariants, not adapter UI
   policy.
3. **Plugin packages.** A configured plugin is accepted only from `.fleximark/plugins` after
   manifest hash, WebAssembly hash and Ed25519 signature checks. Effective capabilities are the
   intersection of workspace configuration grants and the signed manifest. Wasmtime supplies the
   component sandbox; unsafe HTML is a separate explicit export-only capability.
4. **Daemon to embedded preview.** The adapter creates a CSP-restricted webview and a random
   message token. The host ignores messages with a different token. Rendered markup is still
   checked by the preview client before it is committed to the live DOM.
5. **Daemon to browser preview.** The server binds an ephemeral `127.0.0.1` port and uses an
   opaque preview token in the path. It checks request host/origin, sends CSP and no-store
   headers, publishes render events through SSE, and receives navigation events through
   POST requests whose `Origin` is one of the allowed loopback origins. The accepted host and
   origin are each allowlisted as `127.0.0.1` or `localhost`; they are not required to use the
   same spelling.
6. **Source and filesystem to rendered output.** Raw HTML policy is loaded from trusted workspace
   configuration. Local assets must remain under configured roots, are content-typed and become
   opaque content-hash references. Preview markup is applied to a detached clone and rejects
   executable scripts and protected attributes before commit.
7. **Release artifact to process execution.** The adapter selects the current platform/CPU entry
   from `bin/manifest.json`, confines its path to the extension, and verifies SHA-256 before
   launching the bundled daemon. An explicitly configured external daemon path is user-supplied
   and outside this checksum boundary.

The CLI deliberately bypasses the JSON-RPC and VS Code trust boundary. Its filesystem commands
still use `fleximark_service` validation and transaction rules; invocation by the local user is
the authority to perform them.

## Wire and ABI formats

| Format | Canonical location | Compatibility notes |
| --- | --- | --- |
| JSON-RPC/LSP stdio | `fleximark-protocol`, `schemas/protocol.schema.json` | Protocol version 1, UTF-8 JSON, `Content-Length: N\r\n\r\n`; custom names are `fleximark/*` and fields use the casing declared by serde/schema |
| Custom request/result DTOs | Rust protocol types, protocol schema, adapter types | Optional fields are omitted on serialization where declared; error codes/messages and result envelopes are external behavior |
| Preview publications | protocol schema and `web/preview-client/index.mts` | Discriminated `full`/`patch` JSON, camelCase fields, session/version/revision/fingerprint identity, navigation and optional style/assets |
| Embedded preview messages | `web/preview-client/host.mts` | `initializePreview` and `previewEvent` envelopes include a per-panel `messageToken`; client events return through VS Code webview messaging |
| Browser preview | `fleximarkd/src/main.rs`, `browser-host.mts` | Tokenized loopback HTTP page, SSE publication stream and JSON POST navigation events |
| Plugin manifest and ABI | `schemas/plugin-manifest.schema.json`, `fleximark-plugin-sdk/wit/fleximark-plugin-v1.wit` | TOML manifest `schema_version = 1`, plugin `api_version = 1`, versioned WIT world and signed artifact digest |
| Release manifest | `scripts/create_release_manifest.py` | JSON `schemaVersion: 1`, `protocolVersion: 1`, and platform/arch/path/SHA-256 entries for Linux, macOS and Windows on x64/arm64 |

The current custom method set is `initialize`, `attachDocument`, `checkpointDocument`,
`requestFullText`, `render`, `createPreview`, `disposePreview`, `setSelection`, `setViewport`,
`previewEvent`, `reloadPreview`, `executeCommand`, `getNoteOptions`, `reconfigureWorkspace`,
`openDocument`, `changeDocument`, and `closeDocument`, all in the `fleximark/` namespace.

## Persistent formats

FlexiMark has no database or database schema. Compatibility-sensitive filesystem formats are:

- `.fleximark/config.toml`, whose `schema_version = 1` shape is described by
  `schemas/config.schema.json`;
- `.fleximark/theme.css` and user Markdown/note files;
- plugin `.toml`, `.wasm`, and `.sig` files below `.fleximark/plugins`, including their hashes,
  signer key and capability grants;
- exported portable HTML/assets plus the destination `.fleximark-export.json` ownership marker;
- `.fleximark/export-targets/<destination-hash>.json` registry records and adjacent
  `.*.fleximark-export-journal.json` transaction journals. Staging and backup names are also part
  of crash recovery;
- generated `bin/manifest.json`, whose entries select and authenticate packaged daemon binaries;
- the repository capability inventory and clean-break catalog, which are source-controlled
  governance contracts rather than user runtime state.

Ownership markers, registry records and journals contain identity/digest/generation information.
They must only be advanced by the export transaction and recovery implementation; hand-editing
or partially copying them makes a destination unmanaged or conflicted.

## Cross-component invariants

- `SessionRegistry` is the authority for open document text, URI mapping, document session ID,
  daemon instance ID and current document version. Adapter replay after a crash creates a new
  daemon generation and reattaches all still-open roots/documents/previews.
- Document versions and render revisions do not move backwards. Stale or out-of-sync edits cause
  a full-text request; a patch is valid only for its stated preview session, base revision,
  renderer fingerprint and structural preconditions.
- Preview changes are transactional: validate against a detached DOM, resolve only declared
  assets, update navigation/style with the same revision, then commit. Any failed operation leaves
  the live DOM unchanged and requests/awaits a full publication.
- Node IDs, provenance and source navigation refer to the same validated IR. Plugin preprocess,
  parse, provenance remap, transform and validation complete before rendering/publication.
- Preview and normal export are safe by default. Portable safe HTML is composed before the
  explicitly granted unsafe export hook can run; unsafe plugin output never enters preview.
- Export replacement is an ownership-checked filesystem transaction. Marker, registry, journal,
  destination identity and digest/generation must agree, and recovery must not overwrite an
  unmanaged or externally changed destination.
- Workspace roots are independent trust/configuration domains even though one daemon serves a
  multi-root window. Removing one root must not dispose state belonging to another root.
- Public command IDs, extension activation/settings, protocol version and fields, CLI output and
  packaged runtime contents are compatibility surfaces. Move-only refactors must preserve them
  unless a separately approved clean break is recorded.

## Build and verification boundaries

The product build order is browser client, release daemon, platform staging, release manifest,
then production extension/preview bundles. Pure RPC/preview tests run in Node, while the VS Code
integration suite remains responsible for extension activation and editor/workspace behavior.
Python `unittest`, TypeScript checks, Rust tests/lints, architecture inventory checks, packaging
inspection and platform clean-install smoke tests cover the remaining boundaries.
