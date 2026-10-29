# FlexiMark 大規模リファクタリング作業指示書

この文書は、`REFACTORING_ASSESSMENT.md`（以下「調査レポート」）の結論を実装するための作業指示書である。作業者はサブエージェントを使う Codex を想定する。この文書だけで作業を進められるよう、決定事項・規約・タスク・完了条件を記した。判断に迷った場合は §2 の決定事項と §3 の不変条件を優先し、それでも決められないものは §4.6 の手順で記録して先へ進むこと。

- 起点: ブランチ `feat/kashiwade/big-update`、HEAD `fd5cd1a`。
- 根拠の行番号は `fd5cd1a` 時点のもので、作業が進むとずれる。**シンボル名で grep して位置を特定する**こと。
- 調査レポートの節は「調査 §4.1」のように参照する。

---

## 目次

1. 目的と完了の定義
2. 決定事項（変更禁止）
3. 維持すべき不変条件と機能
4. 作業規約
5. サブエージェント運用
6. フェーズ 0: 基準線の取得
7. フェーズ 1: WASM プラグインの撤去と version のリセット
8. フェーズ 2: 型付き command と application 公開面の一本化
9. フェーズ 3: 状態の所有と並行性
10. フェーズ 4: 知識の正本の一本化
11. フェーズ 5: tooling・CI の圧縮と掃除
12. フェーズ 6: テストとドキュメント
13. 最終検証と引き渡し
14. 付録 A: サブエージェント用プロンプトの雛形
15. 付録 B: 進捗ログの雛形

---

## 1. 目的と完了の定義

### 目的
- WASM プラグイン機能を完全に撤去し、それに依存する抽象を消して core を縮約する。
- 手作業で複写されている知識（コマンド一覧、配布仕様、構文機能、protocol registry）の正本を一か所にする。
- 状態の所有者を一つにし、並行性の欠陥（1 秒 timeout、SSE のポーリング、不要な daemon 再起動）を解消する。
- god モジュールを分割し、テストを公開された振る舞いの単位に寄せる。
- 結果として、新機能を追加するときに変更する箇所を大幅に減らす。

### 完了の定義（全体）
1. §7〜§12 の全タスクが、それぞれの完了条件を満たしてコミットされている。
2. `mise run check` と `mise run test` が成功する。GUI が使える環境では `mise run verify` も成功する。使えない場合は、実行できなかった工程を進捗ログに明記する。
3. §3 の機能チェックリストが全項目で動作する。
4. `cargo tree` に `wasmtime`、`cranelift`、`ed25519-dalek`、`wit-bindgen` が現れない。
5. `docs/ARCHITECTURE.md`、`docs/IMPLEMENTATION.md`、`docs/DEVELOPMENT.md`、`README.md` が新しい構造と一致している。
6. 進捗ログ `REFACTORING_PROGRESS.md` に全タスクの結果、逸脱、残課題が記録されている。

---

## 2. 決定事項（変更禁止）

| # | 決定 | 内容 |
|---|---|---|
| D1 | **WASM プラグインは全廃する** | 実行時、SDK、WIT、manifest、署名、grants、fixture、CI 工程をすべて削除する。代わりの拡張機構は作らない。拡張は組込みコードだけで行う。Zed 拡張自体の WASM ビルド（`wasm32-wasip2`）は対象外であり、維持する |
| D2 | **本ブランチは未リリースのため、後方互換を持たない** | 旧形式を読む fallback、互換の shim、deprecated の alias を作らない。protocol、render frame、config、export registry の旧形式は単に拒否する |
| D3 | **形式の version はすべて 1 にリセットする** | `release.toml` の `protocol_version = 1`、`PROTOCOL_VERSION = 1`、`CONFIG_SCHEMA_VERSION = 1`。`DOCUMENT_SCHEMA_VERSION` は削除する（D1 で用途がなくなる）。runtime manifest の `schemaVersion` は既に 1 なのでそのまま。**リリースまでは、形式を変えても version を上げない** |
| D4 | **プラグイン設定の移行は行わない** | `[[plugins]]` を含む config は「不明なフィールド」として通常どおり拒否する。移行コマンドの追加も、専用のエラー文言の追加もしない。`migration` crate は main 版（v0.16.x の JS 拡張）のユーザーデータを移行する機能として残す。旧 JS プラグインの検出は残すが、警告文は「移行されず、代替機能もない」という内容に改める |
| D5 | **リリースは手動タグに一本化する** | `v*` タグの push で `release.yml` が動く現行方式に統一する。semantic-release 一式（`release.config.mjs`、`tooling/tasks/semantic-release-package.mjs`、関連する devDependencies）は削除する。version は `release.toml` を手で更新し、`mise run generate versions` で反映する。CHANGELOG は手で更新する |
| D6 | **成果はタスク単位でコミットし、push しない** | 現ブランチに Conventional Commits 形式でコミットする。push、PR 作成、タグ作成はしない |
| D7 | **VS Code の LSP client は自前実装を維持する** | `vscode-languageclient` は導入しない。rpc、supervisor の構造は §9 で整理する |
| D8 | **external preview への配送形式は変えない** | 完全な frame を配送する現行方式を維持する。ただし adapter は revision だけを読み、frame 本体（asset を含む）を保持しない |
| D9 | **protocol の TS 生成器は既存の自前実装を維持する** | `ts-rs` や `specta` は導入しない。registry を単一の表にして生成器の入力を整理する（§10 P4-T6） |
| D10 | **新しい依存の追加は最小にする** | Rust crate、npm package、GitHub Action を新たに追加するのは、この指示書が明示した場合だけとする（例: §11 の rust-cache action）。追加する場合は version を固定し、Action は SHA で pin する |
| D11 | **固定 submodule `tooling/xtask-core` は変更しない** | 構造検査の違反は、所有元の source または `dev/xtask/policy` で解消する |

---

## 3. 維持すべき不変条件と機能

### 3.1 不変条件（破ってはならないもの）

**文書処理**
- 内部の位置は UTF-8 byte 範囲だけで持つ。行・文字位置は共有 SourceIndex から境界で生成する。LSP の UTF-8、UTF-16、UTF-32 の交渉を維持する。
- 文書本文の権威経路は `textDocument/didOpen`、`didChange`、`didClose`（全文同期）だけである。
- 更新の候補は、検証に成功したものだけを反映する。描画の失敗・中断・counter 上限のときは、前の frame を保持する。
- preview の revision は単調に増える。読取りで revision を進めない。完全な frame を push で配送し、通知のあとに再取得する経路は作らない。
- navigation は daemon identity、文書 session、文書 version、preview revision、NodeId で照合し、古い要求を拒否する。
- NodeId は既存の IR と照合して維持する（preview の DOM を保持するために必要）。

**セキュリティ**
- raw HTML は浄化する。内部マーカーとの衝突を防ぐ。`data-fleximark-*` の属性は剥奪する。
- 許可できないリンクは `href` を出さずにラベルを描画する。リンクの拒否だけで文書全体の描画を失敗させない。
- HTTP は `127.0.0.1` の動的 port に bind し、page token、Host、Origin を検証する。CSP を維持する。
- workspace の trust。trust がない workspace では、workspace への書込み、theme の読込み、local asset の読込みを行わない。制限モードでも本文の preview はできる。
- asset の containment。cap_std と nofollow による symlink・junction の防御。preview-assets の許可リスト、包含チェック、hash 検証。
- webview bridge の token 認証。

**export**
- journal、ownership、recovery による transaction 性を維持する。

**依存方向**
- core は I/O、protocol DTO、filesystem、browser bundle に依存しない。
- `contracts/protocol` は application、core、runtime の型を import しない。

### 3.2 機能チェックリスト（各フェーズの終わりに確認する）

自動テストで確かめられない項目は、`mise run test editor` か手動 smoke で確認する。その結果を進捗ログに残す。

| # | 機能 | 主なテスト |
|---|---|---|
| F1 | VS Code: 埋め込み Webview の preview、ライブ更新、DOM の保持 | `tests/typescript/preview-client.test.mts`、`apps/server/tests/preview_session.rs`、editor テスト |
| F2 | VS Code: 外部ブラウザの preview（SSE） | `browser-host.test.mts`、`browser-navigation-runtime.test.mts`、preview-http のテスト |
| F3 | source から preview への明示移動、browser から source への移動 | `browser-navigation-runtime.test.mts`、`preview-coordinator.test.mts` |
| F4 | 単独の Markdown と未保存文書の preview（workspace なし） | `standalone-runtime.test.mts`、`standalone.test.mts` |
| F5 | multi-root workspace、workspace の追加・削除 | `multi-root-runtime.test.mts` |
| F6 | 制限モード（untrusted）での本文 preview と、書込み操作の拒否 | editor テストの restricted、application のテスト |
| F7 | 補完、Semantic Tokens、hover、document symbols、diagnostics、Code Action | `apps/server/tests/language_session.rs` |
| F8 | workspace の初期化、theme の編集 | application、server のテスト |
| F9 | ノート作成（カテゴリ・テンプレート・名前・ファイル名展開） | `note-options.test.mts`、`application/src/native/notes.rs` のテスト |
| F10 | admonition の収集 | application、server のテスト |
| F11 | HTML export と、上書き確認（acknowledge） | `application/src/native/export/tests.rs`、`export-ack.test.mts` |
| F12 | main 版ユーザーデータの明示 migration（CLI と editor） | `migration` crate のテスト、CLI のテスト |
| F13 | Zed: Code Action、executeCommand、`window/showDocument`、外部ブラウザ | Zed のテスト、`standard_commands` 系のテスト |
| F14 | daemon の crash からの回復と再接続 | `daemon-supervisor.test.mts`、`daemon-runtime.test.mts` |
| F15 | 配布 archive の組立、manifest、VSIX、Zed ZIP | Python のテスト、`mise run package all` |
| F16 | 数式、mermaid、ABC（再生を含む）、YouTube、alert、admonition、表、タスクリストの描画 | `core/render-html/src/tests.rs`、`preview-extended.test.mts` |

---

## 4. 作業規約

### 4.1 コマンド

| 用途 | コマンド |
|---|---|
| 初回の準備 | `mise install && mise run setup` |
| 静的検査（生成物、構造、型、lint、format） | `mise run check` |
| unit テスト（Python、Rust、Node） | `mise run test` |
| VS Code の結合テスト（GUI が必要） | `mise run test editor`（Linux では `xvfb-run -a`） |
| 全工程 | `mise run verify` |
| 生成物の更新 | `mise run generate protocol \| l10n \| versions \| architecture \| all` |
| 局所的な確認 | `cargo test -p <crate>`、`cargo clippy -p <crate> --all-targets`、`yarn tsc -b <tsconfig>` など |

