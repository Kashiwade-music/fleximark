# FlexiMark 大規模リファクタリング要否 調査レポート

- 対象: `feat/kashiwade/big-update`（HEAD `fd5cd1a`、`main` から 90 commit 先行、+71,802 / −25,587 行）
- 作成日: 2026-10-29
- 前提: **現行の WASM プラグイン機能は全廃する**（Zed 拡張自体の WASM ビルドは対象外）
- 方法: 5 体のサブエージェントで分担し、読み取りのみで調査した。主要な指摘の一部は統合時に再確認した（§5 に記す）。
  1. Rust core とプラグイン
  2. application・transport・contracts
  3. TypeScript（VS Code adapter / web）
  4. tooling・CI・配布・Zed
  5. git 履歴の横断分析
- 行番号は `fd5cd1a` 時点のもの。サブエージェントの読取り結果のため、数行ずれている場合がある。

> **注記（指示書の作成後に追記）**: 本レポートの作成後に方針が決まり、[REFACTORING_INSTRUCTIONS.md](REFACTORING_INSTRUCTIONS.md) の §2 に記載した。本ブランチは未リリースのため、互換性を持たない。形式の version はすべて 1 にリセットし、プラグイン設定の移行はしない。リリースは手動タグに一本化する。本レポート §3.2・§8・§9 の「config schema v3」「migration の追加」「protocol version を上げる」などの記述は、指示書の決定事項に置き換わっている。

---

## 1. 結論

**3 回目の全面作り直しは不要である。ただし、ほぼ全層にまたがる「横断的な構造整理」は必要であり、その規模は大きい。**

**作り直しが不要な理由**
- 設計の芯は機能しており、壊れていない。
  - core → application → transport → adapter の依存方向は守られている。
  - core は I/O・protocol・Wasmtime に依存しない。
  - 位置は UTF-8 byte と共有 SourceIndex に一本化されている。
  - export の journal と recovery、cap_std による symlink 防御も維持すべき資産である。
- 履歴上、アーキテクチャの書き直しは既に実質 2 回ある（`dbd9a3a`、`4a751f5`）。その後に大規模統合 `07db40e` が行われた。
- その間に同じ領域を何度も作り直している。
  - preview 配送: 8 回
  - workspace の所有者: 4 回
  - tooling: 7 回
  - protocol: 7 週間で v1 から v7 へ
- 再度ゼロから作り直すと、この「同じ領域を何度も作り直す」パターンをもう一度繰り返す危険が高い。

**整理が必要な理由**
- 書き直し後に機能を積み増すたびに、「同じ知識を別の場所へ手で写す」形の歪みが全層で増えている。
- 典型例は次のとおり。
  - コマンド名の一覧が約 10〜16 か所にある。
  - 配布用の target 表とファイル一覧が 7 か所にある。
  - 構文機能の知識が 6 層に散っている。
  - preview 状態を 3 か所が保持している。
  - TS 側に「古い非同期処理の結果を捨てる判定」が 8 種類ある。
- 確認済みの不具合と、仕様と実装の不一致も複数ある（§5）。

**着手のきっかけ**
- WASM プラグインの全廃は、整理を始める最良の起点になる。
  - Rust コードの約 18%（約 4,500〜5,100 行）が消える。
  - 依存 crate が約 4 割減る。
  - engine の文書更新処理が 5 段から 2 段になる。
- 撤去と同時に「残った機能の正本を一か所にする」作業を進めるのが、最も費用対効果が高い。

---

## 2. 定量データ

### 2.1 規模（非生成・非 lock・非 asset、非空行）

| 言語 | 行数 | ファイル数 |
|---|---:|---:|
| Rust | 27,180 | 130 |
| TypeScript | 14,052 | 75 |
| JSON（fixture・schema） | 3,744 | 37 |
| Python（tooling） | 3,655 | 22 |
| CSS / JS | 752 / 487 | 3 / 8 |

ディレクトリ別の上位: `tests/typescript` 7,457、`application/src` 6,863、`adapters/vscode` 4,737、`tooling/tasks` 4,001、`transports/lsp` 3,083、`runtime/wasm-plugins` 3,080。

### 2.2 テストの構成

| 層 | ファイル | 行 | 件数 |
|---|---:|---:|---|
| Rust の単体テスト | 14 | 3,907 | Rust 全体の `#[test]` 201 件（下記の内訳を参照） |
| Rust の統合テスト | 15 | 3,471 | apps/server 26 ほか |
| TypeScript | 30 | 7,507 | 約 132 件。1 件あたり約 57 行のシナリオ型 |
| Python | 10 | 1,719 | 75 件 |
| fixture | 21 | 2,438 | Markdown の fixture は 9 件だけ |

- Rust の `#[test]` 201 件の crate 別内訳: application 50、apps/server 26、wasm-plugins 25、render-html 18、engine 15、protocol 13、parser・model 各 7、lsp 6、preview-http 2、zed 4。
- **TS ではテストコード（7.5k 行）が本体（約 6.6k 行）より大きい。**
- 一方、parser と model のテストは少ない（各 7 件）。

### 2.3 変更のホットスポット（変更回数 × 規模）

