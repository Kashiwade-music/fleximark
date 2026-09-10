# FlexiMark New Architecture

## 1. Status

- Status: Proposed
- Target release: next major version
- Migration strategy: one-time replacement
- Primary implementation language: Rust
- Initial editor adapter: Visual Studio Code
- Future editor targets: Zed, Neovim, Helix, JetBrains IDEs, and other clients

This document defines the replacement architecture for FlexiMark. The current
TypeScript implementation is treated as a source of product requirements, not
as an API or output compatibility target.

## 2. Decision Summary

FlexiMark will be rebuilt as an editor-independent Rust engine with a stable,
versioned protocol. Visual Studio Code will become the first thin client of the
engine rather than the runtime that owns the product logic.

The new implementation will:

- break compatibility with the current internal APIs, generated HTML, settings,
  parser plugins, and extension implementation;
- replace the current implementation in one major-version release;
- avoid a transitional TypeScript core, compatibility layer, dual-write path,
  or old-engine fallback;
- parse Markdown into a FlexiMark-owned intermediate representation instead of
  making HTML the canonical result;
- expose standard editor features through LSP and FlexiMark-specific features
  through a separate JSON-RPC protocol;
- retain JavaScript only where it is intrinsically required in a browser, such
  as DOM updates, Mermaid, and ABC playback;
- defer a Zed adapter, while ensuring that adding one does not require changes
  to parsing, transformation, document state, or export logic.

## 3. Goals

### 3.1 Product goals

- Preserve the important user-facing capabilities of FlexiMark under a newly
  specified behavior contract.
- Support live preview, extended Markdown, HTML export, note operations,
  Mermaid, ABC notation, math, source mapping, and editor/preview navigation.
- Allow the same document engine to be used from editor extensions, a CLI,
  automated builds, and an external browser.
- Make editor support an adapter concern rather than a core concern.
- Make failure isolation, cancellation, restart, and observability explicit.

### 3.2 Architecture goals

- No editor SDK types in the Rust core or service APIs.
- No VS Code paths, settings, URI schemes, or lifecycle assumptions in the core.
- No parser-library AST types in public protocols.
- No HTML DOM structure as the identity of a document node.
- Stable source spans and node identities for navigation and incremental updates.
- Versioned wire protocols with capability negotiation.
- A sandboxed, editor-independent replacement for `parserPlugin.js`.
- Cross-platform native distribution with verifiable artifacts.

## 4. Non-goals

- Preserving byte-for-byte HTML output from the current implementation.
- Preserving the current MDAST/HAST transformation hooks.
- Running existing `parserPlugin.js` files.
- Preserving current internal module names or TypeScript APIs.
- Shipping a Zed extension in the first release.
- Implementing Mermaid layout or ABC audio rendering in Rust.
- Supporting a mixed old/new runtime after the major-version cutover.
- Hiding the fact that different editors expose different UI capabilities.

## 5. Constraints and Principles

### 5.1 One replacement, not a staged migration

Development has internal dependencies and parallel workstreams, but there is
only one product cutover. The new engine and new VS Code adapter ship together.
The old runtime is removed at that cutover.

The release must not contain:

- a switch between old and new renderers;
- a legacy JavaScript plugin host;
- a fallback to the current TypeScript conversion pipeline;
- two configuration sources with synchronization logic;
- compatibility normalization intended to reproduce legacy HTML.

### 5.2 Ports and adapters

The domain and application layers define ports. Editors, filesystems, browser
transports, plugin runtimes, and render targets implement adapters for those
ports. Dependency direction always points toward the domain.

### 5.3 Structured output before presentation

The canonical output of parsing and transformation is the FlexiMark Document
IR. HTML is one projection of that IR. This is required because future editors
may offer a native structured preview without offering a Webview.

### 5.4 Explicit capabilities

An editor adapter reports which events and surfaces it supports. FlexiMark must
degrade by capability rather than by editor-name conditionals.