- コマンドに `--dry-run` を付けると、実行計画を表示するだけで終了する。

### 4.2 コミットの関門

各コミットの前に、最低限 `mise run check` と `mise run test` が成功していること。どうしても途中の状態でコミットする必要がある場合は分割をやり直し、**ビルドが壊れたコミットは作らない**。

### 4.3 コードの規約

- **Role 行**: すべての crate root と module root の先頭に `//! Role: <英語 1 文>` を付ける。責務が変わったら Role も直す。同じ文面の使い回しは禁止する（調査 §4 で `Deliver LSP and RPC…` の使い回しを指摘済み）。
- **行数上限**: production と test の各ファイルは 1000 code lines 未満とする。Rust は xtask が検査する。TS はフェーズ 5 で lint に加えるが、それ以前の新規・変更ファイルも上限を守る。
- **生成物は手で編集しない**: `contracts/typescript/protocol.generated.mts`、`contracts/schemas/*.json`、`tests/fixtures/protocol/*`、`architecture/**`、`adapters/zed/architecture/**`、l10n は `mise run generate` で更新する。
- **コメント**: 既存の密度と言語（コードは英語）に合わせる。docs は既存どおり日本語で、簡潔な常体で書く。
- **本番コードの panic**: 新しく `unwrap` や `expect` を書かない。invariant を表すときは型で表現するか、`Result` を返す。
- **互換コードを作らない**（D2）: 削除した API の re-export や `#[deprecated]` も作らない。
- **テスト用の入口を本番クラスに置かない**: テスト専用の API は、テストビルドか extension の test API 層に置く。

### 4.4 コミットメッセージ

- 形式は Conventional Commits とし、既存の履歴に合わせる（例: `refactor(engine)!: drop plugin hooks`）。
- 形式・挙動を変えるものは `!` を付ける。本文に、対応するタスク ID（例: `Task: P1-T2`）と要点を書く。
- 1 タスク 1 コミットを基本とする。大きなタスクは、ビルドが通る単位に分けて複数コミットにしてよい。

### 4.5 docs の更新時期

- 構造が変わるタスクでは、そのタスクの中で `docs/ARCHITECTURE.md` と `docs/IMPLEMENTATION.md` の該当箇所を最小限更新する。
- 全面的な書直しはフェーズ 6 で行う。

### 4.6 判断に迷ったとき


1. まず §2 の決定事項と §3 の不変条件に照らす。
2. 決まらない場合は、**変更量が少なく、元に戻しやすい選択**をとる。
3. 選んだ内容を、進捗ログの「判断の記録」に書く。
4. その判断が利用者の確認を要するものであれば、「残課題」にも記載する。
5. 作業は止めずに先へ進む。

ただし、次の場合は、そのタスクを保留にして残課題に書き、依存のないタスクを先に進める。

- §3.1 の不変条件を破らなければ実現できない場合
- 機能を削除しなければならない場合

---

## 5. サブエージェント運用

### 5.1 役割

| 役割 | 責務 |
|---|---|
| **統括者（メインの Codex）** | タスクの順序と割当て、共有ファイルの編集、生成物の再生成、関門の実行、差分のレビュー、コミット、進捗ログの更新 |
| **実装者（サブエージェント）** | 割り当てられた 1 タスクの実装と、局所的な検証。**コミットしない**。**所有範囲の外を編集しない** |
| **レビュアー（サブエージェント、読取り専用）** | フェーズの終わりに、差分を完了条件と §3 に照らして検査し、指摘を返す |

### 5.2 共有ファイル（統括者だけが編集する）

- `Cargo.toml`（root）、`Cargo.lock`、`package.json`（root）、`yarn.lock`、`release.toml`、`mise.toml`
- `dev/xtask/policy/**`、`.github/**`
- 生成物すべて（§4.3）
- `docs/**`、`README.md`、`REFACTORING_PROGRESS.md`

実装者がこれらの変更を必要とする場合は、必要な差分を報告に書く。統括者がそれを適用する。

### 5.3 並列化の規則

- 並列に動かせるのは、**所有範囲（パス）が重ならないタスク**だけとする。各フェーズの「並列化」の項に従う。
- 作業ツリーが共有される場合、`mise run generate` と `mise run check` は統括者だけが実行する。実装者は `cargo test -p`、`cargo clippy -p`、個別の `tsc` などの局所的な検証にとどめる。
- Codex が worktree を分けて使える場合は、タスクごとに分けてもよい。その場合も、統合とコミットは統括者が一本の履歴にまとめる。
- protocol の型を変更するタスク（P1-T3、P2-T2、P4-T6 など）が完了して再生成するまでは、それに依存する TS 側のタスクを始めない。

### 5.4 実装者が返す報告

付録 A の雛形に従い、次を返させる。

1. 変更したファイルの一覧
2. 実装の要点
3. 実行した検証とその結果
4. 共有ファイルに必要な差分
5. 未解決の事項
6. 完了条件の各項目の自己判定

### 5.5 レビューの観点

各フェーズの終わりに、統括者はレビュアーを起動する。観点は次のとおり。

- 完了条件の充足
- §3 の不変条件の違反
- 互換コードや fallback の混入
- 本番コードの panic
- Role 行と行数上限
- 削除漏れ（`git grep` で旧シンボル名を確認する）
- テストを不当に弱めていないか（assert の削除や skip）

指摘への対応は統括者が判断し、進捗ログに記録する。

---

## 6. フェーズ 0: 基準線の取得

### P0-T1 基準線の記録（統括者）

- `mise run setup` の後に、`mise run check` と `mise run test` を実行する。既存の失敗があれば、そのまま進捗ログの「基準線」に記録する。これは作業中に起きた失敗と区別するためである。
- 次の計測値を記録する。フェーズ 6 の後に同じ方法で再計測し、比較する。
  - `cargo tree -p fleximarkd -p fleximark-cli -e normal,build --prefix none | sort -u | wc -l`
  - 言語別の非空行数（調査 §2.1 と同じ除外条件）
  - `mise run test` の所要時間
- `REFACTORING_PROGRESS.md` を付録 B の雛形で作成し、コミットする（`chore: start refactoring progress log`）。

### P0-T2 地図の確認（統括者、読取りのみ）

- 調査レポートの §3〜§5 を読み、本指示書のタスクとの対応を確認する。
- 指摘された症状が `fd5cd1a` 以降の変更で既に解消していないかを grep で確かめる。解消済みのものは進捗ログに記録し、該当する作業を省略する。

---

## 7. フェーズ 1: WASM プラグインの撤去と version のリセット

**目的**: プラグインと、それに依存する抽象を完全に除き、core の更新処理を「parse → NodeId 照合 → 反映」の 2 段にする。

**順序**: 次の順で、それぞれビルドが通る状態でコミットする。P1-T1 → P1-T2 → P1-T3 → P1-T4 → (P1-T5 ∥ P1-T6) → P1-T7。

### P1-T1 外側からのプラグイン切離し

- **所有**: `application/**`、`apps/**`、`transports/**`、`contracts/config/**`、`migration/**`
- **作業**
  1. `application/src/native/plugins.rs`、`application/src/test_plugins.rs`、`application/tests/engine_pipeline.rs` を削除する。`engine_pipeline.rs` にプラグインと無関係な観点（文書更新の順序、失敗時の保持など）があれば、application か engine の通常のテストへ移す。
  2. `native/services.rs` の `configured_host` など、プラグインホストを構成するコードを削除する。
  3. `SessionRegistry` と workspace 状態からプラグインを外す。
     - `host: Option<Arc<dyn PluginExecutor>>` を削除する。
     - `configure_workspaces` の引数のタプルから `Arc<dyn PluginExecutor>` を外す。
     - 再構成のループをプラグインに依存しない形にする。
     - この段階では engine の API がまだ executor を要求するため、空の executor を内部で渡す暫定実装でよい（P1-T2 で消す）。
  4. `contracts/config` から次を削除し、`fleximark-plugin-sdk` への依存を外す。
     - `ConfiguredPlugin`、`plugins` フィールド、プラグイン関連の error variant と検証、plugin grants
     - config の JSON Schema の該当部分（生成元を直す）
  5. export の unsafe HTML 段階を削除する。
     - `ExportReceipt.unsafe_output`、registry の `unsafe_output_used`、警告用の aside
     - `prepare_safe_export` → `compose_portable` → `apply_unsafe` を単一段にする
     - registry の旧形式は読まない（D2）
  6. `ServiceError::PluginConfig` を削除する。`reject_linked_path`（`application/src/native/paths.rs`）は、theme や asset から呼ばれても正しい意味になる variant（例: `LinkedPath`）を返すようにする。
  7. LSP のプラグイン診断を削除する。対象は `transports/lsp/src/server/diagnostics.rs` の該当部分、`DocumentSession::plugin_diagnostics`、telemetry の `plugin_failure`。
  8. CLI のテスト `default_session_does_not_load_workspace_plugins` などを削除する。
  9. `migration/src/lib.rs` の旧 JS プラグイン警告を D4 の文面に改め、`plugins: vec![]` を削除する。
  10. untrusted のときにも空の PluginHost を作っている経路（`native/plugins.rs`、`theme.rs`）を除く。
- **完了条件**
  - application、apps、transports、contracts/config が `fleximark-plugin-host` と `fleximark-plugin-sdk` に依存しない。
  - `mise run test` が成功する。
- **コミット**: `refactor!: detach WASM plugins from application and transports`

### P1-T2 core からのプラグイン除去