| ファイル | commit 数 | 行 | 所見 |
|---|---:|---:|---|
| `adapters/vscode/src/adapter.mts` | 23 | 1,012 | private field 29 個とテスト用の入口 4 個を持つ。1000 行の上限は TS には適用されない |
| `tests/typescript/preview-client.test.mts` | 22 | 1,029 | |
| `tests/typescript/adapter/preview-coordinator.test.mts` | 16 | 992 | 本体（689 行）より大きく、本体と毎回同時に変更されている |
| `tooling/codegen/src/main.rs` | 11 | 850 | protocol を変えるたびに追従が必要 |
| `core/render-html/src/lib.rs` | 10 | 871 | |
| `application/src/native/notes.rs` | 10 | 510 | admonition 収集も同居している |
| `.github/workflows/{release,ci}.yml` | 15 / 14 | | 両者が同時に変更される率は 0.92（内容が重複） |

### 2.4 同時に変更されやすいファイル群

1. **VS Code adapter とその TS テスト**: 26 回、正規化率 0.87。テストが実装の詳細に密結合している。
2. **protocol の連鎖**: notes.rs → codegen → 生成 TS → contract テスト。かつては version を 1 回上げるごとに約 20 ファイルを更新していた。値 `7` は現在 8 か所にある。
3. **tooling・CI・TS テスト**: 18 回。build 手順の変更がテストにまで波及している。
4. **wasmtime 系**: Cargo.lock・CI・plugin host・fixture。プラグインを廃止すると消える。

---

## 3. WASM プラグイン全廃の影響

### 3.1 規模

| 項目 | 量 |
|---|---|
| 専用ディレクトリ | 約 3,770 行（plugin-api 160、contracts/plugin 319、runtime/wasm-plugins 3,080、fixture 211）＋生成 SVG 16 ファイル |
| 他の crate 内の専用コード | 約 920 行（application の plugins.rs・test_plugins.rs・engine_pipeline.rs、engine の provenance.rs・plugin_tests.rs ほか） |
| 部分的に依存しているコード | 約 350〜450 行 |
| **合計** | **約 4,500〜5,100 行（Rust 全体の約 18%）**。テスト約 34 件・約 2,100 行を含む |
| 依存 crate | 出荷物の依存グラフで 246 → 136 / 257 → 161（計測方法の違い）。**約 4 割減**。wasmtime / cranelift / ed25519 が 6 platform の release build から消える |
| Cargo workspace | 3 → 2（`tests/fixtures/plugin-component` を廃止） |
| churn に占める割合 | 非生成の変更行の 8.6%、17 / 89 commit |

### 3.2 対象ごとの分類

| 対象 | 分類 | 備考 |
|---|---|---|
| `runtime/wasm-plugins`、`contracts/plugin`（WIT・SDK）、`tests/fixtures/plugin-component`、`contracts/schemas/plugin-manifest.schema.json` | **削除** | |
| `core/plugin-api` | **削除** | `PluginExecutor`、Hook、`PluginRun`、`PreprocessedSource`、`EditMap*`、`UnsafeExportOutput` など |
| `CancellationToken`（plugin-api 内） | **移設して残す** | 現在 29 ファイルが参照しており、プラグイン以外にも application・LSP・CLI・engine が使っている。application か transport に移すか、core の別 crate に置く |
| engine の前処理 edit map と出典の復元（`provenance.rs`） | **削除** | 呼び出し元はプラグイン前処理だけ |
| engine の `CandidateHooks`、`prepare_source_candidate`、`open`/`open_configured`、`reconfigure*` 群、`render`/`render_configured`/`render_with_plugins` | **簡素化** | 処理は「parse → NodeId 照合 → 反映」の 2 段になる。engine 本体は約 1,500 行から 700〜800 行になる見込み |
| NodeId 照合（`identity.rs`） | **維持** | preview で DOM を保持するために必要 |
| model の `SourceProvenance::Derived/Generated`、`TransformId`、`GeneratedAnchor`、複数 range、`Block.attributes`、`DOCUMENT_SCHEMA_VERSION` と serde 派生 | **簡素化・削除** | 本番で生成しているのはプラグイン経路だけ。`Block.range: SourceRange` 一つに縮約できる |
| `BlockKind::Plugin{namespace,name}` | **改名して残す** | 不明な directive の受け皿として使われている。`Directive{name}` などにする |
| `RenderConfig` の `plugin_set_hash`、`plugin_generation`、`for_plugins`、`validate_for(plugins)` | **簡素化** | 現状は trust がない workspace でも空の PluginHost を作っている（`application/src/native/plugins.rs:24-35`） |
| `RenderSnapshot.annotations` と script 出力、TS 側の annotations DOM 処理 | **削除** | 生成元はプラグインだけ。protocol version を上げる必要がある |
| export の unsafe HTML 段階（`PreparedExport`/`ResolvedExport`/`apply_unsafe_export_html`、`ExportReceipt.unsafe_output`、registry の `unsafe_output_used`） | **削除** | export が単一段の処理になる。**ディスク上の registry 形式の互換方針を決める必要がある** |
| `EngineError::Plugin`、`ServiceError::PluginConfig` | **削除・再分類** | プラグインと無関係な失敗にも使われている（§4.6） |
| `contracts/config` の `[[plugins]]`、grants、plugin-sdk 依存 | **削除** | `deny_unknown_fields` のため既存の設定が読めなくなる。**config schema v3 と migration の追加が必要** |
| trust モデル | **簡素化** | 残る意味は「workspace への書込み」と「theme・local asset の読込み」だけ。二値の `trusted`/`enabled` を一つの enum にできる |
| LSP のプラグイン診断、telemetry の `plugin_failure` | **削除** | |
| xtask policy の `wasmtime::component::bindgen` と fixture の `trusted_macros`、`visible_to` のプラグイン行、dependabot と cache の fixture lockfile | **削除** | |
| `tasks.py` の fixture 用 6 step（fmt・clippy・wasm32 check・test・xtask・diagram） | **削除** | `check_steps` は 14 → 10、architecture の検査対象は 3 root → 2 root |
| `rustup target add wasm32-wasip2`、Zed の `trusted_macros` | **維持** | Zed 拡張用 |
| migration の旧 JS プラグイン検出 | **維持（文言修正）** | 警告文「signed WASM plugin に置換」を修正する |
| VS Code の設定 watcher `plugins/**`、untrusted 説明文の「WebAssembly components」 | **簡素化** | |
| raw HTML の sanitizer、`content-security.mts` | **維持** | 作者が書いた HTML の防御として必要 |
| docs（ARCHITECTURE の版記述・文書更新順序、IMPLEMENTATION、README、`contracts/capabilities/*.json`） | **更新** | |

