# 開発手順

リポジトリ直下で `mise run <操作> [対象]` を実行する。Yarn のラッパーも同じ処理を呼ぶ。
設定は [mise.toml](../mise.toml)、実装は [tasks.py](../tooling/tasks/tasks.py)。

## 初回の準備

```sh
mise install
mise run setup
```

`mise install` は固定バージョンの toolchain を導入する。
`setup` は固定 submodule、Yarn/Python の依存関係、rustfmt/clippy、wasm32-wasip2 を導入する。ネットワークが必要。

## コマンドと実行内容

| コマンド | 実行されること |
| --- | --- |
| `mise run help` | コマンド一覧。個別の引数は例: `mise run build --help` |
| `mise run build vscode` | native 本体・browser UI・VS Code 拡張をビルドして組み立てる |
| `mise run build zed` | native 本体・browser UI・Zed WASM 拡張をビルドして組み立てる |
| `mise run build runtime` | native daemon/CLI と browser UI をビルドして組み立てる |
| `mise run build core` | Rust の文書処理・application libraries を debug build する |
| `mise run build all` | runtime を一度ビルドして VS Code と Zed の両方を作る |
| `mise run watch` | VS Code 製品だけをビルドし、JS/TS の変更を監視・再ビルドする。起動は F5 |
| `mise run check` | 生成物・Rust 構造・型・lint・format・WASM compilation を検査する |
| `mise run test` | Python/tooling・Rust workspace/Zed/plugin fixture・Node のテストを実行する |
| `mise run test editor` | VS Code 製品をビルドして workspace/standalone/restricted の結合テストを実行する |
| `mise run package vscode` | VS Code 製品をビルドしてインストール用 VSIX を作る |
| `mise run package zed` | runtime と Zed 拡張をビルドして ZIP と SHA-256 を作る |
| `mise run package runtime` | runtime をビルドして ZIP と SHA-256 を作る |
| `mise run package all` | runtime・Zed ZIP と VS Code VSIX を作る |
| `mise run verify` | check → test → audit → 全製品 build → 性能・editor テスト → VSIX・identity → clean-install smoke |
| `mise run audit` | registry へ問い合わせて JavaScript 依存の high/critical 脆弱性を検査する |
| `mise run smoke <VSIXのパス>` | 既存 VSIX を隔離 VS Code に入れ、daemon 起動・本文描画・ライブ更新を実画面で検証する |
| `mise run clean` | `.build` 内の JS/TS・test 出力を削除する。`target` と `.artifacts` は残す |

`build` / `package` は対象指定が必須。ビルドはエディタを起動せず、`package` はテスト・公開を行わない。
runtime/Zed の ZIP は組立処理の一部なので `build` でも生成する。VSIX は `package` / `verify` で生成する。
`check` は検査用にコンパイルするが、test suite は実行しない。古い生成物は再生成したうえで失敗する。
`test` の省略時は `unit`（GUI なし）。`yarn test` も同じ Python/Rust/Node テストを呼ぶ。
`test editor` / `smoke` は GUI が必要で、初回は VS Code をダウンロードする場合がある。
`verify` は GUI・実 Chrome/Edge・ネットワークが必要。browser は `FLEXIMARK_BROWSER` で指定できる。
Linux の GUI 検証には `xvfb-run -a mise run verify` を使う。

各コマンドは最初に実行計画・出力先を表示する。末尾に `--dry-run` を付けると表示だけで終了する。
例: `mise run package vscode --dry-run`。処理を追加すると計画にも反映される。

## VS Code で動かす

1. このリポジトリを VS Code で開き、F5 → `Run Extension (extension workspace)` を選ぶ。
2. F5 が watch task を起動し、`FlexiMark VS Code watch ready` を待って開発用 VS Code を開く。
3. そのウィンドウで Markdown を開き、`FlexiMark: Preview Markdown` を実行する。
4. Rust/CSS を変更した場合はデバッグと **`mise: watch` task の両方を終了**して F5 をやり直す。自動監視は JS/TS のみ。

別 workspace を使う場合は `Run Extension` を選び、その絶対パスを入力する。監視単独の停止は Ctrl+C。
Zed は `mise run build zed` 後、Install Dev Extension で `adapters/zed` を選び、`lsp.fleximark.binary.path` を組立済み daemon へ向ける。

## 出力先

| パス | 内容 |
| --- | --- |
| `target/debug/` | Rust core libraries |
| `target/<Rust target>/release/` | native daemon / CLI |
| `target/zed/wasm32-wasip2/release/` | Zed WASM 拡張 |
| `.build/` | JS/TS・test 出力。F5 の拡張は `.build/vscode/product/` |
| `.artifacts/<version>/<Rust target>/` | `runtime/`・`vscode/`・`zed/` の組立結果、ZIP・checksum・VSIX |

version の正本は `release.toml`。ビルド対象は現在の OS/CPU。daemon の手動起動は組立済み `runtime/bin/fleximarkd lsp`。
直接ビルドした binary を使う場合は `FLEXIMARK_RUNTIME_ROOT` に組立済み runtime の絶対パスを指定する。

## 生成物と CI

`mise run generate <対象>` で更新する。対象は `protocol`・`l10n`・`versions`・`architecture`・`all` のいずれか一つ。
protocol は Rust の契約から schema/TypeScript/fixture を生成する。図・翻訳・version も source と一緒に差分を確認する。
構造検査の違反は所有元の source で修正し、固定 submodule `tooling/xtask-core` や生成図を直接変更しない。

CI は `python tooling/tasks/tasks.py ci <phase>` を呼び、ローカルと同じ処理を使う。
`quality` = check + unit tests、`product` = runtime/VS Code build + 性能/editor/VSIX/identity/smoke、`zed` = Zed WASM/ZIP のみ。
ネットワーク audit は独立した必須工程。全 6 platform を各 host で検証し、Linux x64 では `FLEXIMARK_BROWSER_DOM=true` も指定する。

## 旧コマンドからの移行

- `install` → `setup`、`dev` → `watch`（起動は F5）。
- `build` / `build-all` → `build all`、`build-vscode` 等 → `build vscode` 等。普段は対象を限定する。
- `test-pure` → `test`（Python/Rust/Node 全体）、旧 `test` → `test editor`。
- 旧 `package` の全検証 → `verify`、梱包だけ → `package vscode`。
- `verify-quality/product` → CI 専用 `ci quality/product`、`verify-audit` → `audit`。
- `protocol-generate` / `l10n` → `generate protocol` / `generate l10n`、`protocol-check` → `check`。