- **所有**: `core/**`、`application/**`（API 追従のみ）、`transports/**`（API 追従のみ）、`apps/**`（API 追従のみ）
- **作業**
  1. `CancellationToken` を移設する。
     - 現在は `core/plugin-api` にあり、application、LSP（`transports/lsp/src/cancellation.rs`、`server/state.rs`、`server/routing.rs`）、CLI、engine が参照している。
     - parse と render は同期処理で、途中にキャンセルを確認する箇所がない。そのため **engine の公開 API からキャンセルを外し**、確認は application が描画の前と export の各段の間で行う。
     - 型は `application`（例: `application/src/cancellation.rs`）が所有する。LSP はそれを使う。
  2. engine を縮約する。
     - `provenance.rs` と、前処理の edit map の合成・復元を削除する。
     - `CandidateHooks`、`SourceCandidateInput`、`prepare_source_candidate`、`plugin_tests.rs` を削除する。
     - parse の後の `document.validate` の二重実行（parser と session）を一回にする。
     - 公開面を `open(uri, version, source, encoding, config)`、`change`、`resync`、`set_render_config`、`render` に集約する。`open_configured`、`reconfigure`、`adopt_reconfiguration`、`clear_configuration`、`reconfigure_render`、`render_configured`、`render_with_plugins`、`PluginRenderSnapshot` を削除する。
     - `generate_frame` は `&self` にする。
  3. `RenderConfig` から次を削除する。`plugin_set_hash`、`plugin_generation`、`for_plugins`、`validate_for(plugins)`。fingerprint は config と theme の hash、および generation だけにする。
  4. `RenderSnapshot.annotations` と、その script 出力・fingerprint への算入を削除する。
  5. export の型状態（`PreparedExport`、`ResolvedExport`、`compose_portable`、`apply_unsafe_export_html`）と `EngineError::UnsafeExportPolicy` を削除し、portable な描画 → 組立ての直列処理にする。
  6. model を縮約する。
     - `SourceProvenance::Derived/Generated`、`TransformId`、`GeneratedAnchor`、`AnchorAffinity`、複数 range と `primary_range_index`、関連する `ValidationError` を削除し、`Block.range: SourceRange` 一つにする。
     - `Block.attributes` を削除する。
     - `DOCUMENT_SCHEMA_VERSION`、`schema_version`、プラグインとの交換用の serde 派生を削除する。`identity.rs` が serde に依存している箇所は、明示的な正規化に置き換えてよい（P4-T3 の先取り）。
  7. `BlockKind::Plugin{namespace,name}` を `BlockKind::Directive{name}`（不明な directive の受け皿）に改名する。
  8. `EngineError::Plugin(String)` を廃止し、実際の失敗の種類で variant を分ける。
     - 対象は `session.rs`、`assets.rs`、`render.rs` の各箇所。
     - キャンセルは engine から外したので、engine のエラーにキャンセルを含めない。
     - テストで `"operation cancelled"` という文字列を比較している箇所は、型による判定に直す。
  9. `core/plugin-api` を削除し、依存していた crate の `Cargo.toml` から外す。依存していたのは application、apps/cli、apps/server、engine、transports/lsp である。
- **完了条件**
  - `git grep -n -E 'PluginExecutor|plugin_api|PreprocessedSource|EditMap|TransformId|annotations|unsafe_output|PluginRun'` が、docs と調査レポート以外でヒットしない。
  - engine の公開関数が上記の集約後の形になっている。
  - `mise run test` が成功する。
- **コミット**: `refactor(core)!: remove plugin hooks and provenance remapping`（必要なら engine と model で分割する）

### P1-T3 プラグイン crate・fixture の削除と version のリセット

- **所有**: 統括者（共有ファイルを含む）
- **作業**
  1. `runtime/wasm-plugins`、`contracts/plugin`、`tests/fixtures/plugin-component`、`contracts/schemas/plugin-manifest.schema.json` を削除する。
  2. root の `Cargo.toml` から次を外す。
     - members と exclude の該当項目
     - `wasmtime`、`wasmtime-wasi`、`ed25519-dalek`（ほかの用途がないことを `git grep` で確認する）
     - その上で `Cargo.lock` を更新する。
  3. version をリセットする（D3）。
     - `release.toml` の `protocol_version = 1`
     - `contracts/protocol/src/lib.rs` の `PROTOCOL_VERSION = 1`
     - `contracts/config/src/lib.rs` の `CONFIG_SCHEMA_VERSION = 1`
     - テストと fixture の config 文字列 `schema_version = 2` を `1` にする（`git grep -n 'schema_version *= *2'` で網羅する）
     - `application/src/native/workspace.rs` のテンプレートも直す
  4. 「旧 protocol version を拒否する」テスト（例: `apps/server/tests/language_session.rs` の `PROTOCOL_VERSION - 1`）を、version 0 や不正値の拒否に書き換える。
  5. `mise run generate all` で protocol の TS・schema・fixture・version・l10n・図を再生成する。
- **完了条件**
  - `git grep -n -E 'protocolVersion\D*7|PROTOCOL_VERSION.*7|schema_version *= *2'` がヒットしない。
  - `cargo tree` に wasmtime 系がない。
  - `mise run check` と `mise run test` が成功する。
- **コミット**: `refactor!: delete plugin crates and reset format versions to 1`

### P1-T4 tooling・xtask・CI からのプラグイン工程の削除

- **所有**: 統括者
- **作業**
  1. `tooling/tasks/tasks.py` から fixture 用の工程を削除する。対象は fmt、clippy、wasm32 check、test、xtask、図の 6 つで、setup の文言のうちプラグイン部分も含む。
  2. 関連する Python テストを修正する（`test_task_commands.py`、`test_verification_phases.py` など）。
  3. `dev/xtask/policy/workspace.toml` を修正する。
     - `trusted_macros` から `wasmtime::component::bindgen` を外す。
     - `visible_to` のプラグイン行を外す。
     - fixture 用の policy を削除する。
  4. `.github/dependabot.yml` と `.github/actions/setup-toolchain/action.yml` から、fixture の lockfile と cache key を外す。
  5. 次の 2 つは Zed 用なので残す。`rustup target add wasm32-wasip2`、Zed の `trusted_macros = zed::register_extension`。
- **完了条件**
  - `git grep -n -i -E 'plugin-component|wasm-plugins|plugin-host|plugin-sdk|fleximark-plugin'` が docs と調査レポート以外でヒットしない。
  - `mise run check` と `mise run test --dry-run` の計画にプラグインの工程がない。
- **コミット**: `build!: drop plugin fixture and wasmtime tooling`

### P1-T5 TypeScript 側の追従（P1-T3 の再生成後に開始する）

- **所有**: `adapters/vscode/**`、`web/**`、`tests/typescript/**`、`contracts/typescript/*.mts`（生成物を除く）
- **作業**
  1. annotations の DOM 処理を削除する。対象は `web/preview/index.mts` の該当部分と `preview-client.test.mts` の該当テスト。
  2. 設定 watcher の glob `plugins/**` を外す（`adapters/vscode/src/events.mts`、`wiring.test.mts`）。
  3. `adapters/vscode/package.json` と nls の untrusted 説明文から「WebAssembly components」を削除する。
  4. version 7 を前提にした TS テストを直す。
- **完了条件**: `git grep -n -i -E 'annotation|webassembly|plugins/\*\*' -- adapters web tests/typescript` の結果に、意図しないものが残っていない。
- **コミット**: `refactor(vscode)!: remove plugin annotations and settings`

### P1-T6 capabilities・README の追従（P1-T5 と並列に進めてよい）

- **所有**: 統括者
- **作業**: `contracts/capabilities/*.json` と `README.md` から WASM プラグインの記述を削除する。`contracts/capabilities` の未参照 JSON は P5-T4 で一括削除するので、ここでは README と、参照されているものだけを直す。
- **コミット**: `docs: remove WASM plugin references`

### P1-T7 フェーズ 1 のレビューと docs の最小更新

- `docs/ARCHITECTURE.md` と `docs/IMPLEMENTATION.md` を次のとおり更新する。
  - 文書更新の順序（5 段 → 2 段）
  - version の記述（protocol、config を 1 にし、document schema、plugin API、manifest の記述を削除）
  - plugin-api、wasm-plugins、unsafe HTML の段階、grants の記述を削除
- レビュアーを起動し、指摘を処理する。
- §3.2 の F1〜F16 を確認する。
- P0-T1 と同じ方法で依存 crate 数を計測し、進捗ログに記録する。

---

## 8. フェーズ 2: 型付き command と application 公開面の一本化

**目的**: workspace 操作の定義・認可・配送を一か所にする。新しい操作を追加するときの変更箇所を 13〜16 から 4〜6 に減らす。

**順序**: P2-T1 → P2-T2 → P2-T3 → (P2-T4 ∥ P2-T5) → P2-T6 → P2-T7。

### P2-T1 エラーモデルの統一

- **所有**: `application/**`、`transports/**`、`contracts/protocol/**`、`apps/**`
- **作業**
  1. application の公開エラーを一つの型にまとめる。例: `ApplicationError { kind: ErrorKind, message, source }`。`ErrorKind` は `NotFound`、`Denied`（trust）、`Conflict`、`Invalid`、`Cancelled`、`Internal` とする。
  2. 統合対象は `SessionError`、`ServiceError`、`OperationError`、`ServiceOperationError`、`ConversionError` の外部向け部分。`SessionError::Engine(String)` が「設定が未登録」「重複 root」を兼ねている問題もここで解消する。
  3. `execute_operation` の内部で文字列に潰している箇所（`operations.rs`）をなくす。
  4. JSON-RPC のエラーコードを `contracts/protocol` の定数表にまとめる。transport は `ErrorKind` からコードへの対応表だけを持つ。
  5. `let _ = error;` でエラーを捨てている箇所（約 9 か所。`git grep -n 'let _ = error'`）を、構造化ログへの出力に置き換える。
  6. `log_operational_event` を一本にする。preview-http の stub を削除し、共通の logger を使う。
  7. `Error::source` が下位のエラーを飛ばしている問題（`core/engine/src/error.rs`）を直す。
- **完了条件**
  - transport の中に数値のエラーコードの直書きがない。
  - `git grep -n 'let _ = error'` がヒットしない。
- **コミット**: `refactor!: unify application errors and RPC error codes`

### P2-T2 `WorkspaceCommand` を protocol に定義する

- **所有**: `contracts/protocol/**`、`tooling/codegen/**`（統括者が再生成する）
- **作業**
  1. `#[serde(tag = "command", rename_all = "camelCase")]` の `WorkspaceCommand` enum を定義する。variant は操作ごとに型付きの引数を持つ。
     - 対象の操作: `initializeWorkspace`、`editTheme`、`createNote`、`getNoteOptions`、`collectAdmonitions`、`exportHtml`、`acknowledgeExport`、`planMigration`、`applyMigration`、`openPreview`（Zed 用など、現存する操作をすべて洗い出して含める）
  2. 各操作の記述子表を一つ置き、ここから一覧・capability・Code Action を導出する。
     - 記述子の項目（例）: `name`、`requires_document`、`requires_workspace`、`requires_trust`、`code_action`（Zed に Code Action として出すか）、`browser_ui`（workspace UI から呼べるか）、`cli`（CLI で公開するか）
     - **macro は使わず**、`const` の配列と `match` で書く（xtask の制約）。
  3. 結果の型も操作ごとの型付き enum にする（`CommandResult`）。message の形を一つに統一する（調査 §4.3 の `CommandResult.message` のずれを解消する）。
  4. 平坦な `ExecuteCommandParams`（全操作分の Option フィールドを持つ型）を削除する。
  5. note options の結果を、既存の `GetNoteOptionsResult` / `NoteCategoryOption` の型で返す（手組みの `json!` を廃止する）。
  6. workspace UI の HTTP API（`/state`、`/command` の要求と応答）も protocol の型として定義する。
  7. codegen を対応させ、TS 型、記述子表（コマンド名の一覧を含む）、schema を生成する。