---

## 4. 横断的な構造問題

この節は各層の指摘を、根本原因ごとに束ね直したものである。

### 4.1 【高】コマンドと操作の面が文字列で多重定義され、入口も 3 系統ある

- **Rust 側の名前一覧は 6 か所**ある。
  - `transports/lsp/src/server/standard_commands.rs:6-16`（`COMMANDS`）と `:37-47`（Code Action の表）
  - `commands.rs:47-60`（文字列から `Op` への match）
  - `workspace_ui.rs:113-121`（許可リスト。**`acknowledgeExport` だけが抜けている**）
  - `rpc.rs:59-65`（capability）
  - `apps/cli/src/main.rs`
- **TS 側にもある。** `adapters/vscode/src/commands.mts:211-219`、`web/workspace-ui/model.mts:22-27`、`web/workspace-ui/index.mts:17` の HTML。`collectAdmonitions` という名前は 16 ファイルに出現する。
- **入口が 3 系統ある。** `fleximark/executeCommand`、LSP 標準の `workspace/executeCommand`（Zed）、HTTP の `/command`（workspace UI）。さらに CLI は application の型付き操作を経由せず `native` 関数を直接呼ぶ。
- **型が弱い。** `ExecuteCommandParams` は `command: String` と全コマンド分の Option フィールドを持つ平坦な型である（`contracts/protocol/src/lib.rs:434-456`）。
- **内部で JSON を経由した再入がある。**
  - `standard_execute_command` が `json!` で引数を作って `create_preview` を呼び、結果を `/result/url` のポインタで読み戻す（`standard_commands.rs:123-140`）。
  - `lsp_initialize` が旧 RPC 形式の JSON を組み立てて `configure_session` に渡し、`.expect` で受ける（`lsp.rs:72`）。旧 RPC の残骸である。
- **影響**
  - 新しい workspace 操作を一つ足すのに 13〜16 か所の変更が要る。
  - 定義のずれ（`acknowledgeExport` の抜け）が既に起きている。
  - trust の判定が少なくとも 6 か所に分散している。
- **対応**
  - `contracts/protocol` に `#[serde(tag = "command")]` の `WorkspaceCommand` を置く。名前・必要な文脈・trust 要件をここに持たせ、TS とコマンド一覧を生成する。
  - application の `execute_operation` を唯一の入口とし、CLI も含めて使う。
  - TS 側の dispatcher も一本化する。

### 4.2 【高】状態の所有者が一つに定まっていない

**Rust の preview 状態が 3 か所にある**
- application の `DocumentSession.previews`（`application/src/document.rs:10`）
- LSP の `Server.preview_states`（`transports/lsp/src/server/state.rs:47-54`）
- HTTP の `PreviewServer.pages`（`transports/preview-http/src/state.rs:22-33`。frame を複製している）
- 「再描画して配信する」処理も `lsp.rs:487-528` と `rpc.rs:476-510` に重複している。
- 三者の整合を手作業で保っている。ARCHITECTURE の「状態を層ごとに複製しない」と矛盾する。

**TS で古い非同期結果を捨てる判定が 8 種類ある**
- `sameDaemonOrigin`、`previewOwnsIncarnation`、`previewCandidateIsCurrent`、`previewCandidateIdentityIsCurrent`、`previewIsCurrent`（`preview-coordinator.mts`）
- `#previewIsActive`、`#originIsCurrent`、closure（`adapter.mts`）
- `PreviewState` / `PreviewBinding` は 10 個の可変フィールドの組合せで、暗黙の状態機械になっている（`runtime-state.mts:16-33`）。
- `binding.renderRevision` は embedded と external で意味が違う。
- binding の交換と `disposePreview` の呼出しが、それぞれ 3 か所に重複している。

**workspace の集合**
- VS Code adapter は「runtime の集合」が変わると daemon を kill して再起動する（`adapter.mts:272-278` → `daemon-supervisor.mts:134-143`）。
- その一方、daemon に渡すのは常に `vscode.workspace.workspaceFolders` である。
- このため、**workspace 外の Markdown を初めて開いただけで daemon が再起動するが、daemon に新しい情報は何も届かない**。
- `synchronizeEditorWorkspaces` は事実上何もしない。この挙動は `multi-root-runtime.test.mts:420-423` で固定されている。