## 6. System Context

```text
┌──────────────────────────────────────────────────────────────┐
│ Editor adapters                                              │
│                                                              │
│ VS Code now       Zed later       Other editors later        │
│ Webview/UI        LSP/native UI   LSP/custom integration     │
└────────────────────────────┬─────────────────────────────────┘
                             │ LSP + FlexiMark JSON-RPC
┌────────────────────────────▼─────────────────────────────────┐
│ fleximarkd                                                   │
│                                                              │
│ sessions  commands  plugin host  preview server  filesystem │
└────────────────────────────┬─────────────────────────────────┘
                             │ Rust APIs
┌────────────────────────────▼─────────────────────────────────┐
│ fleximark-core                                               │
│                                                              │
│ parser  Document IR  transforms  diff  renderers  source map│
└────────────────────────────┬─────────────────────────────────┘
                             │ Preview protocol
┌────────────────────────────▼─────────────────────────────────┐
│ Browser/Webview client                                      │
│ DOM patching  Mermaid  abcjs  preview-side interaction      │
└──────────────────────────────────────────────────────────────┘
```

## 7. Repository Layout

```text
crates/
  fleximark-model/          # Public domain types and Document IR
  fleximark-parser/         # Markdown and FlexiMark syntax parser
  fleximark-transform/      # Built-in semantic transformations
  fleximark-render-html/    # IR-to-HTML renderer
  fleximark-engine/         # Sessions, versions, caching, and patch generation
  fleximark-protocol/       # JSON-RPC DTOs and protocol versioning
  fleximark-plugin-sdk/     # Guest-facing plugin types and WIT contract
  fleximark-plugin-host/    # WASM/WASI plugin execution
  fleximark-lsp/            # Standard editor language features
  fleximarkd/               # Native service process
  fleximark-cli/            # User and CI command-line interface

adapters/
  vscode/                   # Thin TypeScript VS Code adapter

web/
  preview-client/           # Shared browser/Webview TypeScript client
  assets/                   # CSS, fonts, Mermaid, abcjs, KaTeX assets

schemas/
  config.schema.json
  protocol.schema.json
  plugin-manifest.schema.json

fixtures/
  parser/
  renderer/
  protocol/
  security/
```

The exact number of crates may be consolidated during implementation, but the
dependency boundaries defined here must be retained.

## 8. Core Domain Model

### 8.1 Document IR

The core owns a serializable, parser-independent intermediate representation.
Conceptually:

```rust
pub struct Document {
    pub schema_version: u32,
    pub document_version: u64,
    pub uri: DocumentUri,
    pub metadata: DocumentMetadata,
    pub blocks: Vec<Block>,
}

pub struct Block {
    pub id: NodeId,
    pub span: SourceSpan,
    pub kind: BlockKind,
    pub attributes: Attributes,
    pub children: Vec<Node>,
}

pub enum BlockKind {
    Paragraph,
    Heading { level: u8 },
    List,
    Table,
    Quote,
    CodeBlock { language: Option<String> },
    Admonition { kind: AdmonitionKind },
    Tabs,
    Details,
    Mermaid,
    AbcNotation,
    Math,
    Media,
    RawHtml,
    Plugin { namespace: String, name: String },
}

pub struct SourceSpan {
    pub byte_start: u64,
    pub byte_end: u64,
    pub start: SourcePosition,
    pub end: SourcePosition,
}
```

All protocol-visible types must own their data and be serializable. Arena
references and types from the selected Markdown parser must not escape the
parser crate.

### 8.2 Node identity

Every renderable block has a stable `NodeId`. IDs are used for:

- incremental preview patches;
- source-to-preview and preview-to-source navigation;
- selection highlighting;
- plugin diagnostics;
- cache keys.

