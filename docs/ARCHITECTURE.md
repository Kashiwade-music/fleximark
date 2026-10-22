# FlexiMark アーキテクチャ

FlexiMark は、エディターに依存しない文書処理、文書・プレビューのセッション管理、実行環境、通信、エディター製品をそれぞれ独立した所有者に分けている。VS Code と通常版 Zed は同じ native runtime を利用する。

Cargo の実依存関係は [生成された crate 図](../architecture/crates.svg)、crate 内の参照関係は [生成された module 図](../architecture/modules/) を参照する。Zed 拡張には [独立 workspace の図](../adapters/zed/architecture/crates.svg)、WASM fixture にも [独立 workspace の図](../tests/fixtures/plugin-component/architecture/crates.svg) がある。これらは `cargo xtask check` が生成する追跡対象であり、説明用の手描き図ではない。

## 所有者と依存方向

| 配置 | 責務 |
| --- | --- |
| `core/model` | 文書 IR、NodeId、UTF ソース位置、生成元情報とモデル検証 |
| `core/parser` | comrak の解析結果から検証済み文書 IR を生成 |
| `core/render-html` | IR から安全化した HTML、ノード一覧、navigation を生成 |
| `core/engine` | ソース前処理、解析、NodeId 照合、変換、描画 snapshot、export の処理段階 |
| `core/language` | 補完候補と構文の意味分類。LSP の数値 kind や相対 token 配列は生成しない |
| `core/plugin-api` | object-safe な `PluginExecutor`、中断 token、hook 値、診断、実行結果 |
| `application` | 文書・workspace・preview の権威状態、更新、再同期、revision、キャッシュ、I/O port と型付き操作結果 |
| `runtime/workspace` | ファイル、設定、theme、note、アセット解決、export transaction と port の native 実装 |
| `runtime/wasm-plugins` | 署名・manifest 検証、Wasmtime、権限、時間・memory・fuel・出力量制限 |
| `runtime/preview-assets` | インストール済み runtime manifest と browser 資源の検証・読込み |
| `transports/lsp` | 標準 LSP、FlexiMark の preview/workspace 契約、application と外部 DTO の変換 |
| `transports/preview-http` | loopback HTTP、SSE、browser navigation、workspace UI の要求配送 |
| `apps/server` / `apps/cli` | `fleximarkd` / `fleximark` の composition root |
| `contracts` | protocol、wire、config、plugin ABI、生成 TypeScript・Schema、capability と配布形式 |
| `web/preview` / `web/workspace-ui` | 共通描画クライアントと note・theme・export 等のブラウザ操作画面 |
| `adapters/vscode` / `adapters/zed` | editor API、言語登録、接続、起動・配布と製品固有 UI |
| `migration` | 明示操作でのみ使う旧ユーザーデータの読取り・変換 |
| `tooling` / `dev/xtask` | codegen、build・検証・配布タスク、構造検査 |

core は concrete runtime、protocol DTO、filesystem、browser bundle に依存しない。ソース位置や文書 counter は native `u64` で扱う。JavaScript の数値制約、JSON Schema、外部 DTO は contracts と transport の所有である。

application は `WorkspaceServices` と `PluginExecutor` を通じて実行を要求する。runtime は `CreatedNote`、`ThemeResource`、`ExportReceipt` などの型付き結果を返す。外部向けの message、URI、JSON 応答の組立ては transport が担当する。CLI と daemon の package 間に依存はない。

## 起動と文書同期

`fleximarkd lsp` は標準入出力の Content-Length framing を使う。公開の起動モードは LSP で、独立した旧 RPC 文書管理モードはない。ログは標準エラーへ出力する。

接続は標準 LSP `initialize` 一回で成立する。`initializationOptions.fleximark` に workspace ごとの URI・trust と追加能力を渡す。通常の `workspaceFolders` / `rootUri` も使え、明示されない trust は false になる。応答の `capabilities.experimental.fleximark` が daemon identity、現在の protocol 能力、workspace 状態を返す。その後は標準の `initialized` 通知を送る。

文書本文の権威経路は `textDocument/didOpen`、`didChange`、`didClose` だけである。プレビューの接続確認や checkpoint は version・hash を照合し、別の本文管理経路を作らない。位置符号化は UTF-8、UTF-16、UTF-32 を扱い、LSP が交渉した単位を application で UTF-8 offset に変換する。保存前の編集も同じ文書状態を更新する。

LSP transport は補完、Semantic Tokens、hover、document symbols、diagnostics、Code Action、executeCommand を配送する。core/language の絶対位置・意味分類を LSP の kind と差分 token 配列へ変換するのは transport である。

## 文書処理と preview の権威状態

文書更新では次の順序を保つ。