- **完了条件**
  - Rust と TS のどちらにも、コマンド名の手書き一覧が存在しない（生成物と記述子表を除く）。
  - protocol のテストが全 variant の往復を検証している。
- **コミット**: `feat(protocol)!: add typed workspace command union`

### P2-T3 application の公開面を一本化する

- **所有**: `application/**`
- **作業**
  1. `pub mod native` を `pub(crate)` にする。外部に出す API は次の 3 つに整理する（module 単位でよく、crate の分割は不要）。
     - `DocumentStore`: 文書と preview frame
     - `WorkspaceAuthority`: root、trust、検証済み config のキャッシュ
     - `WorkspaceOps`: `execute(command: WorkspaceCommand, context) -> Result<CommandOutcome, ApplicationError>`
  2. trust の判定は `WorkspaceOps` の入口一か所だけにし、記述子の `requires_trust` を使う。現在の判定 6 か所（`operations.rs`、`commands.rs`、`standard_commands.rs`、`workspace_ui.rs`、`lib.rs`、CLI）を撤去する。
  3. `WorkspaceAuthority` に検証済み config をキャッシュする。
     - export 1 回で `validate_config` を 3 回以上読み直している問題を解消する。
     - キャッシュの無効化は、config の変更通知（`reconfigureWorkspace`）を契機にする。
  4. 次の 3 つを transport へ移す（application は byte offset と型付きの値だけを扱う）。
     - LSP 形の DTO（`application/src/requests.rs` の `DidOpenParams`、`Position` など）
     - 位置変換（`positions.rs`）
     - daemon ID の検証（`lib.rs`）
  5. タプルの戻り値（`workspace_statuses() -> Vec<(String,bool,bool)>` など）を名前付きの struct にする。trust の状態は `Restricted`、`Trusted`、`Invalid(error)` の enum にする（D1 で `enabled` の意味がなくなる）。
  6. migration を `planMigration` → 確認 → `applyMigration` の 2 段で公開する。`MigrationOutput{stdout}` のように application が文言を組み立てることをやめ、型付きの outcome を返す。文言は transport と CLI が作る。
  7. `default_note_root()`（`contracts/config/src/lib.rs`）の値 `"."` とテンプレートの `"notes"` の不一致を解消する。どちらを正とするかは、既存のテストと README で利用者に示している値に合わせる。判断の結果は進捗ログに書く。
  8. 次の重複を解消する。
     - `sha256` が二重に定義されている（`integrity.rs`、`export/filesystem.rs`）。
     - 安全な相対パスの判定が二つある（`workspace.rs`、`export/filesystem.rs`）。
  9. `export_html_transaction`（約 260 行）を段階ごとの関数に分割する。挙動は変えない。
  10. application が `preview-assets` の自由関数 `read()` を直接呼んでいる箇所（呼ぶたびに manifest を読み直す）を、composition root から注入したインスタンス経由にする。
- **完了条件**
  - transport と CLI が `application::native::*` を参照していない。
  - trust の判定が一か所である。
  - `mise run test` が成功する。
- **コミット**: `refactor(application)!: expose a single typed operation surface`（必要なら分割する）

### P2-T4 transport の配送を一本化する

- **所有**: `transports/**`
- **作業**
  1. `fleximark/executeCommand`、`workspace/executeCommand`（Zed）、HTTP `/command`（workspace UI）の 3 つの入口を、**それぞれ要求を `WorkspaceCommand` に変換するだけの薄い層**にする。全入口が同じ dispatcher を通る。
     - `standard_commands.rs` の `COMMANDS` と Code Action の表、`commands.rs` の文字列 match、`workspace_ui.rs` の許可リストと独自の `Command` struct、`rpc.rs` の capability 一覧を、記述子表からの導出に置き換える。
  2. JSON を経由した内部の再入をなくす。
     - `standard_execute_command` が `json!` を組み立てて `create_preview` を呼び、`/result/url` で読み戻している処理は、型付きの関数呼び出しにする。
     - `lsp_initialize` が旧 RPC 形式の JSON を作って `configure_session` に渡している処理（旧 RPC の残骸）は、型付きの初期化関数を直接呼ぶ。
  3. protocol version の検査を一か所にする（現在は `lsp.rs` と `rpc.rs`）。
  4. 旧 RPC の名前を整理する。
     - `rpc.rs` を custom method の配送であると分かる名前（例: `fleximark_methods.rs`）にする。
     - Role の文面を各 module の実際の責務に直す。
  5. `fleximark/attachDocument` の役割を確認する。didOpen の後に session ID を返すためだけのものなら、ARCHITECTURE に「session ID の取得用であり、本文の経路ではない」と明記して残す。不要なら削除する。判断を進捗ログに書く。
  6. 手組みの `json!` を protocol 型に置き換える。対象は Code Action、diagnostics、hover、initialize の capability、migration の結果など。LSP 標準の構造は、transport 内の型付き struct で表してよい。
  7. 未使用の protocol フィールドを削除する（`embedded_html`、`structured_preview`、`open_external`）。adapter 側の送信の削除は P2-T6 で行う。
  8. preview server を bind できないと daemon が panic する問題（`routing.rs` の `.expect("loopback preview server must start")`）を直し、`Result` で起動の失敗として返す。
- **完了条件**
  - `git grep -n -E '"(initializeWorkspace|createNote|exportHtml|collectAdmonitions|acknowledgeExport)"' -- transports` が記述子の参照以外でヒットしない。
  - LSP、Zed、HTTP の各入口について、全コマンドの往復テストがある（`apps/server/tests` に追加する）。
- **コミット**: `refactor(lsp)!: route all workspace commands through one dispatcher`

### P2-T5 CLI の統合（P2-T4 と並列に進めてよい）

- **所有**: `apps/cli/**`、`apps/server/**`（composition root のみ）
- **作業**
  1. CLI も `WorkspaceOps` を通す。
     - CLI 用の `WorkspaceAuthority` を作る。workspace の判定（祖先 directory の config 探索）はここに閉じる。
     - `--trusted-workspace` はこのとき trust を与える手段とする。
  2. `create-note` にカテゴリ、テンプレート、名前の引数を追加する（daemon 側の機能と一致させる）。
  3. export と ack-export で重複している destination の解決処理を一つにまとめる。
  4. 開発用の benchmark コード（`apps/cli/src/main.rs` の該当部分）を製品バイナリから外す。移す先は `dev/xtask` か Rust の bench/test。
  5. `migrate` を plan と apply の 2 段の API で実装する（dry-run、`--recover`、`--settings` は維持する）。
- **完了条件**
  - CLI が `application::native` を参照しない。
  - CLI のテストが成功する。
- **コミット**: `refactor(cli)!: use the shared workspace operation API`

### P2-T6 TS 側の追従（P2-T2 の再生成後に開始する）

- **所有**: `adapters/vscode/**`、`web/workspace-ui/**`、`tests/typescript/**`、`contracts/typescript/*.mts`（生成物を除く）
- **作業**
  1. `adapters/vscode/src/commands.mts`、`web/workspace-ui/model.mts`、`web/workspace-ui/index.mts` が、生成されたコマンド一覧と型を使うようにする。手書きの `Category`、`CommandResult`（`model.mts`）は削除する。
  2. adapter からのコマンド実行を一本化する。
     - 文書の有無で LSP 標準の `workspace/executeCommand` と `fleximark/executeCommand` を切り替えている現状（`adapter.mts`、`editor-service.mts`）をやめる。
     - VS Code は `fleximark/executeCommand` に統一する。標準の入口は Zed 用として daemon に残す。
  3. migration を「plan を表示 → 利用者が確認 → apply」の 2 段にする。
  4. 送信している未使用の capability（`embeddedHtml`、`structuredPreview`、`openExternal`。`editor-service.mts`）を削除する。
  5. `selectWorkspaceUri` の重複（`commands.mts` と `workspace-selection.mts`）を l10n 対応の一つにまとめる。
  6. l10n の漏れを補う。対象は `package.json` の workspace / previewAt / migrate のタイトル、`adapter.mts` と `editor-service.mts` のエラー文言。
- **完了条件**
  - TS にコマンド名の文字列リテラルが残っていない（生成物を除く）。
  - workspace UI の全コマンドが動く。
  - `acknowledgeExport` を含め、記述子の `browser_ui` に従って許可されている。
- **コミット**: `refactor(vscode)!: consume generated workspace commands`

### P2-T7 フェーズ 2 のレビュー

- レビュアーを起動する。
- 次の 2 点を進捗ログに記録する。
  - 「新しい workspace 操作を 1 件追加するときに変更するファイル」の一覧。目安は 4〜6（protocol の定義と記述子、application の実装、UI、l10n）。
  - 実際にダミーの操作で数え、数え終わったら破棄する。
- §3.2 を確認する。

---

## 9. フェーズ 3: 状態の所有と並行性

**目的**: preview 状態と接続状態の所有者を一つにする。1 秒 timeout、SSE のポーリング、不要な daemon の再起動を解消する。

**並列化**: Rust 側（P3-T1〜P3-T3）と TS 側（P3-T4〜P3-T7）は所有範囲が重ならないので、並列に進めてよい。protocol を変更する場合（P3-T3）は、統括者が再生成してから TS 側に取り込む。

### P3-T1 preview 状態の所有者を一つにする（Rust）

