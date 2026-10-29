# FlexiMark アーキテクチャ

FlexiMark は、純粋な文書処理、状態と native I/O をまとめた application、通信、エディター製品で構成する。VS Code と通常版 Zed は同じ native runtime を利用する。同じ状態や値型を層ごとに複製せず、一つの所有者から利用する。

Cargo の実依存関係は [生成された crate 図](../architecture/crates.svg)、crate 内の参照関係は [生成された module 図](../architecture/modules/) を参照する。Zed 拡張には [独立 workspace の図](../adapters/zed/architecture/crates.svg) がある。これらは `cargo xtask check` が生成する追跡対象である。

## 所有者と依存方向

| 配置 | 責務 |
| --- | --- |
| `core/model` | 文書 IR、NodeId、UTF-8 byte 範囲、共有 SourceIndex、モデル検証 |
| `core/parser` | comrak の解析結果から検証済み文書 IR を生成 |
| `core/render-html` | IR から安全化した HTML、ノード一覧、navigation を生成 |
| `core/engine` | 全文解析、NodeId 照合、候補の反映、描画 snapshot |
| `core/language` | 補完候補と構文の意味分類。LSP の数値 kind や相対 token 配列は生成しない |
| `application` | 文書・workspace・preview の権威状態、更新と公開、ファイル・設定・theme・note・アセット・export の native 実装と型付き操作結果 |
| `runtime/preview-assets` | インストール済み runtime manifest と browser 資源の検証・読込み |
| `contracts/distribution` | 配布 target・ファイル・サイズ上限の正本と、Rust 共通の runtime manifest 検証 |
| `transports/lsp` | 標準 LSP、FlexiMark の preview/workspace 契約、application と外部 DTO の変換 |
| `transports/preview-http` | loopback HTTP、SSE、browser navigation、workspace UI の要求配送 |
| `apps/server` / `apps/cli` | `fleximarkd` / `fleximark` の composition root |
| `contracts` | protocol、wire、config、生成 TypeScript・Schema、配布形式 |
| `web/preview` / `web/workspace-ui` | 共通描画クライアントと note・theme・export 等のブラウザ操作画面 |
| `adapters/vscode` / `adapters/zed` | editor API、言語登録、接続、起動・配布と製品固有 UI |
| `migration` | 明示操作でのみ使う旧ユーザーデータの読取り・変換 |
| `tooling` / `dev/xtask` | codegen、build・検証・配布タスク、構造検査 |

core は concrete runtime、protocol DTO、filesystem、browser bundle に依存しない。内部ソース範囲は UTF-8 byte offset だけを保持し、行・文字位置は共有 SourceIndex から通信境界で生成する。文書 counter は native `u64` で扱う。JavaScript の数値制約、JSON Schema、外部 DTO は contracts と transport の所有である。

完全 frame 配送の wire protocol と config schema は version 1 である。リリースまでは形式変更でも version を上げず、旧形式を境界で拒否する。内部 IR に交換用の document schema version は置かない。拡張構文は組込みコードで実装し、動的なプラグイン機構は持たない。

application は workspace 所属・trust・設定と文書状態を管理し、native 操作の型付き結果を返す。外部向けの message と JSON 応答は transport が組み立てる。CLI と daemon は共通の native 処理を利用する。中断 token は application が所有し、描画前と候補反映前、export の各段の境界で検査する。

## 起動と文書同期

公開エラーは `ApplicationError` に統一し、`ErrorKind` が NotFound・Denied・Conflict・Invalid・Cancelled・Internal を区別する。下位の I/O・config・engine・parser の原因を保持する。RPC の数値コードは protocol の `error_code` に集約し、transport が kind から変換する。共通 logger は URI やエラー詳細をそのまま出力せず、分類と hash を記録する。

`fleximarkd lsp` は標準入出力の Content-Length framing を使う。公開の起動モードは LSP で、独立した旧 RPC 文書管理モードはない。ログは標準エラーへ出力する。

接続は標準 LSP `initialize` 一回で成立する。`initializationOptions.fleximark` に workspace ごとの URI・trust と追加能力を渡す。通常の `workspaceFolders` / `rootUri` も使え、明示されない trust は false になる。応答の `capabilities.experimental.fleximark` が daemon identity、現在の protocol 能力、workspace 状態を返す。その後は標準の `initialized` 通知を送る。

文書本文の権威経路は `textDocument/didOpen`、`didChange`、`didClose` だけである。本文は全文同期し、再接続では現在の本文を開き直す。位置符号化は UTF-8、UTF-16、UTF-32 を扱い、共有 SourceIndex が LSP の交渉した単位と UTF-8 offset を変換する。保存前の編集も同じ文書状態を更新する。

