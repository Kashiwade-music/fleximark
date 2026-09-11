# FlexiMark New Architecture

## 1. Status

- Status: Approved
- Target release: next major version
- Cutover strategy: clean-break replacement
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
  through distinct JSON-RPC method namespaces on one authoritative session
  connection;
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

### 5.1 One replacement, not a staged rollout

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
  capability-matrix.schema.json

capabilities/
  release-baseline.yaml     # Normative capability traceability matrix

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
    pub provenance: SourceProvenance,
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

pub struct SourceRange {
    pub byte_start: u64,
    pub byte_end: u64,
    pub start: SourcePosition,
    pub end: SourcePosition,
}

pub struct SourcePosition {
    pub line: u64,
    pub character: u64,
    pub encoding: PositionEncoding,
}

pub enum PositionEncoding { Utf8, Utf16, Utf32 }

pub enum SourceProvenance {
    Original {
        ranges: NonEmpty<SourceRange>,
        primary_range_index: u32,
    },
    Derived {
        ranges: NonEmpty<SourceRange>,
        primary_range_index: u32,
        transform: TransformId,
    },
    Generated {
        anchor: Option<GeneratedAnchor>,
        transform: TransformId,
    },
}

pub struct GeneratedAnchor {
    pub range: SourceRange,
    pub affinity: AnchorAffinity, // Before or After
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
blocks. Reconciliation is one-to-one: an old ID may be assigned to at most one
new node and a new node may inherit at most one old ID. Exact provenance and
semantic matches take precedence over similarity. If two or more candidates
remain equally ranked, the match is ambiguous; none inherits the old ID and all
ambiguous new nodes receive deterministic fresh IDs. In plugin guest DTOs, an
existing node's `NodeId` is an immutable reference. A newly created node carries
only an invocation-local `CreationKey`; after candidate validation, the host
assigns/reconciles its NodeId and replaces creation-key references. Unknown or
duplicate existing IDs, reused creation keys, and attempts to edit an ID reject
the candidate. These rules prevent duplicated blocks or plugin output from
aliasing the same preview DOM element.

### 8.3 Source mapping

Source mapping is part of the IR, not an HTML post-processing step. Every range
is a zero-based, half-open interval `[start, end)` in the original document
snapshot. `byte_start` and `byte_end` are UTF-8 byte offsets. `SourcePosition`
stores a zero-based line plus a character offset tagged with its encoding.

LSP position negotiation follows the LSP `positionEncoding` capability. The
service converts negotiated LSP positions (normally UTF-16 code units) to and
from canonical UTF-8 byte offsets at the protocol boundary; byte offsets must
never be interpreted as Unicode scalar, UTF-16, or grapheme indexes. Invalid
positions, including offsets inside a code point or surrogate pair, are
rejected. Each immutable source snapshot owns a line index used for these
conversions.

`Original` means the IR content maps directly to one or more original ranges.
`Derived` means a transform combined or rewrote content while retaining all
contributing original ranges. Both variants require a non-empty range list and
an in-bounds `primary_range_index`. Ranges are ordered, may be non-contiguous,
and must not be collapsed to a single covering range. Diagnostics and
preview-to-editor navigation use the declared primary range while retaining the
full range set in protocol DTOs.

`Generated` has no source text. With an anchor, preview-to-editor navigation
resolves to a zero-width caret at the anchor start for `Before` and anchor end
for `After`. Without an anchor, the node itself is non-navigable; the client may
use the nearest ancestor with provenance, and otherwise emits no editor
navigation event. Diagnostics for an unanchored generated node attach to the
responsible transform/plugin configuration rather than inventing a source
range.

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
original source snapshot
  → provenance-preserving source preprocess plugins
  → Markdown parser
  → normalized Document IR
  → built-in semantic transforms
  → IR plugins
  → candidate validation and host NodeId assignment/reconciliation
  → IR diagnostics
  → target-specific structured render extensions
  → identity validation
  → renderer
  → final target-policy validation and sanitization
```

Built-in transformations are deterministic and side-effect free. Filesystem or
network access must occur through service-layer capabilities, never implicitly
inside an IR transform.

A source preprocessor does not accept and return an untracked string. It returns
the derived UTF-8 text and an edit map whose segments map half-open byte ranges
in that text to zero, one, or multiple ranges in the input snapshot. Insertions
are explicitly `Generated`; replacements and combinations are `Derived` and
retain every contributing origin. Edit maps compose after every preprocessing
step, so the parser and all later transforms see provenance resolved to the
original document snapshot. The host validates maps for ordered output
coverage, range bounds, valid UTF-8 boundaries, and declared generated gaps. An
invalid or incomplete map fails that plugin invocation; the host never guesses
positions.

Built-in and plugin IR transforms must propagate provenance when they preserve,
split, merge, reorder, or synthesize nodes. Validation rejects a renderable node
without valid provenance. Diagnostics produced against derived text are mapped
back through the composed edit map and may report related ranges when more than
one original range contributed.

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

### 10.4 Render integrity boundary

Live preview has no arbitrary HTML-string postprocessing hook. Preview plugins
may contribute only versioned structured render annotations or typed IR nodes;
the host validates their schema and provenance before the renderer assigns
stable `data-fleximark-node-id` attributes. Plugins cannot create, remove,
duplicate, or edit those identity attributes. Each extension receives an
isolated value and produces a candidate value; it cannot mutate the preceding
validated model, session IR, or render cache.

After the renderer, the preview sanitizer and target policy operate on another
isolated candidate as the final content-changing stages. The host parses or
retains that result as a final DOM model and verifies NodeId uniqueness, the
expected identity set, parent/child relationships, protected identity
attributes, and the single allowed root. Only this validated final DOM model may
be diffed, cached, or published.

If an optional structured extension produces an invalid candidate, the host
discards it, emits a diagnostic, and rerenders from the immediately preceding
validated model; it does not repair or partially reuse the candidate. A
required extension violation, or a failure in the renderer/final sanitizer
itself, fails closed for that render and publishes neither patch nor snapshot.
If final DOM validation implicates an optional extension, the host may retry
once with that extension removed, again starting from its immediately preceding
validated model. A successful retry is published only as a full snapshot.

Portable export may optionally enable `unsafe_export_html` for a plugin whose
manifest declares the separate `unsafe_html_output` capability and whose
workspace has explicitly granted it. The hook chain is the sole exceptional
final content stage: it receives an already rendered, sanitized, validated, and
serialized safe export, and no content-changing stage runs after it. Only text
encoding, byte/size limits, and output-file integrity are validated afterward;
the resulting HTML carries no FlexiMark safety claim and is explicitly marked
unsafe. This hook is disabled by default and never used for live preview. It
runs in an ephemeral export job over a serialized copy and cannot write to the
document session, preview IR, renderer cache, fingerprint, revision history,
snapshot, or patch stream. Unsafe export output is never accepted as a future
preview input. In safe export, structured extensions run before the final
sanitizer and `unsafe_export_html` does not run.

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

### 11.1 Authoritative sessions and versions

Within one daemon instance and control connection, each open URI has exactly one
authoritative `DocumentSession`: source text, negotiated position encoding,
document version, configuration generation, plugin set, parsed IR, and render
caches. The editor adapter never maintains a competing parse or render state.
LSP and FlexiMark methods on that connection resolve the same opaque
`documentSessionId`; URI alone is not a session capability. A standalone `rpc`
or `serve` process is an independent authority. Session IDs, versions, requests,
operations, and patch bases never cross daemon-instance or control-connection
boundaries, even when the normalized URI is identical.

The client supplies monotonically increasing document versions on open and
change. LSP versions need not be consecutive, so a jump alone is never treated
as a delivery gap. The daemon applies each ordered incremental notification to
its current snapshot, applies that notification's edits atomically after
position conversion, and advances to the supplied version. A stale version,
invalid range, or content-hash checkpoint mismatch marks the session out of
sync and emits `fleximark/requestFullText`; because `didChange` is an LSP
notification, it is not treated as if it had a response. Until a full-text
replacement establishes a new version and hash, dependent requests fail with
`ContentModified` and no render is published. Closing a document invalidates
its session ID and every associated preview session.

A `PreviewSession` is a target-specific projection of one document session. It
owns its own monotonic `renderRevision`, starting with the first full snapshot.
Theme, style, configuration, plugin-set, renderer-version, asset-policy, or
security-policy changes may advance `renderRevision` without changing
`documentVersion`. Their canonical serialization is hashed into a
`rendererFingerprint`. A fingerprint change is a hard diff boundary: the
engine must publish a full snapshot under the new fingerprint and must never
generate a patch across the change.

### 11.2 Incremental updates

The engine produces IR-aware patches based on stable node identity:

```json
{
  "previewSessionId": "preview-f82c",
  "documentVersion": 42,
  "baseRenderRevision": 17,
  "resultRenderRevision": 18,
  "baseRendererFingerprint": "sha256:...",
  "resultRendererFingerprint": "sha256:...",
  "operations": [
    {
      "type": "replace",
      "nodeId": "block-a120",
      "parentId": "document-root",
      "content": {},
      "precondition": {
        "nodeExists": true,
        "currentParentId": "document-root"
      }
    }
  ]
}
```

Supported operations are initially `insert`, `remove`, `replace`, `move`, and
`setAttributes`. `setAttributes` accepts only the protocol's presentation
attribute allowlist; identity, style, event-handler, `srcdoc`, and URL-bearing
attributes are forbidden. For a patch,
`baseRendererFingerprint == resultRendererFingerprint` is mandatory.
`insert` and `move` always name `parentId` and exactly one
insertion anchor: `beforeId`, `afterId`, or the explicit `atEnd: true`.
`remove`, `replace`, `move`, and `setAttributes` require the target to exist and
include its expected parent; operations may add content-hash or attribute-hash
preconditions where needed. Inserted subtrees must contain globally unique
NodeIds.

A patch is an ordered but atomic transaction. Before changing the DOM, the Web
client verifies the preview session, exact base render revision, renderer
fingerprints, and initial NodeId uniqueness, then simulates the operations on a
shadow model. Each operation's target, parent, anchor, and preconditions are
resolved against the shadow state immediately before that operation, including
the effects of preceding operations, rather than always against the base tree.
It changes the live DOM and advances to `resultRenderRevision` only if the full
simulation succeeds; otherwise it applies none. Events referring to an older
revision are ignored. Gaps, duplicate or missing IDs, unknown anchors, failed
preconditions, session changes, unequal or unexpected fingerprints, or
interrupted application require a full snapshot. A full snapshot declares the
preview session, document version, result render revision, and fingerprint, and
replaces the complete preview state atomically. When the expected fingerprint
changes, the client rejects patches until it receives that new fingerprint's
full snapshot.

## 12. Service Process

`fleximarkd` is the editor-independent native process. One executable may expose
multiple modes:

```text
fleximarkd lsp              # one editor connection: LSP + fleximark/* methods
fleximarkd rpc              # standalone FlexiMark RPC, with its own document sync
fleximarkd serve <document>
```

The VS Code adapter starts exactly one `fleximarkd lsp` process. LSP and
FlexiMark-specific messages share that process, one `Content-Length` framed
JSON-RPC stream, one initialization lifetime, and one document-session
registry. The adapter must not start a companion `rpc` process. `rpc` and
`serve` are standalone entry points for clients that do not use LSP; each owns
its own sessions and must receive full document content through its protocol.
Sessions and incremental patch bases are never shared between daemon processes.

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
All LSP document synchronization is dispatched into the authoritative session
registry described in Section 11.1. The negotiated LSP position encoding is
recorded on that connection and used for every incremental edit; FlexiMark
requests name the resulting document session and expected document version.
After `textDocument/didOpen`, a rich client sends a
`fleximark/attachDocument` request containing the URI, expected LSP version, and
UTF-8 content hash. Ordered dispatch guarantees that `didOpen` is processed
first. The successful response returns the opaque `documentSessionId`, accepted
version, and daemon-computed hash; `didOpen` itself remains a notification and
does not pretend to return an ID.

### 13.2 FlexiMark protocol

Use versioned `fleximark/*` JSON-RPC methods for FlexiMark-specific operations:

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
In `lsp` mode this handshake follows successful LSP initialization on the same
framed stdio connection and returns a random `daemonInstanceId`. In standalone
`rpc` mode the same handshake initializes the connection and the FlexiMark
protocol provides full-text open/change/close methods. A FlexiMark request is
rejected unless its `documentSessionId`, expected `documentVersion`, and daemon
instance all match the authoritative registry. There is no cross-process lookup
by URI and no independent preview copy of an unsaved buffer.

The VS Code adapter sends `fleximark/checkpointDocument` with the session ID,
latest document version, and UTF-8 content hash after a coalesced edit burst and
before a render/export that requires a new version. Version jumps remain valid;
the hash detects divergence. A standalone `rpc` incremental change additionally
declares `baseDocumentVersion` and `baseContentHash`. Any mismatch requests or
requires a full-text replacement rather than attempting to merge unknown state.

### 13.3 Preview transport

HTTP serves the preview shell and authorized assets. WebSocket or Server-Sent
Events carries live preview updates. The server must bind only to loopback,
request an OS-assigned port by default, and require an unguessable session token.
The browser transport attaches to an existing `PreviewSession` in the same
daemon; it does not open or mutate a document. Browser selection and viewport
events include the preview session and render revision and are routed to that
session's authoritative document state. The WebSocket is only a delivery and
interaction transport, not a second version authority.

### 13.4 Restart and replay

Daemon process state is disposable; the editor buffer is the recovery source of
truth. After a daemon exit, the adapter starts one replacement process with
bounded backoff, performs LSP and FlexiMark initialization, replays canonical
workspace configuration, and sends a full-text `didOpen` for every currently
open FlexiMark buffer with its current editor version. For each buffer it then
calls `fleximark/attachDocument` with that version and full-text hash, and waits
for the new document session ID before recreating preview sessions and
requesting full snapshots. Only after that barrier may it resume incremental
`didChange` and UI events; edits received during recovery are coalesced into a
newer full-text replacement or queued in version order.

Document session IDs, preview session IDs, render revisions, session tokens,
and patches from the exited `daemonInstanceId` are invalid and must be dropped.
The adapter never attempts to restore a daemon cache or replay an old patch.
Crash/restart tests must cover unsaved buffers, edits during replay, multiple
open documents, stale messages from the previous process, and partial preview
reconnection.

## 14. Plugin System

The existing JavaScript parser plugin contract is removed. New plugins use a
versioned WASM/WASI contract.

Initial hooks are:

```text
preprocess_source
transform_document
transform_block
extend_render_model
unsafe_export_html
```

`preprocess_source` returns derived text plus the validated edit map specified
in Section 9.2. The two transform hooks consume and return typed IR with
provenance. Existing NodeIds are immutable references; new nodes use unique
invocation-local creation keys until the host validates the candidate and
assigns/reconciles IDs. `extend_render_model` is a structured, target-aware hook
that runs before identity validation and sanitization.
`unsafe_export_html` is the only raw HTML-string hook and is governed by the
unsafe export policy in Section 10.4; it is not a preview hook.

Plugins receive versioned, serializable DTOs rather than Rust parser types. A
plugin manifest declares capabilities explicitly:

```toml
[plugin]
id = "example"
api_version = 1
required = false

[capabilities]
read_workspace = true
write_workspace = false
network = false
environment = false
unsafe_html_output = false
```

The host enforces:

- execution time and memory limits;
- capability-scoped filesystem access;
- network denial by default;
- deterministic invocation order;
- API-version validation;
- structured errors and diagnostics;
- cancellation at document-version boundaries.

The host also validates composed source edit maps, provenance, schema limits,
and NodeId integrity at the boundaries described above. A capability grant does
not change hook ordering or permit a plugin to bypass final preview
sanitization.

Every hook invocation is a candidate transaction chained from the last
validated value. A later plugin sees only that value, never an invalid candidate
from an earlier plugin. For `required = false`, timeout, trap, malformed output,
invalid edit map/provenance, or policy violation discards the candidate, emits a
structured diagnostic, and continues the chain from the prior validated value.
For `required = true`, the same condition fails the entire parse/transform or
render operation closed for that document version; no downstream plugin runs
and no partial result is committed or published. A plugin failure must not
terminate the daemon or corrupt the active document session.

## 15. Configuration

The canonical workspace configuration moves out of `.vscode/settings.json`:

```text
.fleximark/
  config.toml
  theme.css
  plugins/
```

The canonical configuration contains notes, assets, security, and plugins.
Syntax is a versioned built-in contract rather than a workspace setting. Preview
layout remains an adapter setting, while export destination and operation are
command parameters. Preview and export raw-HTML policies are distinct fields in
the security section. Editor settings contain only adapter concerns such as:

- daemon binary path;
- preview column or pane placement;
- whether to start a preview automatically;
- adapter logging level.

The service loads and validates canonical configuration. Adapters do not
interpret domain configuration.

### 15.1 Clean-break cutover

This release has no legacy conversion command, first-run converter, compatibility
reader, or rollback path for earlier FlexiMark configuration. A workspace must be
initialized directly in the new layout. Only `.fleximark/config.toml`,
`.fleximark/theme.css`, and configured WASM plugins are recognized.

Earlier VS Code settings, `.fleximark/fleximark.json`, workspace/global legacy
CSS files, and JavaScript parser plugins are unsupported inputs. The adapter,
service, engine, and CLI do not discover, parse, copy, translate, execute, or
retire them. Users who need equivalent behavior must express it afresh through
the documented new configuration and WASM plugin contracts. This is an
intentional clean break for an unreleased replacement, not a compatibility or
data-conversion workflow.

## 16. VS Code Adapter

The replacement VS Code extension remains TypeScript because it is an adapter
to the VS Code JavaScript API. It owns only:

- activation and command registration;
- Webview creation and disposal;
- editor, buffer, selection, and viewport event collection;
- VS Code-only TextMate grammar and snippet contributions;
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

An export destination is managed only after FlexiMark creates an ownership file
named `.fleximark-export.json`. The file contains a format version, a random
destination ID, a monotonically increasing committed generation, the canonical
source/workspace and destination identities, a manifest of each generated
relative path with its type and content hash, and a cryptographic digest of that
canonical ownership payload. The service-owned export-target registry stores
the same destination ID, committed generation, and complete ownership-payload
digest. A destination ID alone is never authorization. A missing destination or
an empty destination may be initialized after explicit target validation. A
non-empty destination without a valid ownership file is unmanaged and export
must stop without changing it. Being inside the workspace is not proof of
ownership.

An ownership file is valid only when it is a regular, no-follow file; its schema
and format version are recognized; its canonical destination and
source/workspace identities match the current request; its destination ID
matches the service-owned export-target registry under `.fleximark/`; its
generation and canonical ownership-payload digest exactly match the registry's
committed values; and every manifest path is normalized, relative, non-empty,
traversal-free, unique under the target filesystem's case rules, and typed as a
regular file or directory.
Before any replacement, every previously managed regular file must still match
its recorded hash and every directory must still have the recorded identity.
Unknown fields required by a newer format, a copied marker, a missing registry
record, malformed paths, type changes, or hash/identity mismatches make the
destination unmanaged or conflicted and require an explicit adopt/reconcile
workflow; a marker/registry generation or digest mismatch without a matching
active recovery journal is a conflict, and ordinary export stops without any
change rather than repairing either side.

The ownership marker, transaction journal, and staging/backup control metadata
are reserved control paths. They are excluded from the generated-payload
manifest and its file list so the marker cannot hash itself. Their integrity is
protected separately by the registry's ownership-payload digest and the
journal's own digest and recorded file identities. Generated output may not use
or collide with a reserved control path.

For an owned destination, replacement may remove or overwrite only paths named
by the preceding manifest. User-added paths are preserved, and directories are
removed only when empty. The exporter copies every unmanifested path into the
new staging tree without following links and verifies its identity and content;
an unmanifested link, special file, collision, or concurrent change aborts the
export rather than dropping the path. Before planning and again immediately
before commit,
the exporter canonicalizes the destination and parent, rejects filesystem roots,
workspace roots, source trees, symlinks/reparse points in managed path
components, and any identity or containment change. File operations use
no-follow/handle-relative APIs where available so a path cannot be swapped to a
link between validation and use. A failed TOCTOU check aborts the export.

The exporter renders into a uniquely named sibling staging directory on the
same filesystem and validates the complete new manifest. It fsyncs staged file
contents, the manifest, and staging directory, then writes and fsyncs a durable
sibling transaction journal containing transaction ID, destination ID, exact
file identities, staging/target/backup paths, manifest hashes, and state. Two
ordinary renames are not described as atomic. The journal advances through
`Prepared`, `OldMoved`, `NewInstalled`, and `Committed`, and each transition plus
affected parent-directory metadata is durably flushed before the next step.
The registry update is a participant in this same recoverable transaction: the
journal records its previous and proposed generation/digest, and `Committed` is
not written until both the installed marker and registry entry are durable and
exactly match. A registry write is never performed as an unjournaled follow-up.

When the platform offers a verified atomic directory-exchange primitive, the
exporter may use it and record that fact in the journal. Otherwise it renames
the old destination to a unique backup, records `OldMoved`, renames staging to
the destination, verifies the new ownership/manifest, and records
`NewInstalled` and `Committed`. On service startup and before another export,
recovery must inspect the journal and recorded file identities to deterministically
finish installation and its registry update or restore both the backup and
previous registry generation/digest; it must not infer state from path existence
alone. If durability primitives required by this protocol are absent, export
fails before mutation rather than weakening the guarantee.

The backup and committed journal remain recoverable until the new output has
been opened and validated or the user explicitly discards them. Asset
collisions, preserved unmanaged files, partial failures, journal recovery, and
recovery instructions are reported explicitly; recursive deletion based only
on a resolved path is forbidden.

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
new specification. Excluding byte-for-byte legacy output does not permit a
user-facing capability to disappear without an explicit product decision.

### 21.1 Capability traceability release gate

Before implementation begins, the release baseline tag is inventoried into the
normative machine-readable capability matrix. Each atomic capability has one
stable, never-reused capability ID and exactly one record; records may not group
multiple IDs. Every contributed command, configuration key, documented feature,
syntax contribution, persisted workspace/global file, and observable workflow
maps to one such ID. Each record contains baseline evidence, one `Keep`,
`Change`, or `Remove` decision, normative specification references, an owner,
and named fixture/end-to-end test IDs. `Change` and `Remove` also require an
explicit clean-break rationale and release-note ID. CI fails on duplicate/missing IDs, unreferenced
inventory, stale specification/test links, or a failing required test. Product
and engineering owners approve the complete machine-readable matrix as a
release artifact.

The following Markdown table is a non-normative, grouped summary for readers;
it is not the release gate and does not replace one-record-per-ID entries:

| Current capability | Decision | Normative contract | Required verification |
| --- | --- | --- | --- |
| VS Code side-by-side live preview, browser preview, force reload, and scroll synchronization | Keep, with session/revision semantics changed | Sections 10, 11, 13, 16 | `e2e/preview-vscode`, `e2e/preview-browser`, `protocol/revision-resync`, `e2e/scroll-selection` |
| HTML export and asset copying | Keep, with new output/security contract | Sections 10.1, 10.3, 18.3 | `e2e/export-portable`, `security/export-ownership`, `security/export-toctou`, `e2e/export-rollback` |
| GFM, front matter, math, admonitions, tabs, details, YouTube, Mermaid, ABC, titled/numbered code blocks, syntax highlighting, and configured raw HTML | Keep, behavior respecified | Sections 9.1, 9.3, 10.2 | one parser and render fixture per syntax; `e2e/mermaid`, `e2e/abc-audio`, `security/raw-html` |
| Source/preview navigation and selection/viewport synchronization | Keep, protocolized | Sections 8.2, 8.3, 11.2, 13.3 | `property/source-provenance`, `protocol/navigation-ranges`, `e2e/scroll-selection` |
| Note creation using category trees, templates, filename prefix/suffix, and date/snippet variables | Keep under the new configuration only | Sections 11, 15, 15.1, 22.2 | `e2e/create-note-categories`, `e2e/create-note-template` |
| Collect admonitions | Keep | Sections 11, 22.2 | `fixture/collect-admonitions`, `e2e/collect-admonitions` |
| Workspace initialization | Use only schema-valid `config.toml` and `theme.css`; old markers are unsupported | Sections 15, 15.1 | `e2e/init-clean` |
| Workspace/global legacy CSS and its reset commands | Remove; author a new workspace `theme.css` | Sections 10.4, 15, 15.1 | `e2e/edit-theme` |
| JavaScript parser plugins and their open commands | Remove; configure a new WASM plugin explicitly | Sections 4, 14, 15.1 | `fixture/plugin-inventory`, `e2e/plugin-replacement` |
| Old preview-mode, fixed-port, and sync settings | Remove; use new adapter preferences and negotiated capabilities | Sections 13.3, 15, 15.1, 16 | `e2e/preview-preference` |
| Markdown/ABC TextMate grammar injection and snippets | Keep as VS Code-only presentation aids | Section 16 | `e2e/vscode-grammar-contributions`, `e2e/vscode-snippets` |

`Keep` promises capability continuity under the new behavior specification, not
legacy HTML equality. `Change` requires tests for the new behavior and a
release note. `Remove` requires a recorded clean-break rationale and release
note; it does not imply a compatibility reader or data conversion path.

### 21.2 Required coverage

Required coverage includes:

- parser conformance fixtures for every supported syntax;
- Document IR snapshots;
- source-span and node-identity property tests;
- preprocess edit-map composition, multi-origin provenance, generated nodes,
  Unicode encoding conversion, and ambiguous identity fixtures;
- the invariant `apply(old, diff) == new`;
- malformed syntax, Unicode, and large-document fuzzing;
- request cancellation and stale-version rejection;
- protocol schema and version-negotiation tests;
- daemon crash and restart recovery;
- single-authority, full-text replay, render-revision, patch atomicity, and
  forced-resynchronization tests;
- WASM plugin capability, timeout, and memory-limit tests;
- path traversal, symlink, URL encoding, CSP, token, and CORS tests;
- export ownership, unmanaged-directory refusal, manifest-limited replacement,
  TOCTOU, interrupted commit, backup, and rollback tests;
- forged manifest with a valid destination ID but mismatched payload digest,
  reserved-control-path/self-reference rejection, and registry-update crash
  recovery tests;
- preview-client DOM patch tests;
- Mermaid and ABC rerender tests;
- VS Code extension integration and Webview end-to-end tests;
- CLI tests on every supported operating system;
- install, update, checksum, and binary compatibility tests;
- clean-install tests proving unsupported old configuration is not read;
- capability-matrix completeness and row-to-test linkage checks.

Files under `markdown_for_debug/` may become input fixtures, but expected results
must be authored from the new behavior specification.

## 22. Implementation Workstreams

The following workstreams produce one replacement release. They are not product
transition phases and do not ship independently as alternative runtimes.

### 22.1 Specification and contracts

- Define the FlexiMark syntax specification.
- Define Document IR, provenance/edit-map, and position-encoding rules.
- Define stable node identity and reconciliation behavior.
- Define configuration and plugin manifests.
- Define the shared LSP/FlexiMark session authority, patch transaction, and
  restart/replay contracts.
- Define security and performance budgets.
- Freeze and approve the capability traceability matrix.

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
- Enforce the new configuration boundary without legacy readers.

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
- Enforce capability-matrix and clean-break release gates.
- Remove the old TypeScript runtime at cutover.

## 23. Acceptance Criteria

The replacement may ship only when all of the following are true:

- The VS Code adapter contains no domain parsing, rendering, diff, or note logic.
- `fleximark-core` builds and tests without VS Code, Node.js, browser, or network
  dependencies.
- The CLI can render and export without an editor.
- LSP document synchronization and completion work against the same engine used
  by preview.
- LSP and `fleximark/*` methods share one connection, daemon instance, document
  registry, and version authority; restart recovery passes full-text replay
  tests for unsaved buffers.
- Webview and external-browser previews use the same HTML renderer and Web client.
- Every patch is scoped by preview session, base/result render revisions, and
  renderer fingerprint, and failed preconditions trigger atomic resynchronization.
- Selection and viewport synchronization are expressed through protocol DTOs.
- Preprocessing and every IR transform preserve validated, original-snapshot
  provenance across Unicode, generated content, and multiple source ranges.
- No `parserPlugin.js` execution path remains.
- Canonical configuration is stored under `.fleximark/`.
- No legacy setting, CSS file, marker, or JavaScript plugin is discovered or read.
- Clean initialization and unsupported-old-layout behavior are tested.
- A daemon restart restores open VS Code sessions without data loss.
- Preview plugins cannot modify NodeIds or bypass final sanitization; unsafe HTML
  output is export-only and requires its independent capability grant.
- Export requires generation- and digest-bound marker/registry ownership,
  refuses unmanaged non-empty destinations, and passes forged-marker,
  reserved-control-path, symlink/TOCTOU, manifest-limited replacement,
  crash-recoverable commit, registry-update, and journal recovery tests.
- The approved normative machine-readable capability matrix is complete and
  every `Keep`/`Change` record references passing fixture and end-to-end test IDs.
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