- **所有**: `application/**`、`transports/**`、新しく作る crate
- **作業**
  1. 現在 preview 状態を持っているのは次の 3 か所である。
     - application の `DocumentSession.previews`
     - LSP の `Server.preview_states`（token、`delivered_revision`、`explicit_jump`）
     - HTTP の `PreviewServer.pages`（frame の複製、`document_version`、sequence）
  2. これを次の所有に整理する。
     - **application**: frame（`Arc`）と revision の正本。
     - **HTTP**: browser への配信状態（page token、SSE の sequence）だけ。frame は application の `Arc` を参照し、複製しない。
     - **LSP**: LSP 固有の状態（explicit jump の要求など）だけ。revision と version の複製を持たない。`mark_preview_document_version` のような手作業の同期を削除する。
  3. 「文書の全 preview を再描画して配信する」処理（`lsp.rs` と `rpc.rs` の 2 か所）を一つの関数にまとめる。
  4. application → protocol の frame 変換を、LSP と HTTP の共通の場所へ移す。
     - 現在、変換は `preview-http/src/protocol.rs` にあり、LSP が preview-http に依存している。
     - 新しい crate（例: `transports/wire-mapping`、crate 名 `fleximark-wire-mapping`）を作り、LSP と HTTP の両方がそれに依存する形にする。
     - LSP → preview-http の依存は、HTTP server の起動に必要な分だけ残す。
     - xtask の policy と ARCHITECTURE の表を更新する。
  5. `transports/lsp/src/server/protocol.rs` の素通しの包み関数を削除する。
- **完了条件**
  - frame と revision の正本が application だけにある。
  - `git grep -n 'mark_preview_document_version'` がヒットしない。
  - preview_session と browser navigation のテストが成功する。
- **コミット**: `refactor(preview)!: make application the single owner of preview frames`

### P3-T2 actor ループと I/O を分ける（Rust）

- **所有**: `transports/**`、`application/**`
- **作業**
  1. 時間のかかる workspace 操作（export、collectAdmonitions、createNote、migration など）を、`Server` の actor ループの上で同期実行しないようにする。
     - ループは入力（文書の snapshot、検証済み config、asset の解決結果）を `Arc` で取り出し、worker thread で実行させる。
     - 完了はループへのイベントで受け取り、応答する。
     - 状態の更新（例: export registry の記録）は、ループに戻ってから行うか、application の transaction に閉じる。
  2. HTTP の workspace handler の `recv_timeout(1s)`（`transports/lsp/src/transport/stdio.rs`）を撤廃する。
     - 代わりに、server の終了時に待機中の要求を確実に失敗させる。
     - 利用者向けには、長時間の操作中も UI が応答し続けるようにする（LSP の要求を止めない）。
  3. `didChange` のたびに全画像を読み直す `refresh_assets` を memo 化する。参照集合と、各ファイルの (mtime, size) を key にする。config か theme が変わったときは無効化する。
  4. HTTP の要求ごとに作っている deadline 監視スレッド（10ms 間隔の sleep）を廃止し、socket の read/write timeout で代える。
  5. SSE を本来のストリーミングにする。
     - `Connection: close` と `retry: 250` によるポーリングをやめる。
     - 接続を保持し、更新時に完全な frame を送る。keep-alive のコメントを 15 秒程度ごとに送る。
     - SSE の接続は通常の worker pool とは別に扱い、上限（例: 同時 16 接続）を設ける。上限を超えたら 503 を返す。
     - 再接続のときは、最新の frame をまず送る。
  6. `remap_provenance` 由来の O(n×m) の処理は P1-T2 で消えている。消えていなければここで消す。
- **完了条件**
  - 10 秒程度かかる export を模したテストを追加する（遅延を注入できる仕組みはテスト用に用意する）。そのテストで、その間も LSP の補完要求に応答し、export が成功を返すこと。
  - SSE のテストで、1 接続のまま複数回の更新を受け取れること。
  - asset の memo 化のテスト。
- **コミット**: `perf(lsp)!: run workspace operations off the actor loop and stream SSE`

### P3-T3 workspace の増減を daemon で処理する（Rust）

- **所有**: `transports/lsp/**`、`application/**`、`contracts/protocol/**`
- **背景**: adapter は `workspace/didChangeWorkspaceFolders` を送っている（`editor-service.mts`）が、**daemon はこの通知を処理していない**（`fd5cd1a` で確認済み）。現在の `fleximark/reconfigureWorkspace` は、既存の root の config を再読込みするだけである。
- **作業**
  1. 標準の `workspace/didChangeWorkspaceFolders` を処理し、root の追加と削除を application の `WorkspaceAuthority` に反映する。追加した root の trust は既定で false とする。
  2. trust の変更を受け付ける。`fleximark/reconfigureWorkspace` の引数に `trusted` を加えるか、専用の method を設けるかは、少ない変更で済むほうを選ぶ。選んだ設計を ARCHITECTURE に書く。
  3. root を削除したときは、そこに属する文書を workspace のない文書（detached）として扱い直す。preview は維持する。
  4. initialize で `workspaceFolders` capability（`changeNotifications`）を広告する。
- **完了条件**: server のテストで、root の追加 → trust の付与 → config の反映 → root の削除までを、再起動なしで検証する。
- **コミット**: `feat(lsp)!: handle workspace folder changes without restart`

### P3-T4 TS の振る舞いテストを先に作る（TS）

- **所有**: `tests/typescript/**`
- **作業**
  1. P3-T5 と P3-T6 で内部構造を作り替える前に、**公開された振る舞いだけを検証するテスト**を追加する。fake daemon（実際の LSP framing で応答する）と fake vscode を使う。
  2. 対象は次のとおり。
     - preview を開く・更新する・閉じる
     - 接続が切れて再接続したときの再同期
     - 古い接続からの遅れた応答の破棄
     - embedded と external の切替え
     - 外部ブラウザからの navigation
     - workspace の追加と削除
  3. 共通の harness（polling、fake daemon の起動、vscode の stub）を `tests/typescript/support/` に一つ作る。既存の 4 つの polling helper（`standalone-runtime.test.mts`、`multi-root-runtime.test.mts`、`browser-dom-harness.mts`、`browser-navigation-runtime.test.mts`）をこれに置き換える。
  4. 当面の注意（P5-T7 で改善するまで）: TS のテストを追加するときは、`tsconfig.unit.json` か `tsconfig.electron.json` の include、aggregator（`pure.test.mts` か `extension.test.mts`）、`tooling/tasks/build.py` の 3 か所を更新する必要がある。
- **完了条件**: 新しいテストが、現在の実装に対して成功する。
- **コミット**: `test(vscode): add behavior-level runtime tests`

### P3-T5 接続の世代と preview の状態モデル（TS）

- **所有**: `adapters/vscode/src/**`、`tests/typescript/adapter/**`
- **作業**
  1. `DaemonConnection` を導入する。
     - rpc、generation、`daemonInstanceId` をまとめた、破棄できるオブジェクトにする。
     - `AbortSignal` を持ち、破棄されると、それに紐づく非同期の完了がすべて無効になる。
  2. `PreviewController` を導入する。
     - `EmbeddedPreview` と `ExternalPreview` の判別 union にし、binding を交換する唯一の API を持つ。
     - `disposePreview` の呼出しは一か所にする（現在は 3 か所）。
     - 現在の `PreviewState` / `PreviewBinding` が持つ 10 個の可変フィールドは、状態ごとの型に分ける。
  3. 古い非同期結果を捨てる 8 種類の判定を、`binding.isCurrent()`（と `connection.signal`）の一つに置き換える。
     - 対象: `sameDaemonOrigin`、`previewOwnsIncarnation`、`previewCandidateIsCurrent`、`previewCandidateIdentityIsCurrent`、`previewIsCurrent`、`#previewIsActive`、`#originIsCurrent`、`syncDocument` に渡す closure
  4. `binding.renderRevision` の意味を統一する。「最後に適用・確認した revision」とし、embedded と external で同じ意味にする。
  5. external preview では、frame の revision だけを読み、frame 本体を保持しない（D8）。
- **完了条件**
  - 上記 8 種類の判定のシンボル名が `git grep` でヒットしない。
  - P3-T4 のテストが成功する。
- **コミット**: `refactor(vscode)!: model daemon connections and previews explicitly`

### P3-T6 `adapter.mts` を分割し、daemon の再起動を廃止する（TS、P3-T3 と P3-T5 の後）

- **所有**: `adapters/vscode/src/**`、`tests/typescript/**`
- **作業**
  1. `FlexiMarkAdapter`（`adapter.mts`、1,012 行）を次の 3 つに分ける。目安として `adapter.mts` を 300 行以下にする。
     - `ConnectionRouter`: server メッセージの振り分け。`as` によるキャストをやめ、生成された validator で型を絞る。
     - `CommandService`: コマンドの実行。
     - `EditorEventForwarder`: selection、viewport、文書イベントの送信。
  2. テスト用の入口（`previewNavigationForTest`、`openExternalPreviewForTest`、`crashDaemonForTest`、`recoveryStateForTest`）と `#testOrigin` を本番クラスから外す。必要なものは、`ExtensionMode.Test` のときだけ公開する extension の test API に移す。テスト向けの re-export も削除する。
  3. workspace の構成が変わったときの daemon の再起動を廃止する。
     - `start()` が runtime の key 集合の変化を見て `workspaceChanged()` を呼び、process を kill している（`adapter.mts`、`daemon-supervisor.mts`）。これをやめる。
     - folder の追加・削除は `workspace/didChangeWorkspaceFolders` と trust の通知（P3-T3）で伝える。
     - 再起動は crash のときだけにする。
     - `workspaceRevision` と、空処理になっている `synchronizeEditorWorkspaces` を削除する。
     - `multi-root-runtime.test.mts` の、`daemonInstanceId` が変わることを assert している期待値を、「変わらない」に改める。
  4. spawn の cwd を「最初の runtime の workspace」という恣意的な値から、workspace に依存しない固定の場所（例: runtime root）に変える。
  5. `logLevel` の `error` と `debug` を実際に効かせる（現在は `!== "off"` の判定だけ）。
  6. 意味のない抽象を整理する。
     - `AsyncTaskObserver`（catch するだけ）
     - `PreviewHost.dispose` を 2 回呼ぶと enhancer をもう一度 dispose する問題
     - 未使用の export（`isPosition`、`isSourcePosition`、`isSourceRange`、`isPreviewChangedParams`、`isSourceNavigationEvent`、`ClientPreviewEventParams`、旧 `InitializeParams`、`TextSelection`、`sameStyle`、テストでしか使っていない `runtimeTarget` と `DaemonSupervisor.snapshot()`）
- **完了条件**
  - workspace 外の Markdown を開いても、`daemonInstanceId` が変わらない（テストで確認する）。
  - `git grep -n 'ForTest\|#testOrigin' -- adapters` がヒットしない。
  - P3-T4 のテストと editor テストが成功する。