**対応**
- Rust: 配信状態（token・sequence）は HTTP だけが持ち、frame は application の `Arc` を参照する。配信処理は一本にまとめる。
- TS: 接続の世代（`DaemonConnection`。破棄でき、AbortSignal を持つ）と `PreviewController`（embedded と external の判別 union）を導入し、判定を `binding.isCurrent()` 一つにまとめる。
- workspace の増減は `didChangeWorkspaceFolders` で処理し、再起動は crash のときだけにする。

### 4.3 【高】同じ知識の正本が複数ある

| 知識 | 重複箇所 | 対応 |
|---|---|---|
| 配布の target triple 表 | 7 か所: `distribution.py:17-24`、`_targets.py:24-31`（旧 layout）、`runtime-manifest.mts:12-19`、`.vscode-test.mjs:13-20`、`adapters/zed/src/release.rs:10-20`、schema、workflow の matrix | `contracts/distribution` に機械可読な正本を置き、各言語の定数を生成する |
| runtime の 7 ファイル一覧 / manifest の検証 | 一覧が 7 か所、検証の実装が 6 つ（Python 3、Rust preview-assets、Zed、TS）。検証の強さが実装ごとに違い、TS は要求された binary だけ、Zed はサイズ上限なし。正本の schema を読むのはテストだけ | 検証は Rust の 1 crate（Zed と共用）と Python 1 つにまとめる |
| `release.toml` の解析 | 手書き parser が 4 つ | 生成するか、共通の reader を使う |
| 構文機能（admonition・abc・mermaid・math・YouTube） | model の enum と子要素の規則、parser の文字列 match（`parser/src/lib.rs:176-189,211`）、language の補完（`language/src/lib.rs:75,82,178-214,252-258`）、render-html の kind、web の `enhance.mts:57-103` の switch。YouTube の判定は parser と render-html の 2 か所 | core に組込み構文の記述子テーブルを置き、parser・language・web の kind 定数をそこから導出する。web 側は Enhancer registry にする |
| protocol の型 registry | `WireType` が 5 重（variant・ALL・name・schema_name・ts_name）、さらに codegen の import・match・`METHOD_SPECS`。1 型の追加で 7〜8 か所 | 宣言的な表一つにする（xtask の macro 制約の見直し、または build.rs での生成） |
| 生成 TS 型と手書き型 | `web/workspace-ui/model.mts:1-36`（`CommandResult.message` が生成型と**ずれている**）、`navigation.mts:8-21`、`providers.mts:13-21`。`contracts/typescript/protocol.mts:39-40` では alias の名前が生成型と逆転している | 生成名をそのまま使う。workspace HTTP API も codegen の対象にする |
| IR の再帰走査 | inline の子要素 match が 6 か所（model・engine・render-html・identity・wasm candidate・application/assets） | `InlineKind::children()` と visitor を model に置く |
| SHA-256 hex の検査 / asset 参照の形式 | 5 か所 / 3 か所 | newtype にして共有する |
| CSP 文字列、HTTP 応答の組立て | 3 か所 / 7 か所以上 | 定数と helper を一つにする |
| polling・fixture 用のテスト helper | TS 4 重、Rust 3 系統 | 共通の harness にする |

### 4.4 【中】層の境界が崩れている

- **application の公開 API が 2 系統ある。**
  - 型付き操作の `SessionRegistry::execute_operation` と、生の関数群 `pub mod native` を両方公開している（`application/src/lib.rs:5-20`）。transport と CLI は両方を混ぜて使っている。
  - 「`WorkspaceServices` を置かない」という方針の代わりに、`native` そのものが雑多なサービス層になっている。
  - 戻り値がタプルで型が弱い（`Vec<(String,bool,bool)>`、`lib.rs:88`）。
  - 対応: `native` を `pub(crate)` にし、`DocumentStore` / `WorkspaceAuthority` / `WorkspaceOps` に整理する。crate の分割は必須ではない。
- **application が LSP 形の DTO と daemon ID の検証を持っている**（`requests.rs:3-56`、`positions.rs`、`lib.rs:418`）。「位置の変換は通信境界で行う」という方針に反する。
- **core/language が LSP 形の `CompletionItem` を serde 付きで返している**（`language/src/lib.rs:36-58`）。token の並び順だけで transport の数値と暗黙に結びついている。補完の中で行・文字位置を計算している。semantic tokens は fence の状態を持たない（code block の中の `:::` も強調される）。
- **LSP transport が preview-http transport に依存している。** application → protocol の frame 変換が `preview-http/src/protocol.rs` にあり、LSP の `protocol.rs` はそれを包んでいるだけである。ARCHITECTURE の表に載っていない依存である。
- **外部向けの文言を application が作っている。** `migrate_workspace` が stdout の文言を作り、`MigrationOutput{stdout}` として返す。
- **描画層にポリシーが入っている。** URL を許可する判定と asset 参照の検証が render-html の中にある（`render-html/src/lib.rs:794-850`）。navigation の収集と NodeId の重複検査は model と render-html で二重になっている。

### 4.5 【中】並行性・性能

- **workspace 操作が単一の actor ループ上で同期実行され、1 秒で timeout する**（`transports/lsp/src/transport/stdio.rs:94`、**確認済み**）。
  - 時間のかかる export は 1 秒を過ぎると「timed out」と表示されるが、処理は続いて成功する。その間、LSP の要求は止まる。
