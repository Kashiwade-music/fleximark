# 開発手順

repository root で実行する。toolchain と task 名の正本は [mise.toml](../mise.toml)、処理本体は [tooling/tasks/tasks.py](../tooling/tasks/tasks.py) にある。root の JavaScript package は private tooling で、VS Code 製品 manifest は `adapters/vscode/package.json` である。

## セットアップ

```sh
mise install
mise run install
git submodule update --init --recursive
```

`mise run install` は immutable な Yarn install と frozen な uv sync を実行する。Rust の WASM fixture と Zed 拡張を build する場合は `wasm32-wasip2` target も用意する。

```sh
rustup target add wasm32-wasip2
```

## core と application

native の文書処理・セッション検証には browser bundle の事前生成を要求しない。

```sh
cargo check -p fleximark-application --offline
cargo test -p fleximark-model -p fleximark-parser -p fleximark-render-html -p fleximark-engine -p fleximark-language -p fleximark-application --offline
```

`--offline` は依存 crate が取得済みの場合に使う。新規環境では依存を取得してから実行する。engine の unit tests は抽象 `PluginExecutor` を使い、実 WASM 実行は runtime 側で検証する。

構造検査と生成図の更新は次の入口を使う。

```sh
cargo xtask check
cargo xtask check --root adapters/zed
cargo xtask check --root tests/fixtures/plugin-component
```

root の [crate SVG](../architecture/crates.svg) と [module SVG directory](../architecture/modules/)、Zed の `adapters/zed/architecture`、WASM fixture の `tests/fixtures/plugin-component/architecture` を source と一緒に確認する。図は手動編集しない。Role、1000 code lines 上限、module cycle、依存境界の問題は所有元の source で直す。pinned `tooling/xtask-core` は変更しない。

独立した WASM fixture は native で hook の出力を検証でき、実際の component は WASI target で build する。生成 ABI export は `src/registration.rs` に隔離し、手書きの hook と sandbox probe は通常の構造解析対象である。

```sh
cargo test --manifest-path tests/fixtures/plugin-component/Cargo.toml --target-dir target/fixture-native --locked --offline
cargo build --manifest-path tests/fixtures/plugin-component/Cargo.toml --target-dir target/fixture-wasm --target wasm32-wasip2 --locked --offline
```

## 契約の生成

```sh
mise run protocol-generate
mise run protocol-check
```

Rust の現在の公開契約から `contracts/schemas/protocol.schema.json`、`contracts/typescript/protocol.generated.mts` と protocol fixture を生成する。型や外部形式を変えたら generator を実行し、Rust と TypeScript の conversion・validator・fixture も合わせて確認する。core の domain 型へ Schema derive や JavaScript の整数制約を追加しない。

## 製品 build と出力

```sh
mise run build
```

この task は contract の一致を確認し、release version を生成し、daemon と CLI、共通 preview、workspace UI、VS Code adapter を build する。native の Cargo 出力は `target/release`、JavaScript と test 出力は `.build`、製品 staging は `.artifacts/<version>/<Rust target>` に置く。

runtime archive は `fleximark-<version>-<Rust target>.zip`。同じ directory に checksum を出力し、`runtime/` に二つの binary、browser 資源、manifest を組み立てる。`vscode/` は同じ archive を取り込んだ VSIX 用 staging である。source tree をそのまま package root として使用しない。

個別の native build・runtime assembly には次の CLI がある。

```sh
uv run --frozen python tooling/tasks/distribution.py native --target x86_64-pc-windows-msvc
uv run --frozen python tooling/tasks/distribution.py runtime --target x86_64-pc-windows-msvc
```

target は実行環境に合わせて選ぶ。配布 tooling が定義するのは Linux GNU、macOS、Windows MSVC の x64 / arm64。cross target の compiler・linker は別途用意する。`runtime` assembly は指定 target の native build と browser 資源が既に存在することを前提とする。

daemon を手動起動する場合は組立済み runtime の `bin/fleximarkd lsp` を使う。`target/debug` 等の binary を直接試す場合は、`FLEXIMARK_RUNTIME_ROOT` に組立済み runtime directory を指定する。manifest と資源がない状態では browser service を起動できない。