Node identity must not depend on serialized HTML or sibling array indexes alone.
The engine should combine structural ancestry, semantic content, and source
location, then reconcile nodes between document versions. The implementation
must define deterministic behavior for inserted, deleted, moved, and duplicated
blocks.

### 8.3 Source mapping

Source mapping is part of the IR, not an HTML post-processing step. Positions
must be representable as both UTF-8 byte offsets and line/column coordinates.
Protocol boundaries must declare their position encoding.

## 9. Parsing and Transformation

### 9.1 Parser backend

The initial parser should use a maintained Rust CommonMark/GFM implementation
with source-position support. Comrak is the preferred starting point, but all
Comrak nodes must be normalized into FlexiMark IR immediately after parsing.

The parser must implement the new FlexiMark syntax specification for:

- GFM;
- front matter;
- math;
- admonitions;
- tabs;
- collapsible sections;
- YouTube embeds;
- Mermaid blocks;
- ABC notation blocks;
- titled and numbered code blocks;
- raw HTML according to the configured security policy.

Parser behavior is defined by FlexiMark tests and documentation, not by the
legacy unified/remark/rehype output.

### 9.2 Transformation pipeline

The transformation pipeline operates on the Document IR:

```text
source
  → preprocess plugins
  → Markdown parser
  → normalized Document IR
  → built-in semantic transforms
  → IR plugins
  → validation and diagnostics
  → renderer
  → target-specific postprocess plugins
```

Built-in transformations are deterministic and side-effect free. Filesystem or
network access must occur through service-layer capabilities, never implicitly
inside an IR transform.

### 9.3 Raw HTML

Raw HTML support must be an explicit configuration decision. Preview and export
may use different policies. Sanitization and Content Security Policy are
defense-in-depth measures; enabling raw HTML must not implicitly enable network,
filesystem, or plugin capabilities.

## 10. Rendering

### 10.1 Render targets

The renderer interface accepts a Document IR plus a target policy:

```rust
pub trait Renderer {
    type Output;

    fn render(
        &self,
        document: &Document,
        context: &RenderContext,
    ) -> Result<Self::Output, RenderError>;
}
```

Initial targets are:

- `HtmlPreview`: interactive Webview or browser output;
- `HtmlPortable`: self-contained or directory-based export;
- `JsonDebug`: inspectable IR and render metadata;
- `PlainText`: testing and accessibility fallback.

Future native editor renderers consume Document IR or a reduced structured
preview DTO rather than parsing generated HTML.

### 10.2 Special blocks

Mermaid and ABC are represented as typed IR nodes. The HTML renderer emits a
stable container and payload. The Web client owns Mermaid layout, ABC notation
rendering, audio playback, and rerendering after patches.

KaTeX may be rendered server-side or initialized in the Web client, but the
choice must be uniform for preview and export where their security policies
permit it.

### 10.3 Asset resolution

The core identifies asset references but does not create VS Code Webview URIs,
register Express routes, or copy files. The service resolves assets according
to a target-specific `AssetPolicy`.

Asset resolution must:

- canonicalize paths;
- enforce allowed workspace and configured asset roots;
- reject traversal outside those roots;
- define symlink behavior;
- distinguish local, data, and remote URLs;
- produce diagnostics for rejected assets;
- generate collision-free export names.

## 11. Document Engine

`fleximark-engine` owns live document state. Its principal operations are:

```text
open_document
change_document
close_document
render
render_patch
set_selection
set_viewport
export_document
collect_admonitions
plan_note_creation
```

Every mutation and render request carries a monotonically increasing document
version. Work for obsolete versions is cancelled when possible and its result
is always discarded.

### 11.1 Incremental updates

The engine produces IR-aware patches based on stable node identity:

```json
{
  "documentVersion": 42,
  "operations": [
    {
      "type": "replace",
      "nodeId": "block-a120",
      "content": {}
    }
  ]
}
```

