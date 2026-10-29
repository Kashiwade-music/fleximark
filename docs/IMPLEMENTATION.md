# 実装の所有境界

この文書は現在の実装境界と維持する不変条件を記録する。構成の全体説明は [ARCHITECTURE.md](ARCHITECTURE.md)、実行コマンドは [DEVELOPMENT.md](DEVELOPMENT.md) を参照する。

## 実装単位

- `core/{model,parser,render-html,engine,language}` は editor に依存しない文書処理を所有する。
- `application` は文書・workspace・preview の権威状態と native 操作を所有する。`application/src/native` は filesystem、設定、アセット、note、theme、export を実装する。
- application の workspace はプラグイン host を保持しない。設定と theme から render policy を作り、config の plugins は通常の未知フィールドとして拒否する。config schema は `contracts/config/schema.template.json` と config の version 定数から codegen で生成する。
- `runtime/preview-assets` は配布資源の実装を所有する。
- `transports/{lsp,preview-http}` は外部 DTO への変換と通信を所有する。
- `apps/{server,cli}` はそれぞれ `fleximarkd` と `fleximark` を組み立てる。
- `contracts/{protocol,config,wire,typescript,schemas,distribution}` は現在の公開形式、生成 binding と配布形式を所有する。
- `web/{preview,workspace-ui}` は共通描画と操作フォームを所有する。
- `adapters/{vscode,zed}` は各 editor 製品を所有する。
- `migration` は CLI と application の明示操作で使う旧ユーザーデータ移行だけを所有する。
- `tooling/{codegen,tasks,xtask-core}` と `dev/xtask` は開発・検証を所有する。`xtask-core` submodule は pin したものを使用する。

## 文書処理の境界

core の位置は UTF-8 byte 範囲だけを保持し、共有 SourceIndex が行・文字位置を変換する。位置と counter は native 値であり、JavaScript 制約や protocol Schema を持たない。protocol DTO は core/application/runtime の型を import せず、自身の外部表現を定義する。

文書更新は全文解析とモデル検証の後、既存 IR と照合して NodeId を維持し、成功した候補を反映する。block と inline は単一の UTF-8 byte 範囲を持つ。動的なプラグイン機構や変換元の edit map は持たない。

`DocumentEngine` は source、IR、描画設定を保持する。公開処理は `open/change/resync/set_render_config/render` に集約する。`RenderSnapshot` に preview framing はない。application が中断を確認し、候補の engine と frame を成功時だけ反映する。frame は immutable snapshot と原文を共有し、session identity と preview ごとの revision を持つ。読取りだけで revision を進めず、失敗・中断・counter 上限時に以前の公開結果を保持する。

`core/language` は補完の内部 enum と絶対 token span を返す。LSP の数値 kind、snippet format 値、相対 token 配列への変換は transport に閉じる。

## 接続と editor の境界

標準 LSP `initialize` が workspace roots、trust、位置符号化と追加能力を設定し、`capabilities.experimental.fleximark` へ初期状態を返す。本文の同期経路は標準 `didOpen/didChange/didClose` に一本化し、全文同期する。preview は初回と更新時に完全 frame を受け取り、通知後の read RPC は使わない。接続 binding の交換で古い非同期完了を排除する。

VS Code は埋め込み Webview と外部 browser を提供する。通常版 Zed は共通の外部 browser UI を使い、Code Action の URI・version・range、executeCommand、window/showDocument によってプレビューと workspace 操作を提供する。Zed では明示 source-to-preview と browser-to-source navigation を扱い、継続的な viewport 追跡は契約に含めない。

browser UI は context が足りない場合に文書・workspace を選択させ、note category/template/name と export destination を型付き操作へ渡す。application は `CreatedNote` や `ExportReceipt` などの outcome を返す。強化済みの表示状態は HTMLElement にひもづけ、保持した DOM の操作状態を毎回作り直さない。

## 配布とデータ移行

root の `release.toml` が製品 version を所有する。native archive は daemon、CLI、preview/workspace browser 資源、version・target・protocol・SHA-256 を記録した manifest を含む。source、`.build` の build output、`.artifacts` の製品 staging を分ける。root `package.json` は private tooling である。

旧ユーザーデータの decoder は `migration` crate に閉じる。CLI と editor の明示 migration command は同じ library を直接呼び出す。通常起動・設定読込みに旧形式の fallback、探索、移行 offer は置かない。

## 構造検査と回帰検証

`cargo xtask check` は明示した依存 allowlist、module の循環、Role と code line 上限を検査する。手書きロジックを隠す macro exemption は設けず、解析できる明示的な Rust 実装を用いる。

[crate 図](../architecture/crates.svg) と [module 図](../architecture/modules/) は xtask が生成する。Zed の独立 workspace にも同じ検査と [生成図](../adapters/zed/architecture/) がある。生成 SVG は source と一緒に更新して追跡する。

pure core の unit tests は Node output を必要としない。application の filesystem transaction、移行、LSP/HTTP、browser、adapter、配布 archive を対応する試験で検証する。ローカルの `check` と `test` を CI/release の `ci quality` が共用し、製品検証は `ci product` が担当する。全体の入口は `mise run verify`。コマンドの処理範囲は [DEVELOPMENT.md](DEVELOPMENT.md) を参照する。