- **`didChange` のたびに全画像を読み直して検証する**（`lsp.rs:458` → `application/src/lib.rs:98-113` → `native/assets.rs:132`）。
- **SSE が実質ポーリングである。** `Connection: close` と `retry: 250` の組合せで、1 タブあたり毎秒 4 回接続する（`preview-http/src/lib.rs:396-401`、**確認済み**）。HTTP 要求ごとに deadline 監視スレッドが 10ms 間隔で sleep する。
- **同じ frame を 3〜4 回検証している。** 経路は adapter の RPC 受信 → bridge → `PreviewHost.apply` → `PreviewDocument.apply`。そのたびに汎用 JSON Schema 解釈と base64 検証を通る。external preview でも asset 入りの完全な frame を受け取るが、使うのは revision だけである。
- **出典の復元が遅い。** `remap_provenance` がノードごとに SourceIndex を作り直し、O(ノード数 × 本文長) になる。プラグイン廃止で消える。

### 4.6 【中】エラーの型付けが途中で失われる

- **経路の途中で文字列に潰される。** 変換は `ServiceError` → `String` → `ServiceOperationError::Failed(String)` → `(i64, String)` と進む。JSON-RPC のエラーコード（-32020 など）は数値の直書きで散在している。
- **variant の使い方がずれている。**
  - `EngineError::Plugin(String)` をプラグインと無関係な失敗やキャンセルにも使っている。テストは `"operation cancelled"` という文字列を比較している。
  - `reject_linked_path` は theme や asset からも呼ばれるのに、常に `PluginConfig("plugin package contains…")` を返す。
- **本番コードに panic がある。** 特に preview server を bind できないと daemon が panic する（`routing.rs:35`）。
- **ログが不十分。**
  - `let _ = error;` でエラーを捨てている箇所が 9 か所ある。
  - `log_operational_event` が二重に定義されている。preview-http 側は何もしない stub である。
  - TS の `logLevel` は `error` / `debug` の区別が効かない。
- **対応**
  - application の公開エラーを一つの enum（NotFound / Denied / Conflict / Invalid / Cancelled / Internal）にする。
  - エラーコードは protocol の定数表から取る。
  - 構造化ログを一系統にする。

### 4.7 【中】god モジュール

| ファイル | 問題 |
|---|---|
| `adapters/vscode/src/adapter.mts`（1,012 行） | DI 用の依存 bag 2 つ、process 起動、LSP メッセージの手書き振り分け（`as` によるキャスト）、selection と viewport の送信、文字列コマンドの分岐、ログを 1 クラスで持つ。**本番クラスにテスト用の入口が 4 つある**（`*ForTest`）。接続がないときに `#daemonOrigin()` が `#testOrigin` を返す |
| `core/render-html/src/lib.rs`（871 行） | raw HTML をマーカー → ammonia → 差し戻しで処理する経路が **3 系統**ある（block・nodes・inline。`:113-201`、`:504-570`、`:572-611`）。微妙に違う規則で書かれ、4 要素タプルを添字でアクセスしている。セキュリティ上重要な経路で修正漏れが起きうる |
| `application/src/native/export/mod.rs` | `export_html_transaction` が約 260 行の単一関数。export 1 回で `validate_config` を 3 回以上ディスクから読み直す |
| `core/parser/src/lib.rs` | 到達しない分岐がある（無効な拡張の footnote など）。Highlight などを Emphasis に潰す。不明なノードは Debug 表示の文字列で代用する。alert の独自タイトルを無視する |
| `tooling/tasks/tasks.py` | Plan の定義・JS build・Rust・配布が同居し、関数内 `__import__` が多い |
| `web/preview/fleximark.css`（817 行） | 一つの巨大な `.markdown-body {}` ネスト。hex 色の直書きが 71 か所。現行の renderer が出力しない legacy class（`div.info` など）が残っている |

### 4.8 【中】tooling・CI・リリース

- **リリース経路が branch 上で切れている（確認済み）。**
  - main では `release.yml` が semantic-release を実行していた。現在は `tags: v*` で起動するだけで、版上げとタグ付けを行う workflow がない。
  - それでも `release.config.mjs`、`semantic-release-package.mjs`、devDependencies 7 個が残っている。
  - GitHub release を作る処理が二重にある（semantic-release の draft と `gh release create --draft`）。
- **CI と Release の workflow が同じ内容である。** `ci.yml:15-106` と `release.yml:15-106` の 3 job は同一。その重複を Python テストが YAML 文字列の正規表現で固定している（`test_release_workflow_contract.py`）。matrix の `vscode:` キーは、この正規表現を満たすためだけに存在する。
- **未使用の composite action が 3 つある**（`smoke-vsix`、`validate-extension`、`verified-release-artifact`）。
- **cache の key に restore-keys がなく、`target/` が更新されない。** Rust の unit test は Linux でしか実行しない。
- **orchestrator が 5 段に重なっている**（yarn → mise → `tasks.py` → cargo xtask → xtask-core）。yarn と mise の 2 段はそのまま転送するだけである。二次 CLI も 6 つある。
- **codegen**
  - TS 型への変換と validator が手製である。
  - LSP method の enum が古い（`codeAction`、`semanticTokens/full`、`executeCommand`、`showDocument`、`logMessage` がない）。利用者がテストだけなので表面化しない。
  - fixture 生成のために engine と comrak に依存している。
  - `check_protocol_contract` が verify の 1 回で 4 回以上走る。
- **xtask の制約**
  - macro を禁止しているため、型 registry を宣言的に書けない。
  - `visible_to` の allowlist は実際の依存より許可が多く、逆方向の許可まで含む。依存方向の制約として機能していない。
  - 1000 行の上限は Rust にしか効かない（TS の adapter は超過している）。