- **コミット**: `refactor(vscode)!: split adapter and stop restarting the daemon on workspace changes`（分割してよい）

### P3-T7 preview の共通 session（web、P3-T5 と並列に進めてよい）

- **所有**: `web/preview/**`、`adapters/vscode/webview/**`、`tests/typescript/*host*.test.mts`、`preview-client.test.mts`
- **作業**
  1. 「frame の適用と、保留している navigation の順序制御」と「失敗時の回復」を、`web/preview` の共通 `PreviewSession` にまとめる。
     - 現在は、adapter（`preview-coordinator.mts`）と `browser-host.mts` に別々の実装がある。
     - 回復の方針も違う（`vscode-host.mts` は 1 回だけ回復を試み、`browser-host.mts` は即座に失敗する）。
     - 共通化に合わせて、方針を一つに決める。
  2. host 側（`vscode-host.mts`、`browser-host.mts`）は transport の役割だけにする。
  3. frame の検証を境界で 1 回にする。
     - 現在は、adapter の RPC 受信 → bridge → `PreviewHost.apply` → `PreviewDocument.apply` で 3〜4 回検証している。
     - 境界（webview bridge、または browser の SSE 受信）で検証したら、以降は型付きの値として信頼する。
- **完了条件**
  - 保留中の navigation の処理が一つの実装になっている。
  - preview-client と host のテストが成功する。
- **コミット**: `refactor(preview): share frame sequencing between hosts`

### P3-T8 フェーズ 3 のレビュー

- レビュアーを起動する。
- §3.2 の F1〜F5、F13、F14 を、editor テストか手動 smoke で確認する。

---

## 10. フェーズ 4: 知識の正本の一本化

**目的**: 構文機能、描画ポリシー、IR の走査、protocol registry、配布仕様の正本を一か所にする。

**並列化**: core 系（P4-T1〜P4-T5）、protocol と codegen（P4-T6）、web（P4-T7）、配布（P4-T8）は所有範囲が分かれるので、並列に進めてよい。ただし P4-T1 と P4-T7 の kind 定数の受渡しは、P4-T6 の生成の仕組みを使う。そのため P4-T7 の生成部分は P4-T6 の後に行う。

### P4-T1 組込み構文の記述子テーブル