1. プラグインによるソース前処理。
2. 全文解析と、前処理 edit map から元ソースへの生成元範囲の復元。
3. 既存 IR と照合した NodeId の維持、モデル検証。
4. block 変換、document 変換、候補検証。
5. 成功した候補だけを `DocumentEngine` に反映。

core の `RenderSnapshot` は document version、ブロック HTML、style、assets、annotations、navigation、renderer fingerprint を持つ。daemon ID、preview ID、render revision、購読状態を含まない。

application が snapshot に `PreviewSessionId` と単調な revision を付け、preview ごとに最新の完全な `RenderFrame` を保存する。読取りは revision を進めず、プラグインも再実行しない。描画に失敗した場合、中断された場合、revision が上限に達した場合は前の frame を保持する。文書を閉じるとキャッシュを破棄する。再設定しても preview identity と revision の系列を維持する。

navigation は daemon identity、文書 session、文書 version、preview revision と NodeId を照合する。元ソース変更後の frame による古い移動要求は拒否する。source-to-preview の明示移動と browser-to-source の移動には同じ対応表を使う。

## エディター製品とブラウザ

VS Code は native Webview の埋め込みプレビューと外部ブラウザの両方を持つ。`adapters/vscode/webview` が VS Code API との bridge を所有し、共通の `web/preview` はエディター API を import しない。

Zed は通常版の公式 editor と外部ブラウザを使う。Code Action が URI・version・range を渡し、`workspace/executeCommand` が操作を実行する。プレビュー URL とソース範囲は標準 `window/showDocument` で開く。選択位置からプレビューへの明示移動、browser からソースへの移動、未保存本文の更新を扱う。Zed の継続的な editor viewport 追跡はこの製品契約に含めない。

共通 workspace UI は文書・workspace の選択、note のカテゴリ・template・名前、theme、初期化、admonition 収集、export のフォームを提供する。文書 context がない command は開いている文書・workspace を画面で選択させる。

HTTP は `127.0.0.1` の動的 port に bind し、page token、Host、Origin を検証する。SSE は更新を通知し、最新 frame と navigation 状態を取得できる。外部 browser は daemon が動く host の loopback に到達する必要がある。これは公開 HTTP service ではない。

## runtime と配布

version の正本は root の `release.toml`。native archive は同じ target の二つの binary と資源をまとめる。

```text
fleximark-<version>-<Rust target>.zip
├─ bin/fleximarkd[.exe]
├─ bin/fleximark[.exe]
├─ resources/preview/{browser-host.js,fleximark.css,katex.css}
├─ resources/workspace/{workspace.js,workspace.css}
└─ manifest.json
```

manifest は schema version、product version、Rust target、protocol version とファイルごとの SHA-256 を持つ。`runtime/preview-assets` は起動場所の `bin/..` を runtime root として扱い、開発時には `FLEXIMARK_RUNTIME_ROOT` で指定できる。資源の読取りでは許可リスト、配布ディレクトリ内への包含、hash を検証する。core/application の build・test に生成 JavaScript は不要である。

配布 target は Linux GNU、macOS、Windows MSVC の x64 / arm64。VS Code の target 名と Rust target の対応は配布 tooling が持つ。VSIX は組立済みの同一 native archive を取り込む。Zed は明示 binary path、worktree PATH、現在の release の順に選び、download した archive の manifest と hash を検証する。Zed 拡張自体は独立した Cargo workspace の WASM target である。

VS Code adapter は Node extension host から native process を起動する desktop 製品である。browser-only extension host で native runtime を実行する構成は持たない。Zed では worktree の shell environment を使って daemon を起動する。外部 UI は loopback URL を開くため、別 host の browser を使う場合は到達性を別途確保する必要があり、この実装は公開 URL や remote tunnel を生成しない。

## 設定、trust、移行

通常の設定は `contracts/config` の現形式だけを読む。`.fleximark/config.toml` の semantic validation と、workspace trust・plugin grants・アセット containment の検証を実行する。unsafe HTML plugin 出力は、安全な portable render と export 構成の後に明示的に許可される処理段階へ限定する。

旧 JSON、旧 theme、旧 editor 設定は `fleximark migrate` の明示実行だけで読む。`migration` crate への依存は CLI に限定し、editor の migration command も共有 CLI を起動する。通常起動で旧設定を探索・通知・import しない。移行は計画の JSON、dry run、入力 snapshot の再検査、backup と復旧可能な適用を提供する。旧 JavaScript plugin は実行せず、保持して無効化を報告する。

## 検証

`cargo xtask check` は pinned `tooling/xtask-core` を使い、依存 allowlist、module 参照の循環、Role、ファイルの code line 上限と生成図を検査する。すべての crate / module root に正確な `//! Role:` を付け、production と test の各ファイルは 1000 code lines 未満に保つ。検証コマンドと製品 smoke は [開発手順](DEVELOPMENT.md) を参照する。