- **使われていない物**
  - `vscode:prepublish` 系一式（staging で `scripts` を削除しているため実行されない）
  - `_targets.py`、`vendor_katex_css.py`
  - 未参照の JSON 5 つ（`contracts/capabilities/*.json`、`feature-inventory`/`plugin-manifest` の schema）
  - uv / `pyproject.toml`（依存なし）
  - `.vscodeignore` の `packaging/**`
- **開発時の watch が過剰。** runtime の全ファイルを 0.1 秒ごとに SHA-256 で再ハッシュしている（`tasks.py:153-177`）。
- **crate 名とディレクトリ名がずれている。** `runtime/wasm-plugins` = `fleximark-plugin-host`、`contracts/plugin` = `fleximark-plugin-sdk`、`apps/server` = `fleximarkd`。

### 4.9 【中】テストの構造

- TS のテストは、内部の依存 bag と内部関数に強く結合している。
  - `preview-coordinator.test.mts`: `as unknown as` が 20 回、1 テストが約 150 行。
  - `multi-root-runtime.test.mts`: 467 行でテスト 1 本。
  - public メソッドを monkeypatch している。
- **実装を変えるたびにテストも同時に直す必要がある（同時変更率 0.87〜1.00）。** このため、リファクタリングの最大の抵抗になる。
- Python 側では、source や workflow を grep する構造テストが多い（`test_phase12_contract.py` は旧名のまま）。
- 配置のずれがある。
  - `apps/server/tests/service_contract.rs`（475 行）は `application::native` しか使わない。本来は application のテストである。
  - pure なテストが Electron 上で実行されている。
- **観点が抜けている。**
  - parser には `math` fence、alert の独自タイトル、不明な directive のテストがない。
  - language には fence 内部のテストがない。

---

## 5. 確認済み・高確度の不具合と不整合