LSP transport は補完、Semantic Tokens、hover、document symbols、diagnostics、Code Action、executeCommand を配送する。core/language の絶対位置・意味分類を LSP の kind と差分 token 配列へ変換するのは transport である。

## 文書処理と preview の権威状態

文書更新では次の順序を保つ。

1. 全文解析とモデル検証を行う。
2. 既存 IR と照合して NodeId を維持し、成功した候補だけを反映する。

core の `RenderSnapshot` は document version、ブロック HTML、style、assets、navigation、renderer fingerprint を持つ。daemon ID、preview ID、render revision、購読状態を含まない。

application が immutable な snapshot を `PreviewSessionId` と単調な revision で包み、preview ごとに最新の完全な `RenderFrame` を保存する。初回と変更時に完全 frame を配送し、読取りは revision を進めない。描画に失敗した場合、中断された場合、revision が上限に達した場合は前の frame を保持する。文書を閉じるとキャッシュを破棄する。再設定しても preview identity と revision の系列を維持する。

navigation は daemon identity、文書 session、文書 version、preview revision と NodeId を照合する。元ソース変更後の frame による古い移動要求は拒否する。source-to-preview の明示移動と browser-to-source の移動には同じ対応表を使う。

## エディター製品とブラウザ

VS Code は native Webview の埋め込みプレビューと外部ブラウザの両方を持つ。`adapters/vscode/webview` が VS Code API との bridge を所有し、共通の `web/preview` はエディター API を import しない。

表示側は変更のない top-level DOM を保持し、新しい DOM だけを強化する。強化済み記録は HTMLElement にひもづけ、NodeId を描画完了の証拠に使わない。非同期処理と audio/dialog の寿命は対応 DOM と画面の破棄に従う。VS Code の安定した view 記録には交換可能な接続 binding を持たせ、session の切替時に binding ごと交換する。

VS Code の文書 runtime は workspace を任意とし、単独の Markdown ファイルや未保存文書にも同じ LSP 同期・preview・再接続処理を使う。workspace に属さない文書の親 directory を workspace として登録せず、設定・plugin・local asset の権限も推測しない。制限モードでも本文のプレビューは利用でき、workspace 書込みや明示移行は trust を必要とする。

通常の相対リンクは拡張子に関係なく保持する。描画ポリシーで許可できないリンク先は active な `href` を出力せず、ラベルと本文を描画する。リンク先の拒否だけで文書全体の描画を失敗させない。画像の解決済み asset 対応表は hyperlink へ適用しない。

Zed は通常版の公式 editor と外部ブラウザを使う。Code Action が URI・version・range を渡し、`workspace/executeCommand` が操作を実行する。プレビュー URL とソース範囲は標準 `window/showDocument` で開く。選択位置からプレビューへの明示移動、browser からソースへの移動、未保存本文の更新を扱う。Zed の継続的な editor viewport 追跡はこの製品契約に含めない。

共通 workspace UI は文書・workspace の選択、note のカテゴリ・template・名前、theme、初期化、admonition 収集、export のフォームを提供する。文書 context がない command は開いている文書・workspace を画面で選択させる。

HTTP は `127.0.0.1` の動的 port に bind し、page token、Host、Origin を検証する。SSE は初回・再接続と更新時に完全 frame を配送し、対応する navigation の順序を維持する。外部 browser は daemon が動く host の loopback に到達する必要がある。これは公開 HTTP service ではない。

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

通常の設定は `contracts/config` の現形式だけを読む。`.fleximark/config.toml` の semantic validation と、workspace trust・アセット containment の検証を実行する。WASM プラグイン設定は未知のフィールドとして拒否し、プラグインを読み込まない。export は浄化済みの portable render を組み立て、ownership と journal による transaction で保存する。

旧 JSON、旧 theme、旧 editor 設定は明示的な migration 操作だけで読む。CLI と editor の migration command は同じ `migration` library を直接利用する。通常起動で旧設定を探索・通知・import しない。移行は計画の JSON、dry run、入力 snapshot の再検査、backup と復旧可能な適用を提供する。旧 JavaScript plugin は実行せず、保持して無効化を報告する。

## 検証

`cargo xtask check` は pinned `tooling/xtask-core` を使い、依存 allowlist、module 参照の循環、Role、ファイルの code line 上限と生成図を検査する。すべての crate / module root に正確な `//! Role:` を付け、production と test の各ファイルは 1000 code lines 未満に保つ。検証コマンドと製品 smoke は [開発手順](DEVELOPMENT.md) を参照する。