## VS Code

watch と開発用製品の準備は次で実行する。

```sh
mise run dev
```

VS Code の `Run Extension` launch configuration は `.build/vscode/product` を Extension Development Host として開く。変更を確認する前に watch の ready メッセージを待つ。native の変更は Rust binary と staging の更新後に host を再起動する。

確認する操作は次のとおり。

1. 検証用 workspace を開き、Workspace Trust と現在の設定を確認する。
2. Markdown の埋め込み preview と browser preview を開く。
3. 保存せずに本文を変更し、両方の表示に反映されることを確認する。
4. editor から preview への移動、preview から元ソースへの移動を確認する。
5. workspace UI で note の category/template/name、theme、admonition 収集と HTML export を試す。
6. daemon の再接続後に、開いている文書と preview が現在の内容で再作成されることを確認する。

ログは VS Code の FlexiMark Output と daemon の標準エラーで確認する。標準出力へ通常ログを追加しない。

## 通常版 Zed

Zed 拡張は独立した Cargo workspace である。

```sh
cargo test --manifest-path adapters/zed/Cargo.toml --locked
cargo build --manifest-path adapters/zed/Cargo.toml --target wasm32-wasip2 --locked
cargo xtask check --root adapters/zed
```

Zed の extension development 機能で `adapters/zed` を読み込み、必要に応じて `lsp.fleximark.binary.path` を組立済み daemon へ向ける。path を指定しない場合は worktree PATH、次に matching release archive を使う。binary は `--version` で互換性を確認し、`lsp` で起動する。設定例は [Zed README](../adapters/zed/README.md) を参照する。

標準 LSP 初期化の `initialization_options.fleximark` に trust・workspace を指定する。明示しない root は untrusted である。Markdown 上の Code Action で preview、選択位置の preview、workspace 操作を開く。browser のソース移動が Zed の `window/showDocument` へ戻り、未保存の変更が `didChange` で表示に届くことを確認する。外部 browser は daemon host の loopback に接続する。継続的な Zed viewport tracking は検証対象ではない。

## 明示的なデータ移行

旧ユーザーデータを検証用 copy で確認し、計画を読んでから適用する。

```sh
fleximark migrate /absolute/workspace --dry-run
fleximark migrate /absolute/workspace
```

旧 editor settings を指定する場合は `--settings /absolute/settings.json` を使う。未指定なら workspace の `.vscode/settings.json` を読み、JSONC と `.code-workspace` の settings にも対応する。CLI は計画を JSON で出力し、適用時に入力 snapshot を再検査して backup を作る。現設定が移行結果と一致する場合、または旧設定が残っていない場合は `alreadyCurrent` を返し、内容が競合する場合は書き込まない。中断した移行は `fleximark migrate /absolute/workspace --recover` で復旧する。旧 JavaScript plugin は実行・変換せず、保持と無効化を報告する。editor の migration command もこの CLI を明示起動する。通常 activation や設定読込みでは migration を呼ばない。

現在の workspace 書込みを CLI で実行する場合は `--trusted-workspace` を明示する。例えば `fleximark init /absolute/workspace --trusted-workspace` を使う。untrusted の render は workspace plugin を読み込まない。

## 自動検証と package

```sh
mise run test-pure
mise run test
mise run verify
mise run package
```

`test-pure` は Node 上の protocol・preview・adapter unit tests、`test` は build と VS Code integration suite を実行する。`verify` は codegen、root/Zed/WASM fixture の xtask と追跡 SVG の差分、Python tests、TypeScript・ESLint、localization、製品 build、Rust fmt/test/clippy、WASM fixture、performance budget、VS Code integration を組み合わせる。Linux の GUI integration 環境では必要に応じて `xvfb-run -a mise run test` を使う。

`package` は verify 後、host target の staging から dependency-free VSIX を `.artifacts/<version>/<Rust target>` に出力する。完成 VSIX は次で smoke を実行する。

```sh
mise run smoke -- /absolute/path/to/product.vsix
```

localization の更新は `mise run l10n` で行い、`adapters/vscode/l10n` の生成差分と翻訳を同じ変更で管理する。release version は `release.toml` を更新し、各製品 manifest を個別の正本にしない。