Supported operations are initially `insert`, `remove`, `replace`, `move`, and
`setAttributes`. The Web client applies them to DOM nodes carrying the same
stable IDs. A client may request a full snapshot after a rejected patch or a
version gap.

## 12. Service Process

`fleximarkd` is the editor-independent native process. One executable may expose
multiple modes:

```text
fleximarkd lsp
fleximarkd rpc
fleximarkd serve <document>
```

The user-facing CLI may expose:

```text
fleximark preview <document> --open
fleximark render <document>
fleximark export <document>
fleximark init
fleximark create-note
fleximark collect-admonitions
```

The service owns:

- engine and document-session lifetime;
- protocol dispatch;
- plugin discovery and execution;
- configuration loading;
- authorized filesystem operations;
- HTTP/WebSocket preview transport;
- cache lifetime and invalidation;
- logging, cancellation, and recovery.

The daemon must not assume that an editor exists. All commands must also be
invocable from the CLI or an automated process.

## 13. Protocols

### 13.1 LSP

Use Language Server Protocol for capabilities already standardized by LSP:

- document open/change/close synchronization;
- completion;
- diagnostics;
- hover;
- document symbols;
- code actions;
- workspace configuration where appropriate.

This provides a low-cost integration path for future editors. Preview-specific
behavior must not be encoded as proprietary misuse of standard LSP messages.

### 13.2 FlexiMark protocol

Use versioned JSON-RPC over stdio for FlexiMark-specific operations:

- preview session creation and disposal;
- full render snapshots and patches;
- editor selection and viewport updates;
- preview scroll and selection events;
- theme and style changes;
- export and note operations;
- capability negotiation.

Example handshake:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "fleximark/initialize",
  "params": {
    "protocolVersion": 1,
    "client": { "name": "vscode", "version": "1.0.0" },
    "capabilities": {
      "embeddedHtml": true,
      "structuredPreview": false,
      "selectionEvents": true,
      "viewportEvents": true,
      "openExternal": true
    }
  }
}
```

Behavior must be selected from declared capabilities, never from the client name.

### 13.3 Preview transport

HTTP serves the preview shell and authorized assets. WebSocket or Server-Sent
Events carries live preview updates. The server must bind only to loopback,
request an OS-assigned port by default, and require an unguessable session token.

## 14. Plugin System

The existing JavaScript parser plugin contract is removed. New plugins use a
versioned WASM/WASI contract.

Initial hooks are:

```text
preprocess_source
transform_document
transform_block
postprocess_html
```

Plugins receive versioned, serializable DTOs rather than Rust parser types. A
plugin manifest declares capabilities explicitly:

```toml
[plugin]
id = "example"
api_version = 1

[capabilities]
read_workspace = true
write_workspace = false
network = false
environment = false
```

The host enforces:

- execution time and memory limits;
- capability-scoped filesystem access;
- network denial by default;
- deterministic invocation order;
- API-version validation;
- structured errors and diagnostics;
- cancellation at document-version boundaries.

A plugin failure must not terminate the daemon or corrupt the active document
session.

## 15. Configuration

The canonical workspace configuration moves out of `.vscode/settings.json`:

```text
.fleximark/
  config.toml
  theme.css
  plugins/