| # | 内容 | 根拠 | 確度 |
|---|---|---|---|
| 1 | workspace UI からの操作が 1 秒で timeout する。処理自体は続行して成功するため、利用者に誤ったエラーが表示される | `transports/lsp/src/transport/stdio.rs:94` | 統合時に確認 |
| 2 | workspace 外の Markdown を開くと daemon が全再起動する。再起動しても新しい情報は送られない | `adapters/vscode/src/adapter.mts:272-278`、`adapter.mts:796`、`editor-service.mts:79-92` | 統合時に確認（再起動の条件） |
| 3 | 補完が提案する ```` ```math ```` が数式として描画されない。parser は `mermaid`/`abc` 以外の fence を CodeBlock にし、comrak も `math` fence を CodeBlock として返す | `core/language/src/lib.rs:252-258`、`core/parser/src/lib.rs:176-189` | parser の分岐は確認済み。描画結果は実機で要確認 |
| 4 | リリースの版上げとタグ付けを担う workflow がない | `.github/workflows/release.yml:3-5` | 統合時に確認 |
| 5 | workspace UI の許可リストに `acknowledgeExport` がなく、コマンド定義どうしがずれている | `transports/lsp/src/server/workspace_ui.rs:112-121` 対 `commands.rs:52` | 統合時に確認（UI が ack を要求するかは要確認） |
| 6 | SSE が `Connection: close` と `retry: 250` による 250ms 間隔のポーリングになっている | `transports/preview-http/src/lib.rs:396-401` | 統合時に確認 |
| 7 | editor 側の migration に dry-run と確認がなく、即座に apply する。ARCHITECTURE の「dry run」と食い違う | `application/src/native/services.rs:33-50` | サブエージェント |
| 8 | NodeId の識別キーが `format!("{:?}")` に依存している。依存の更新やフィールドの追加で全 NodeId が変わりうる | `core/engine/src/identity.rs:158` | サブエージェント |
| 9 | preview server を bind できないと daemon が panic する | `transports/lsp/src/server/routing.rs:35` | サブエージェント |
| 10 | `default_note_root()` が `"."`、テンプレートは `"notes"` で、値が一致しない | `contracts/config/src/lib.rs:48` | サブエージェント |
| 11 | 未使用の protocol フィールド・capability（`embeddedHtml`/`structuredPreview`/`openExternal`）。adapter は送るが Rust は読まない | `contracts/protocol/src/lib.rs:169-180`、`editor-service.mts:93-99` | サブエージェント |
| 12 | codegen の LSP method enum が実際の配送内容と一致しない | `tooling/codegen/src/main.rs:616` | サブエージェント |
| 13 | TS の `logLevel` の `error`/`debug` が効かない | `adapter.mts:785,998` | サブエージェント |

---

## 6. ドキュメントと実装の乖離

| 記述 | 実態 |
|---|---|
| 「外部向けの message は transport が組み立てる」 | `migrate_workspace` が application の中で stdout の文言を作っている |
| 「transports/lsp が application と外部 DTO を変換する」 | 変換の実体は `preview-http/src/protocol.rs` にある。lsp → preview-http の依存は表に記載がない |
| 「同じ値型を層ごとに複製しない」 | IMPLEMENTATION の「protocol は自前の表現を定義する」と文書内で矛盾している。実際に同名の型 10 個（RenderFrame、SourceRange など）が二重に定義されている |
| 「行・文字位置は通信境界で生成する」 | application の `positions.rs`、core/language の補完が行・文字位置を計算している |
| 「旧 RPC モードはない」 | 機能はないが、`rpc.rs`/`rpc.mts`/`rpc.test.mts`、Role の文面、`lsp_initialize` の旧形式 JSON の往復が残っている |
| 「移行は dry run を提供する」 | CLI だけが dry run を提供する。editor にはない |
| 「VS Code の target 対応は配布 tooling が持つ」 | adapter の TS とテスト設定にも同じ対応がある |
| 「各ファイルは 1000 code lines 未満」 | Rust にしか適用されない（`adapter.mts` は 1,012 行、`preview-client.test.mts` は 1,029 行） |
| 用語 | docs の「preview revision」「daemon identity」は、コードでは `renderRevision`、`daemonInstanceId`。workspace / root / folder も混在している |
| xtask-core README の `vendor/xtask-core` | 実際の配置は `tooling/xtask-core` |
| プラグインの記述全般 | 廃止に合わせて ARCHITECTURE、IMPLEMENTATION、README、capabilities の各 JSON を更新する必要がある |

---

## 7. 維持すべき資産

- crate の依存方向。core は I/O・protocol・ブラウザ資源から独立している。
- UTF-8 byte による内部位置と共有 SourceIndex。CRLF やサロゲートのテストも厚い。
- 失敗した候補を反映しない更新、`out_of_sync` 時の全文再同期、失敗・中断時に以前の frame を保持すること。
- export の journal・ownership・recovery と障害注入テスト。cap_std / nofollow による symlink・junction の防御。
- 内部マーカーとの衝突への防御と、`data-fleximark-*` 属性の剥奪。リンク先を拒否しても文書全体を失敗させない方針。
- preview-assets の許可リスト・包含・hash 検証。
- TS の型付き RPC、依存を注入できる `DaemonSupervisor`、`PreviewDocument` の DOM 差分 reconcile と原子的な rollback、webview bridge の token 認証。
- `tasks.py` の Plan モデルと `--dry-run`。CI がローカルと同じ phase を呼ぶ構成。SHA で pin した action と attestation。
- 小さく公式 API に忠実な Zed adapter（約 430 行）。

---

## 8. 推奨ロードマップ

各フェーズは独立して merge できる単位とする。順序は「削除で単純化 → 正本の一本化 → 状態の整理 → 周辺」とする。フェーズ 3 で TS の状態モデルを整理するまでは、TS の大規模なテスト書換えは避ける。

### フェーズ 0: 方針の決定（コード変更前）
§9 の判断事項を決める。特に次の 4 つ。
- リリース方式
- config schema v3 と migration の範囲
- export registry の互換
- protocol version の運用

### フェーズ 1: WASM プラグインの撤去と core の縮約
- §3.2 の表に従って削除・簡素化する。`CancellationToken` は application か transport の所有に移す。
- engine を `open / change / resync / set_render_config / render` の最小の公開面にする。model の provenance を単一 range にする。annotations と unsafe export を撤去する。
- `EngineError` と `ServiceError` を型付きに再分類する（Cancelled の variant を追加し、Plugin を廃止する）。
- config schema v3 と、「v2 の `plugins` を除去して報告する」migration を追加する。protocol version を上げる。
- tasks.py の fixture step、Cargo の exclude と依存、xtask policy、dependabot、docs を更新する。
- **完了条件**: wasmtime 系が `cargo tree` から消え、verify が通る。

### フェーズ 2: 型付き command protocol と application 公開面の一本化
- `WorkspaceCommand`（tagged enum）を protocol に定義し、TS 型とコマンド一覧を生成する。
- `execute_operation` を唯一の入口とする。`get_note_options` と migration の plan/apply もここに含める。`native` は `pub(crate)` にする。
- CLI も同じ API を通す。単一文書用の `WorkspaceAuthority` を用意する。
- LSP / Zed / HTTP の 3 入口は、同じ dispatcher への薄い変換層にする。JSON 経由の内部再入と旧 `configure_session` 経路を撤去する。
- エラーコードを定数表にし、公開エラーの enum を統一する。
- **完了条件**: コマンド名の手書き一覧が 1 か所になり、trust の判定も 1 か所になる。

### フェーズ 3: 状態所有の整理と並行性
- Rust: preview の配信状態は HTTP だけが持つ。再描画・配信の関数を統合する。LSP → preview-http の依存は、共通の変換 module に切り出して解消する。
- Rust: 長時間かかる workspace 操作を actor ループから切り離し、1 秒 timeout を撤廃する。asset 解決を memo 化する。SSE を本来のストリーミングに直す。
- TS: `DaemonConnection` と `PreviewController`（判別 union）を導入する。判定を 1 つにまとめる。`adapter.mts` を `ConnectionRouter` / `CommandService` / `EditorEventForwarder` に分け、テスト用の入口を本番クラスから除く（目安は 300 行以下）。
- TS: workspace の増減を `didChangeWorkspaceFolders` で処理し、再起動を crash 時だけにする。
- web/preview: frame と保留中の navigation の順序制御を共通の `PreviewSession` にまとめ、検証を境界で 1 回にする。
- **完了条件**: 「古い非同期結果を捨てる判定」が TS で 1 種類になり、preview 状態の所有者が Rust で 1 つになる。

### フェーズ 4: 知識の正本の一本化
- 組込み構文の記述子テーブルを core に置き、parser・language・web の kind 定数をそこから生成する。`math` fence の不具合をこの過程で修正する。web 側を Enhancer registry にする。
- render-html を sanitize（保護断片の共通処理）、`ResourcePolicy`、block 描画、inline 描画に分割する。navigation の処理は model に一本化する。
- model に `InlineKind::children()` と visitor を置く。asset 参照と SHA-256 hex を newtype にする。
- NodeId の識別キーを、Debug 出力ではなく明示的な正規化から作る。
- core/language を byte offset と内部の種別だけを返す形にし、LSP 形の DTO は transport へ移す。
- protocol registry を宣言的な表にする。TS 型の生成は `ts-rs` / `specta` を検討する。手書きの LSP schema を削除する。fixture の生成を codegen から分離する。
- `contracts/distribution` に target 表とファイル一覧の正本を置き、Python・TS・Rust・Zed に生成する。manifest の検証は Rust 1 crate（Zed と共用）と Python 1 つにする。

### フェーズ 5: tooling・CI の圧縮と掃除
- リリース経路をフェーズ 0 の決定どおりに実装する。
- CI と Release の共通部分を reusable workflow にする。matrix は配布の正本から生成する。workflow を文字列で検査するテスト、未使用の action、`vscode:` キーを削除する。cache を見直す。
- yarn scripts を廃止して入口を mise に一本化する。二次 CLI は `tasks.py` の subcommand に統合し、`tasks.py` を責務ごとに分割する。
- §4.8 の使われていない物を削除する。watch のハッシュ計算は変更されたファイルだけにする。
- xtask の `visible_to` を実依存に基づく最小集合にする。TS にも行数上限と import 境界の lint を適用する。crate 名をディレクトリ名に揃える。
- tsconfig を package 単位にし、package 名で import する。include を glob にする。

### フェーズ 6: テストとドキュメント
- TS のテストを公開振る舞いの単位（fake daemon と fake vscode）に寄せ、harness を一つにまとめる。
- `service_contract.rs` を application へ移す。fixture builder を統一する。parser と language の欠けている観点を追加する。
- ARCHITECTURE / IMPLEMENTATION を新構造に合わせて書き直す。§6 の乖離を解消し、用語を統一する。

### 期待される効果の目安

| 指標 | 現状 | 整理後の目安 |
|---|---|---|
| Rust の行数 | 27k | −5k 前後（プラグイン）＋重複の削減 |
| 依存 crate | 約 250 | 約 140〜160 |
| workspace 操作 1 件の追加に要する変更箇所 | 13〜16 | 4〜6（protocol 定義・application の実装・UI・l10n） |
| 構文機能 1 件の追加に要する変更箇所 | 6 層 | 記述子＋renderer＋enhancer |
| 配布 target / ファイルの追加に要する変更箇所 | 7 | 1（正本）＋再生成 |
| TS で古い非同期結果を捨てる判定 | 8 種類 | 1 |
| Cargo workspace | 3 | 2 |

---

## 9. 着手前に判断が必要な事項

1. **リリース方式**: semantic-release を戻すか、タグを手動で打つ運用に一本化するか。後者なら関連設定と devDependencies 7 個を削除する。
2. **既存 config の扱い**: `[[plugins]]` を含む `.fleximark/config.toml` を、明示的な migration で v3 に変換するか、単に拒否するか。
3. **export registry の互換**: `unsafe_output_used` を含む既存の registry を読み続けるか。
4. **protocol version の運用**: 1.0 までは互換を気にせず版を上げる方針でよいか。version の正本を一つにし、fixture の version は生成する。
5. **VS Code の LSP client**: 自前の rpc / supervisor を維持するか、`vscode-languageclient` を採用するか。フェーズ 3 の設計に影響する。
6. **external preview への配送**: 完全な frame をやめ、revision の通知だけにするか（protocol の変更を伴う）。
7. **xtask の macro 制約**: 宣言用の macro を限定的に許可するか、build.rs による生成で回避するか。

---

## 付録: 主な根拠ファイル

- core: `core/parser/src/lib.rs`、`core/render-html/src/lib.rs`、`core/model/src/lib.rs`、`core/language/src/lib.rs`、`core/engine/src/{session,render,provenance,identity,assets}.rs`、`core/plugin-api/src/lib.rs`
- application: `application/src/{lib,operations,outcomes,document,requests,positions,workspace_config}.rs`、`application/src/native/{assets,notes,plugins,services,workspace,paths}.rs`、`application/src/native/export/`
- transport: `transports/lsp/src/server/{lsp,rpc,commands,standard_commands,workspace_ui,protocol,state,routing,diagnostics}.rs`、`transports/lsp/src/transport/stdio.rs`、`transports/preview-http/src/{lib,state,workspace,protocol,http}.rs`
- contracts: `contracts/protocol/src/{lib,registry}.rs`、`contracts/config/src/lib.rs`、`contracts/typescript/protocol.mts`、`contracts/distribution/`、`contracts/capabilities/`
- TS: `adapters/vscode/src/{adapter,preview-coordinator,runtime-state,runtime-membership,daemon-supervisor,editor-service,commands,runtime-manifest}.mts`、`web/preview/{index,enhance,navigation,runtimes,host}.mts`、`web/workspace-ui/{index,model}.mts`
- tooling: `tooling/tasks/{tasks,distribution,release_artifact,build,_targets}.py`、`tooling/codegen/src/main.rs`、`dev/xtask/policy/workspace.toml`、`.github/workflows/{ci,release}.yml`、`.github/actions/`、`mise.toml`、`release.config.mjs`
- Zed: `adapters/zed/src/{release,adapter}.rs`、`adapters/zed/build.rs`