- **所有**: `core/model/**`、`core/parser/**`、`core/language/**`、`core/render-html/**`（kind 名のみ）
- **作業**
  1. 組込み構文の記述子テーブルを `core/model` に置く（例: `core/model/src/syntax.rs`）。parser と language が既に model に依存しているので、ここに置ける。
     - 記述子の項目（例）: 構文の名前、認識の方法（fence の言語名、directive 名、alert 名）、生成する `BlockKind`、描画時の kind 名（`data-fleximark-kind`）、補完の label・snippet・detail
     - 対象の構文: admonition（info、tip、important、warning、danger）、alert、mermaid、abc、math、YouTube
     - **macro を使わず**、`const` の配列で書く。
  2. parser の文字列 match（fence 言語、directive 名）と、language の補完一覧（`core/language/src/lib.rs` の名前一覧、alert 名）を、このテーブルの参照に置き換える。
  3. YouTube の判定（parser と render-html の 2 か所）を一つにする。
  4. **不具合を修正する**: 補完は ```` ```math ```` を提案するが、parser が `math` fence を `CodeBlock` にするため、数式として描画されない。
     - parser で `math` fence を `BlockKind::Math` にする。
     - まず実機で、現在数式として描画されないことを確認する。
     - 修正後のテストを parser と render-html の両方に追加する。
  5. テーブルの全項目について、「parser が認識する」「language が補完する」「render-html が kind を出す」ことを網羅的に検証するテストを追加する。これで追加漏れを検出できるようにする。
- **完了条件**
  - `git grep -n -E '"(mermaid|abc|info|tip|important|warning|danger)"' -- core` が、記述子テーブルとテスト以外でヒットしない。
  - math fence のテストが成功する。
- **コミット**: `refactor(core)!: introduce built-in syntax descriptor table` と `fix(parser): render math fences as display math`

### P4-T2 render-html の分割と、sanitize 経路の一本化

- **所有**: `core/render-html/**`、`core/model/**`（navigation のみ）
- **作業**
  1. raw HTML を「マーカー → ammonia → 差し戻し」で処理する経路が 3 系統ある。これを一つの型（例: `ProtectedFragments`）にまとめ、block、nodes、inline は設定の違いで使い分ける。
     - 3 系統: `render_sanitized_block_sequence`、`render_sanitized_nodes`、`render_inline_sequence`
     - 4 要素タプルを添字でアクセスしている箇所と、`expect("checked above")` を名前付きの struct で置き換える。
     - inline の連続部分を切り出す処理の複写（2 か所）を一つにする。
  2. ファイルを分割する。
     - `sanitize.rs`: 保護断片を扱う共通の処理
     - `resource_policy.rs`: URL を許可するかどうかの判定（`ResourcePolicy`）と asset 参照の検証
     - `block.rs`、`inline.rs`: 描画
  3. navigation の収集と NodeId の重複検査を model に一本化する（現在は model と render-html の二重実装）。
  4. 165 行ある `render_block` を、kind ごとの関数に分ける。
  5. **セキュリティのテストは削除・緩和しない**。分割の前後で、出力の HTML が同じであることを、既存の fixture と追加の fixture で検証する。対象は block、nested、inline の各経路、マーカー衝突、`data-fleximark-*` の剥奪。
- **完了条件**
  - sanitize の実装が一つである。
  - `lib.rs` が 300 行程度以下になる。
  - 既存のテストに加え、同一出力を確かめるテストが成功する。
- **コミット**: `refactor(render-html): unify sanitized fragment handling and split modules`

### P4-T3 IR の走査 API と、共有する newtype

- **所有**: `core/**`、`application/**`（置換のみ）
- **作業**
  1. model に `InlineKind::children()` と、block・inline の visitor（または iterator）を追加する。
  2. 6 か所にある inline の再帰を、これに置き換える。対象は model、engine、render-html、identity、application の `native/assets.rs`（wasm candidate は削除済み）。
  3. asset 参照（`fleximark-asset:` と 64 桁の hex）を `AssetRef` という newtype にし、model か engine の一か所が所有する。
     - 現在、形式の検証が render-html、`engine/assets.rs`、`contracts/protocol` に重複している。
     - protocol は独自の表現を持つが、検証規則は `contracts/protocol` 側でテストにより一致を確かめる。
  4. SHA-256 hex の検査（5 か所）を一つの関数か newtype にする。crate の境界をまたぐ場合は、層ごとに一つずつにし、テストで一致を確かめる。
  5. NodeId の識別キーを、`format!("{:?}")` の Debug 出力ではなく、明示的な正規化（kind ごとに安定したバイト列）から作る（`core/engine/src/identity.rs`）。
     - ハッシュは identity の用途では一種類にする（blake3 か SHA-256 のどちらか）。
     - 変更の前後で、既存の文書の NodeId が維持されることまでは求めない（D2）。
     - 照合のテスト（編集の前後で NodeId が維持されること）は成功させる。
  6. `language::Position` と `model::SourcePosition` の重複を解消する。
  7. `remap_provenance` の名残があれば削除する。`SourceIndex` をノードごとに作り直している箇所（`edit_map.rs` など）が残っていれば、それも直す。
- **完了条件**
  - `git grep -n 'format!("{:?}"' -- core` が識別キーの生成に使われていない。
  - IR の再帰が visitor に集約されている。
- **コミット**: `refactor(model): add IR traversal API and shared value types`

### P4-T4 core/language の境界

- **所有**: `core/language/**`、`transports/lsp/**`
- **作業**
  1. `core/language` の戻り値を、byte offset と内部の enum（補完の種類、token の意味分類）だけにする。
     - serde 付きで LSP の形をした `CompletionItem` をやめる。
     - 補完の中で行・文字位置を計算しているのもやめる。
  2. LSP の数値 kind、snippet format、相対 token の配列への変換は、transport が明示的な対応表で行う。
     - 現在、token 種別の文字列一覧（language）と transport 側の数値対応は、並び順だけで暗黙に結びついている。これを enum から数値への `match` に置き換える。
  3. semantic tokens を fence の状態を考慮して出す。parse の結果を使うか、fence の状態を持つ走査にする。code block の中の `:::` を強調しないことのテストを追加する。
- **完了条件**
  - `core/language` が serde に依存しない（必要がなければ）。
  - fence 内部のテストが成功する。
- **コミット**: `refactor(language)!: keep LSP shapes in the transport`

### P4-T5 parser の整理

- **所有**: `core/parser/**`、`core/model/**`
- **作業**
  1. 到達しない分岐を削除する。footnote、description list、subtext の分岐は、comrak の対応拡張が有効になっていないため到達しない。
  2. Highlight、Superscript、Escaped などを一律に Emphasis にしている処理を改める。意味を保つ kind か Text に明示的に落とす。対応表はコードにコメントで残す。
  3. どれにも当たらないノードを Debug 表示の文字列で代用している処理をやめる。明示的な Text への変換か、エラーにする。
  4. alert の kind と title を `{:?}` から作るのをやめ、comrak の `NodeAlert.title`（独自タイトル）を反映する。テストを追加する。
  5. 不明な directive の扱い（P1-T2 で `BlockKind::Directive` に改名済み）のテストを追加する。
- **完了条件**: parser に `{:?}` を使った文字列の生成がない。新しく追加したテストが成功する。
- **コミット**: `fix(parser): preserve inline semantics and alert titles`

### P4-T6 protocol registry の単一の表と codegen の整理

- **所有**: `contracts/protocol/**`、`tooling/codegen/**`、`contracts/typescript/*.mts`（生成物を除く）
- **作業**
  1. `contracts/protocol/src/registry.rs` の `WireType` は、variant、`ALL`、`wire_name`、`schema_name`、`typescript_name` の 5 か所で多重に列挙されている。これを一つの宣言的な表（`const` 配列の記述子と、それを引く関数）にまとめる。
     - macro は使わない。schemars の schema を得る関数ポインタを記述子に持たせる。
     - codegen 側の import と `match`（`tooling/codegen/src/main.rs`）と `METHOD_SPECS` も、この表を走査する形にする。
  2. 手書きの LSP method の schema（`main.rs` の該当部分）を、実際に配送している method（`codeAction`、`semanticTokens/full`、`executeCommand`、`showDocument`、`logMessage` など）と一致させる。一致を確かめるテストも追加する。利用者がいない部分は削除してよい。
  3. fixture の生成（engine と comrak への依存）を codegen から切り離し、`contracts/protocol` か engine のテスト側（または xtask のコマンド）へ移す。目的は codegen から core への依存をなくすことである。
  4. `check_protocol_contract` が verify の 1 回の実行中に 4 回以上走っている。これを 1 回にする（`tasks.py`、`build.py`）。
  5. TS の契約層を整理する（`contracts/typescript/protocol.mts`）。
     - 生成された型名をそのまま使い、名前が逆転している alias を廃止する（`EditorNavigationEvent`、`PreviewNavigationEvent`）。
     - `Validator<T>` を `(v: unknown) => v is T` の type predicate にし、検証後のキャストを削除する。
     - 検証の方式（`contractValidator` と `semanticContractCheck`）を一つにし、意味の検証は生成された registry に紐づける。
     - 二重に定義されている JSON-RPC の envelope 型（`rpc.mts` と `protocol.mts`）を一つにする。
     - 4 つある `object()` helper を一つにする。
     - 手書きの型の重複（`navigation.mts` の `PreviewNavigationUpdate`、`PreviewNodeEvent`、`providers.mts` の `WirePosition`、`WireRange`、semanticTokens の型一覧の重複、`"embeddedHtml" | "externalBrowser"` のリテラル）を、生成型に置き換える。
- **完了条件**
  - 型を 1 件追加するときの変更箇所が、表への 1 行と型の定義だけになる。統括者がダミーの型で確かめ、確かめ終わったら破棄する。
  - codegen が `fleximark-engine` に依存しない。
- **コミット**: `refactor(protocol): declare wire types in one registry table`

### P4-T7 web/preview の enhancer registry と CSS

- **所有**: `web/preview/**`、`tests/typescript/preview-*.test.mts`
- **作業**
  1. `Enhancer { kind; enhance(el, ctx): Disposable | Promise<Disposable> }` の registry を導入する。
     - `enhance.mts` の `data-fleximark-kind` による switch と、enhancer ごとに違う寿命管理（mermaid は dispose を返し、abc は track/release の callback、tabs は共有 Map）を、この registry に統一する。
     - kind の定数は、P4-T1 のテーブルから P4-T6 の生成の仕組みで TS へ出力する。
  2. ABC 固有のハイライトと payload の再パース（`navigation.mts`）を abc の enhancer へ移す。`shared.mts` の `previewPayload` との重複も解消する。
  3. オーディオの合成器（`runtimes.mts`）を独立した module にする。
  4. `fleximark.css`（817 行の一つのネスト）を機能別に分割する（base、admonition、code、abc、mermaid、math、table など）。
     - hex 色の直書き（71 か所）を custom property にする。
     - 現行の renderer が出力しない legacy class（`div.info` など）は削除する。
     - 削除する前に、render-html の出力に該当する class がないことを grep で確かめる。
     - 配布の対象ファイル（`resources/preview/fleximark.css`）が一つであることは維持し、分割したファイルは build で結合する。
- **完了条件**
  - `enhance.mts` に kind の switch が残っていない。
  - preview の表示テストが成功する。
  - 目視（smoke）で見た目が変わっていない。
- **コミット**: `refactor(preview): add enhancer registry and split stylesheet`

### P4-T8 配布仕様の正本

- **所有**: 統括者（`contracts/distribution/**`、`tooling/tasks/**`、`runtime/preview-assets/**`、`adapters/zed/**`、`adapters/vscode/src/runtime-manifest.mts`、`.github/**`）
- **作業**
  1. `contracts/distribution/distribution.json` を正本として作る。
     - target の表: Rust target triple、VS Code の target 名、OS、arch、実行ファイルの拡張子
     - runtime のファイル一覧: binary、`resources/preview/*`、`resources/workspace/*`
     - サイズの上限
  2. `runtime-manifest.schema.json` の target の enum とファイル一覧は、この正本から生成する。
  3. 小さな Rust crate `contracts/distribution`（crate 名 `fleximark-distribution`。依存は serde、serde_json、sha2 だけ）を作る。内容は次のとおり。
     - manifest の型と検証（許可リスト、包含、サイズ、hash）
     - target の表（`include_str!` した正本の解析）
     - `release.toml` の reader
  4. 正本の利用者を次のように切り替える。
     - `runtime/preview-assets` は検証をこの crate に委ねる。
     - Zed 拡張（独立した workspace）は path 依存でこの crate を使う。wasm32-wasip2 でビルドできることを確かめる。`adapters/zed/src/release.rs` の独自の検証と、`build.rs` の `release.toml` の解析を置き換える。
     - Python（`distribution.py`、`release_artifact.py`、`release_assets.py`）は、正本の JSON を直接読む。`_targets.py` は削除する（P5-T4 と重複するが、ここで行ってよい）。manifest の検証は Python 側で一つの関数にまとめる。
     - TS（`adapters/vscode/src/runtime-manifest.mts`、`.vscode-test.mjs`）は、codegen で生成した定数を使う。TS 側の検証は、起動する binary の hash に加えて、manifest の schema とサイズの上限も確かめる。
     - CI の matrix は、正本から生成する（P5-T2）。
  5. 4 つある `release.toml` の手書き parser を、Rust では上記の crate、Node では一つの helper に置き換える。semantic-release の分は P5-T1 で削除する。
- **完了条件**
  - target の追加が、正本の 1 行と再生成だけで済む。統括者が `--dry-run` で確かめる。
  - `mise run package all` が成功し、archive の内容が変わらない。
- **コミット**: `refactor(distribution)!: single-source targets and runtime manifest`

### P4-T9 フェーズ 4 のレビュー

- レビュアーを起動する。
- §3.2 の F1、F7、F15、F16 を確認する。

---

## 11. フェーズ 5: tooling・CI の圧縮と掃除

**並列化**: P5-T1〜P5-T3 は `.github` と `tasks.py` が重なるので、統括者が順に行う。P5-T7（TS の build）は並列に進めてよい。

### P5-T1 リリース経路の確定（D5）

- **作業**
  1. 次を削除する。
     - `release.config.mjs`、`tooling/tasks/semantic-release-package.mjs`
     - semantic-release の devDependencies（`@semantic-release/*` と `semantic-release`）
     - semantic-release から `distribution.py versions` を呼んでいる経路
  2. `release.yml` は `v*` タグを起点にした現行の draft release の作成を維持する。
  3. `docs/DEVELOPMENT.md` にリリースの手順を書く。手順は次のとおり。
     1. `release.toml` の version を更新する。
     2. `mise run generate versions` を実行する。
     3. CHANGELOG を手で更新する。
     4. commit する。
     5. タグ `vX.Y.Z` を push する。
     6. 生成された draft を確認して公開する。
- **コミット**: `build!: release from manual tags only`

### P5-T2 CI の一本化

- **作業**
  1. `ci.yml` と `release.yml` で同じ内容になっている quality、zed-product、products の 3 job を、`workflow_call` の reusable workflow（例: `.github/workflows/verify.yml`）にまとめる。
  2. matrix は、P4-T8 の正本から setup job が出力した値を `fromJSON` で受け取って作る。
  3. workflow を文字列の正規表現で検査しているテスト（`tooling/tasks/tests/test_release_workflow_contract.py`）と、そのためだけに存在する matrix の `vscode:` キーを削除する。
  4. 未使用の composite action（`.github/actions/smoke-vsix`、`validate-extension`、`verified-release-artifact`）を削除する。
  5. `setup-toolchain/action.yml` の `rustup component/target add` は `tasks.py setup` と重複しているので、一方にまとめる。
  6. Rust の cache を `Swatinem/rust-cache` に置き換える（D10 の例外として許可する。SHA で pin する）。restore-keys がなく `target/` が更新されない問題を解消する。
  7. Windows と macOS でも Rust の unit test（`cargo test --workspace`）を実行する。product job に含めてよい。
- **コミット**: `ci: share verification jobs between CI and release`

### P5-T3 入口の一本化と tasks.py の分割

- **作業**
  1. root `package.json` の scripts（`mise run` へ転送しているだけのもの）を削除し、入口を mise だけにする。`docs/DEVELOPMENT.md` の Yarn wrapper の記述も削除する。
  2. 各モジュールが独自に持つ argparse の CLI（`build.py`、`distribution.py`、`protocol_codegen.py`、`release_artifact.py`、`release_assets.py`、`smoke_vsix.py`）を、`tasks.py` の subcommand に統合する。CI からの直接呼出し（`release.yml` の `release_assets.py`、`release_artifact.py`）も `tasks.py` 経由にする。
  3. `tasks.py` を責務ごとに分割する。例: `tasks/plan.py`、`tasks/js.py`、`tasks/rust.py`、`tasks/distribution.py`、`tasks/release.py`。
     - 関数の中での `__import__` をやめる。
     - Zed の archive 組立て（`distribution.assemble_runtime` の zip と sha256 の処理を複写している）を共通化する。
  4. Plan モデルと `--dry-run` は維持する。
- **完了条件**
  - `mise run help` と各コマンドの `--dry-run` が従来と同等の計画を表示する。
  - Python のテストが成功する。
- **コミット**: `build: consolidate task entry points`

### P5-T4 使われていない物の削除

- 次を削除する。それぞれ、削除する前に `git grep` で未参照であることを確かめる。
  - `vscode:prepublish` 系一式。staging で `scripts` を削除しているため実行されない（`tasks.py` の該当部分、`release_artifact.validate_prebuilt_inputs`、`FLEXIMARK_RELEASE_PREBUILT`、`adapters/vscode/package.json` の `vscode:prepublish`）。
  - `tooling/tasks/_targets.py`（P4-T8 で削除済みなら省略する）
  - `vendor_katex_css.py`。生成物 `web/preview/katex.css` の出所は、DEVELOPMENT.md に注記する。
  - 未参照の JSON: `contracts/capabilities/{clean-break-catalog,feature-inventory,v0.16.14-inventory}.json`、`contracts/schemas/feature-inventory.schema.json`
  - uv、`pyproject.toml`、`uv.lock`。依存がなく、各タスクは素の python で動く。`mise.toml` の uv と `tasks.py` の `uv sync` も削除する。
  - `adapters/vscode/.vscodeignore` の `packaging/**`
  - esbuild の未使用の `--define:__DEV__`（`build.py`）
  - 旧称の残り: `daemon manifest`（`release_artifact.py`）、`test_dev_*`、`test_phase12_contract.py`。phase12 の検査のうち必要なものは、意味のある名前のテストか xtask の検査に移し、残りは削除する。
- **コミット**: `chore: remove dead tooling and unused contracts`

### P5-T5 開発時の watch の効率化

- `tasks.py` の watch が、runtime の全ファイル（native binary を含む）を 0.1 秒ごとに SHA-256 で再ハッシュしている。これを、(mtime, size) が変わったファイルだけを再ハッシュするように改める。
- **コミット**: `perf(tooling): rehash only changed runtime files in watch`

### P5-T6 xtask policy と crate の名前

- **作業**
  1. `dev/xtask/policy/workspace.toml` の `visible_to` を、実際の依存から引いた最小の集合にする。
     - 逆方向の許可（例: application → preview-assets を許す記述）を削除する。
     - 層の方向（core ← application ← transport ← apps）が policy から読めるようにする。
  2. crate の名前をディレクトリに揃える。
     - `apps/server` の crate 名を `fleximark-server` に変える。binary の名前は `[[bin]] name = "fleximarkd"` で維持する。
     - そのほかの不一致（`transports/lsp` = `fleximark-lsp-transport` など）は、ディレクトリから推測できる範囲なら維持してよい。
     - 名前を変えたら、図と docs を再生成・更新する。
- **完了条件**: `mise run check` が成功し、図が更新されている。
- **コミット**: `build: tighten crate visibility policy and align crate names`

### P5-T7 TS の build 構成と境界の強制（並列に進めてよい）

- **所有**: `tsconfig*.json`、`eslint.config.mjs`、`adapters/vscode/package.json`、`web/*/package.json`、`contracts/typescript/package.json`、`tooling/tasks/build.py`（esbuild の部分）
- **作業**
  1. tsconfig を package 単位の project にする。
     - 対象: `contracts/typescript`、`web/preview`、`web/workspace-ui`、`adapters/vscode`（webview を含む）、`tests`
     - `tsconfig.browser.json` が 4 つのディレクトリを一つの project にまとめている現状をやめる。
  2. 相対パスによる package 間の import（`../../../contracts/...`）を、yarn workspaces の package 名による import にする。
  3. eslint の組込みルール `no-restricted-imports` でパターンを指定し、境界を強制する（新しい plugin は導入しない）。
     - `web/preview` は `vscode` と adapter を import しない。
     - `web/*` は互いを import しない。
     - `contracts` はほかを import しない。
  4. eslint の組込みルール `max-lines`（`skipBlankLines`、`skipComments`、max 999）で、TS にも行数上限を適用する。
  5. `tsconfig.unit.json` と `tsconfig.electron.json` の include を glob にする。test の aggregator（`pure.test.mts`、`extension.test.mts`）は glob で発見する方式にし、テストを追加するときに 3 か所を更新しなくて済むようにする。
  6. esbuild の引数が watch 用（`build.py` の `watch_commands`）に手書きで複製されているので、一つの設定にまとめる。
  7. 純粋な TS のテストで Electron 上で実行しているもの（`export-ack`、`workspace-selection`）を unit 側に移す。
- **完了条件**
  - TS の境界に違反する import を書くと lint が失敗する（統括者が試し、確かめ終わったら破棄する）。
  - `mise run check` が成功する。
- **コミット**: `build(ts): split projects per package and enforce boundaries`

---

## 12. フェーズ 6: テストとドキュメント

### P6-T1 テストの再編

- **作業**
  1. `apps/server/tests/service_contract.rs`（`application::native` しか使わない）を application のテストへ移す。`application/src/native/export/tests.rs` と重複している観点（ownership、symlink、journal）はまとめる。
  2. 一時 workspace の fixture builder（`native/test_support.rs`、`service_contract.rs:test_workspace`、apps/cli 内）を一つにする。置き場所は application の test support か、dev 用の crate。
  3. TS の旧テストのうち、P3-T4 の振る舞いテストと重複し、内部の依存 bag に結合しているもの（`preview-coordinator.test.mts`、`multi-root-runtime.test.mts` の巨大シナリオなど）を、振る舞いテストに置き換えるか縮小する。
     - **観点を失わない**ように、削除する前に、観点の対応表を進捗ログに書く。
  4. 配置のずれを直す。redaction のテストが `daemon-supervisor.test.mts` に置かれているので移す。watcher の glob 文字列の完全一致を assert している箇所は、振る舞いの assert に改める。
  5. parser と language の欠けている観点を補う（P4-T1、P4-T4、P4-T5 で追加していないもの）。
- **完了条件**
  - テストの観点の対応表が進捗ログにある。
  - TS のテストの行数が本体の行数を大きく上回っていない（目安として本体の 1.2 倍以下）。
  - `mise run test` が成功する。
- **コミット**: `test: reorganize suites around public behavior`

### P6-T2 ドキュメントの書直し

- **作業**
  1. `docs/ARCHITECTURE.md` と `docs/IMPLEMENTATION.md` を新しい構造に合わせて書き直す。調査 §6 の乖離はすべて解消する。少なくとも次を含める。
     - 所有者と依存方向の表（新 crate `wire-mapping` と `distribution` を含む）
     - 文書更新の 2 段の処理
     - command の記述子と単一の dispatcher
     - preview 状態の所有
     - workspace の増減と trust
     - SSE のストリーミング
     - version の方針（D3: リリースまでは 1 のまま）
     - 配布仕様の正本
  2. 用語を統一し、ARCHITECTURE に用語集の節を設ける。例: 「preview revision」とコード上の `renderRevision`、「daemon identity」と `daemonInstanceId`、workspace / root / folder の使い分け。食い違う名前は、コードかドキュメントのどちらかに揃える。
  3. `docs/DEVELOPMENT.md` に次を反映する。
     - 入口の一本化（P5-T3）
     - リリース手順（P5-T1）
     - TS テストの追加手順（P5-T7）
     - 旧コマンドからの移行表の更新
  4. `README.md` を現状の機能に合わせる。
  5. `tooling/xtask-core` の README は submodule なので変更しない（D11）。配置先の食い違い（README は `vendor/xtask-core`、実際は `tooling/xtask-core`）は、進捗ログに残課題として記録する。
- **コミット**: `docs: rewrite architecture and development guides`

---

## 13. 最終検証と引き渡し

1. `mise run verify` を実行する（GUI が使えない環境では、実行できる工程を全部実行し、残りを進捗ログに明記する）。
2. P0-T1 と同じ方法で再計測し、進捗ログに比較表を書く。
   - 依存 crate 数
   - 言語別の行数
   - テストの所要時間
   - workspace 操作 1 件の追加に要する変更箇所
   - 型 1 件の追加に要する変更箇所
   - target 1 件の追加に要する変更箇所
3. レビュアーに、ブランチ全体の差分（`fd5cd1a..HEAD`）を §3 と §1 の完了の定義に照らして検査させる。
4. 進捗ログの「残課題」に、未完了の項目、判断を保留した事項、利用者の確認が必要な事項をまとめる。
5. `REFACTORING_ASSESSMENT.md` と本指示書は削除しない。最後のコミットで進捗ログの状態を「完了」にする。
6. **push はしない**（D6）。

---

## 付録 A: サブエージェント用プロンプトの雛形

```text
あなたは FlexiMark リポジトリのリファクタリングの実装者である。
リポジトリの root にある REFACTORING_INSTRUCTIONS.md の §2（決定事項）、§3（不変条件）、§4（作業規約）を読み、従うこと。

担当タスク: <タスク ID とタイトル>
タスクの本文: REFACTORING_INSTRUCTIONS.md の該当節を参照すること。

所有範囲（編集してよいパス）:
  <パスの一覧>
編集してはならないもの:
  - 所有範囲の外のすべて。特に共有ファイル（§5.2）と生成物（§4.3）
  - tooling/xtask-core

禁止事項:
  - git commit、push、branch の操作
  - mise run generate と mise run check の実行（統括者が行う）
  - 互換用の shim、fallback、deprecated の alias の追加
  - テストの assert の削除・緩和・skip（観点を移す場合は、移動先を報告すること）

検証: 次の局所的なコマンドで確認すること。
  <例: cargo test -p fleximark-engine / cargo clippy -p fleximark-engine --all-targets -- -D warnings / yarn tsc -p <project> --noEmit>

報告（この形式で返すこと）:
  1. 変更したファイルの一覧
  2. 実装の要点（設計上の判断とその理由）
  3. 実行した検証のコマンドと結果
  4. 共有ファイルや生成物に必要な変更（差分か手順）
  5. 未解決の事項と、統括者への確認事項
  6. 完了条件の各項目の自己判定（満たした / 満たしていない / 統括者の作業待ち）
```

### レビュアー用の雛形

```text
あなたは読取り専用のレビュアーである。ファイルを変更しないこと。
REFACTORING_INSTRUCTIONS.md の §2、§3、§4 と、フェーズ <N> の各タスクの完了条件を読み、
`git diff <フェーズ開始時のコミット>..HEAD` を検査すること。

報告すること:
  - 完了条件を満たしていない項目
  - 不変条件（§3.1）に違反する変更
  - 互換コードや fallback の混入
  - 本番コードの unwrap、expect、panic の追加
  - Role 行の欠落や使い回し、行数上限の超過
  - 旧シンボルの削除漏れ（git grep の結果を添える）
  - テストを弱めた箇所

それぞれに、ファイルと行、重要度（高・中・低）、修正案を付けること。
```

---

## 付録 B: 進捗ログの雛形（`REFACTORING_PROGRESS.md`）

```markdown
# リファクタリング進捗ログ

## 状態
- 現在のフェーズ: P0
- 最終更新: YYYY-MM-DD

## 基準線（P0-T1）
- check: 成功 / 失敗（内容）
- test: 成功 / 失敗（内容）
- 依存 crate 数:
- 言語別の行数:
- test の所要時間:

## タスク
| ID | 状態 | コミット | 備考 |
|---|---|---|---|
| P0-T1 | 完了 | abc1234 | |
| P1-T1 | 進行中 | | |

## 判断の記録
| 日付 | タスク | 判断 | 理由 |
|---|---|---|---|

## 指示書からの逸脱
| タスク | 逸脱の内容 | 理由 | 影響 |
|---|---|---|---|

## 機能チェックリストの結果（フェーズの終わりごと）
| フェーズ | F1 | F2 | … | F16 | 確認方法 |
|---|---|---|---|---|---|

## テストの観点の対応表（P6-T1）
| 旧テスト | 観点 | 新しいテスト |
|---|---|---|

## 残課題
-
```