```

The configuration schema covers syntax, preview behavior, export behavior,
templates, categories, assets, security, and plugins. Editor settings contain
only adapter concerns such as:

- daemon binary path;
- preview column or pane placement;
- whether to start a preview automatically;
- adapter logging level.

The service loads and validates canonical configuration. Adapters do not
interpret domain configuration.

## 16. VS Code Adapter

The replacement VS Code extension remains TypeScript because it is an adapter
to the VS Code JavaScript API. It owns only:

- activation and command registration;
- Webview creation and disposal;
- editor, buffer, selection, and viewport event collection;
- QuickPick, input boxes, notifications, and localization;
- daemon launch, handshake, monitoring, and restart;
- mapping VS Code values to protocol DTOs;
- forwarding Webview messages.

It must not contain:

- Markdown parsing or transformation;
- completion catalogs or domain rules;
- HTML generation;
- AST or DOM diff algorithms;
- note naming or category logic;
- canonical configuration interpretation;
- direct plugin execution;
- a second preview HTTP server.

An automated dependency rule should prevent core modules from importing the
VS Code package.

## 17. Future Editor Adapters

No Zed adapter is part of the initial release. The architecture must nevertheless
support three possible classes of future editor:

1. LSP-only clients: completion, diagnostics, symbols, and code actions.
2. LSP plus external browser: full rendering through `fleximarkd serve`.
3. Rich clients: embedded HTML or native structured preview, selection, and
   viewport synchronization through the FlexiMark protocol.

A future adapter must not require new parser hooks or editor conditionals in
the core. If a new editor capability requires a new protocol feature, it must be
introduced through protocol versioning and capability negotiation.

## 18. Security Model

### 18.1 Workspace trust

The service treats workspace content as untrusted until the client explicitly
reports that the workspace is trusted or the CLI user opts in. Plugin execution,
raw HTML, workspace writes, and remote assets are gated independently.

### 18.2 Local preview server

The preview server must:

- bind only to loopback;
- use an OS-assigned port unless explicitly configured;
- authenticate requests with a random per-session token;
- validate `Origin` and use restrictive CORS behavior;
- set an explicit Content Security Policy;
- serve only canonicalized allowlisted paths;
- prevent directory listing and arbitrary file reads;
- expire routes when their preview session ends.

### 18.3 Export

Export writes first to a temporary location and commits completed output
atomically where supported. Existing output must not be recursively deleted
without validating the resolved target. Asset collisions and partial failures
must be reported explicitly.

## 19. Distribution and Lifecycle

Release artifacts are required for supported Windows, macOS, and Linux CPU
architectures. Each artifact must have a checksum and release manifest.

The VS Code adapter either ships the matching binary in a platform-specific
package or downloads a signed, checksummed artifact. The final choice must meet
marketplace size and supply-chain requirements.

The adapter must:

- verify protocol compatibility during initialization;
- detect daemon exit;
- restart with bounded backoff;
- restore open document sessions after restart;
- surface actionable errors without exposing secrets;
- terminate child processes when the editor session ends.

## 20. Observability and Performance

The daemon emits structured logs to stderr. Protocol stdout is reserved for
framed protocol messages and must never contain ordinary logging.

Performance budgets must be defined for:

- cold process start;
- first render for 1k, 10k, and 100k-line documents;
- keystroke bursts;
- incremental patch size;
- peak memory;
- Mermaid/ABC rerender frequency;
- plugin execution.

Tracing must correlate document URI, document version, parse, transform, render,
patch, and delivery without recording document content by default.

## 21. Verification Strategy

Legacy-output compatibility tests are intentionally excluded. Tests verify the
new specification.

Required coverage includes:

- parser conformance fixtures for every supported syntax;
- Document IR snapshots;
- source-span and node-identity property tests;
- the invariant `apply(old, diff) == new`;
- malformed syntax, Unicode, and large-document fuzzing;
- request cancellation and stale-version rejection;
- protocol schema and version-negotiation tests;
- daemon crash and restart recovery;
- WASM plugin capability, timeout, and memory-limit tests;
- path traversal, symlink, URL encoding, CSP, token, and CORS tests;
- preview-client DOM patch tests;
- Mermaid and ABC rerender tests;
- VS Code extension integration and Webview end-to-end tests;
- CLI tests on every supported operating system;
- install, update, checksum, and binary compatibility tests.

Files under `markdown_for_debug/` may become input fixtures, but expected results
must be authored from the new behavior specification.

## 22. Implementation Workstreams

The following workstreams produce one replacement release. They are not product
migration phases and do not ship independently as alternative runtimes.

### 22.1 Specification and contracts

- Define the FlexiMark syntax specification.
- Define Document IR and source-position rules.
- Define stable node identity and reconciliation behavior.
- Define configuration and plugin manifests.
- Define LSP and FlexiMark protocol boundaries.
- Define security and performance budgets.

### 22.2 Rust engine

- Implement parsing and IR normalization.
- Implement built-in transforms.
- Implement diagnostics and source mapping.
- Implement HTML and export renderers.
- Implement document sessions, caching, cancellation, and patches.
- Implement note, template, category, and collection operations.
- Implement the WASM plugin host and SDK.

### 22.3 Service and CLI

- Implement JSON-RPC and LSP transports.
- Implement preview HTTP/WebSocket transport.
- Implement authorized asset and filesystem services.
- Implement CLI commands.
- Implement logging, recovery, and lifecycle management.

### 22.4 Clients

- Rebuild the shared preview client around stable node IDs and patches.
- Integrate Mermaid, ABC, math, and media.
- Implement the thin VS Code adapter.
- Implement UI-to-protocol event mapping.

### 22.5 Release engineering

- Build and sign the platform matrix.
- Produce checksums and manifests.
- Package or download the correct daemon binary.
- Run cross-platform installation and update tests.
- Remove the old TypeScript runtime at cutover.

## 23. Acceptance Criteria

The replacement may ship only when all of the following are true:

- The VS Code adapter contains no domain parsing, rendering, diff, or note logic.
- `fleximark-core` builds and tests without VS Code, Node.js, browser, or network
  dependencies.
- The CLI can render and export without an editor.
- LSP document synchronization and completion work against the same engine used
  by preview.
- Webview and external-browser previews use the same HTML renderer and Web client.
- Selection and viewport synchronization are expressed through protocol DTOs.
- No `parserPlugin.js` execution path remains.
- Canonical configuration is stored under `.fleximark/`.
- A daemon restart restores open VS Code sessions without data loss.
- The security and performance test suites meet their defined budgets.
- Supported platform packages pass clean-install verification.
- The current TypeScript conversion and server implementation has been removed.

## 24. Principal Risks

### Parser and IR complexity

Custom directives, source spans, and stable identities may require parser-level
work beyond a library's public extension points. Mitigation: keep a strict
parser-to-IR boundary and specify syntax before implementation.

### Incremental identity instability

Poor node reconciliation will cause large patches, flicker, and broken scroll
mapping. Mitigation: property tests, edit-sequence fixtures, explicit fallback
to full snapshots, and performance budgets for patch size.

### Plugin ABI design

A prematurely broad ABI becomes difficult to evolve. Mitigation: expose small,
versioned DTOs, use capabilities, and keep parser internals private.

### Native binary distribution

Platform-specific executables add signing, packaging, and updater complexity.
Mitigation: one release manifest format, reproducible CI jobs, checksums, and
protocol compatibility checks.

### Local file exposure

Preview asset routing can expose files outside the workspace. Mitigation:
canonical path containment, loopback-only service, session tokens, CSP, and
dedicated adversarial tests.

### Scope concentration

A one-time replacement concentrates delivery risk. Mitigation: maintain one
architecture and one release target, but execute implementation workstreams in
parallel behind internal test gates. Do not reduce risk by reintroducing a
second production runtime.

## 25. Estimated Effort

For Rust core, daemon, LSP, custom protocol, new VS Code adapter, Web preview,
export and note functionality, WASM plugins, cross-platform packaging, security,
and end-to-end validation, the working estimate is 26–36 person-weeks.

The estimate does not include implementing or contributing a native preview API
to another editor. A future editor adapter should be a separate project whose
scope is limited to that editor's APIs and the existing FlexiMark protocols.

## 26. Final Architectural Rule

FlexiMark is an editor-independent document engine. Editors are replaceable
clients. HTML is a render target, not the document model. Visual Studio Code is
the first adapter, not the architecture boundary.
