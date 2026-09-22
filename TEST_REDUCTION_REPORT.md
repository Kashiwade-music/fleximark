# テストケース削減調査レポート

調査日: 2026-09-22

## 実施結果

第一段階と第二段階を実施し、Windows 上の実行対象を **349件から309件へ削減した**。

| 区分                                 |  実施前 |  実施後 |               削減 |
| ------------------------------------ | ------: | ------: | -----------------: |
| TypeScript / Node（`test/**/*.mts`） |     108 |      91 |                -17 |
| Python + semantic-release Node       |      87 |      77 |                -10 |
| Rust                                 |     154 |     141 |                -13 |
| **合計**                             | **349** | **309** | **-40（約11.5%）** |

この -40 の大半は、入力ケースと assertion を残したままトップレベルの登録単位を統合したものである。固有assertionがあったケースは代表テストへ移植し、薄いラッパーと実質的な重複保証だけを削除した。

ソース上の宣言数は Rust の OS 排他的な2ケースを両方数えるため350件となるが、Windowsで同時に走るのは349件である。`test/protocol-method-map.type-test.mts` はコンパイル時 assertion のみで、実行時ケース数には含めていない。

## 実測ベースライン

- Python: 82件すべて成功、POSIX限定1件を skip、約3.4〜3.9秒。
- Rust: Windows実行対象154件すべて成功、`cargo test --workspace` 約28.2秒。
- Rust の主な所要時間は engine 16件が約11.1秒、plugin-host 25件が約12.7秒。
- plugin-host の timeout テスト2本は単独実行でそれぞれ約11.6秒。通常の並列実行では重なるため wall time の短縮は限定的だが、直列実行時間とCPU消費は削減できる。
- pure Node 74件、VS Code Electron 17件はすべて成功した。

## 第一段階: 実施済み

### TypeScript: 108 → 91（-17）

次の統合は入力と assertion を維持し、ケース名を assertion message に残す。

| 対象                                                         | 変更                                     | 削減 | リスク |
| ------------------------------------------------------------ | ---------------------------------------- | ---: | ------ |
| `extension.test.mts` の設定2件 + `contributions.test.mts`    | package contributions 契約1件へ統合      |   -2 | 低     |
| `adapter/rpc.test.mts` の unknown/invalid inbound 5件        | close/dispatch の表形式1件へ統合         |   -4 | 低     |
| 同ファイルの malformed typed result 2件                      | typed result matrix 1件へ統合            |   -1 | 低〜中 |
| `protocol-contract.test.mts` の directional previewEvent 2件 | shared fixture 消費1件へ統合             |   -1 | 低     |
| `preview-client.test.mts` の invalid frame 2件               | atomic rejection matrix 1件へ統合        |   -1 | 低     |
| `workspace-selection.test.mts` の active/prompt 2件          | workspace selection 1件へ統合            |   -1 | 低     |
| 同ファイルの encoding/bounds 2件                             | conversion boundary 1件へ統合            |   -1 | 低     |
| 同ファイルの editor reuse 2件                                | match/mismatch 1件へ統合                 |   -1 | 低     |
| `export-ack.test.mts` の成功/失敗2件                         | acknowledges iff open succeeds 1件へ統合 |   -1 | 低     |
| `note-options.test.mts` の選択/取消2件                       | selected/cancelled 1件へ統合             |   -1 | 低     |
| `workspace-migration-runtime.test.mts` の正常/異常2件        | result matrix 1件へ統合                  |   -1 | 低     |
| `preview-coordinator.test.mts` の replay unavailable 2件     | disconnected後条件1件へ統合              |   -1 | 低     |

実際に削除する候補:

- `extension.test.mts::does not ship the legacy JavaScript plugin runtime`（-1）
  - `scripts/verify_architecture.py` が source tree を検査する。
  - `test_release_artifact.py` が VSIX 内の `extension/parserPlugin.js` を禁止する。
  - `.vscodeignore` 契約でも除外を確認している。
  - 配布物保証としては VSIX 検査の方が直接的である。

### Python / release / protocol: 87 → 77（-10）

1. `test_semantic_release_package.py::test_node_unit_suite` を削除し、CIから Node テストを直接起動する（-1）。起動ラッパーだけを消し、Node側5件は維持する。
2. semantic-release の package failure / identity failure を `failureAt` matrix 1件へ統合する（-1）。
3. `test_protocol_codegen.py` の generate/check 起動契約2件を mode matrix へ統合する（-1）。
4. 同ファイルの public task引数拒否 / unknown mode拒否を invalid input matrix へ統合する（-1）。
5. `test_protocol_contract.py::test_fixture_exercises_current_optional_field_omission` を削除する（-1）。Rust の shared fixture テストが同じ optional omission と再serializeを具体的に確認済みである。
6. Python schema層の safe-integer 2件を削除する（-2）。primitive、Rust DTO/RpcId、codegen全integer schema、生成済みTS validatorで同境界を検査している。外部公開schemaを独立実装で検査し続けたい場合は、2件のうちJSON-RPC側だけを残す。
7. Python validator の bool/number 自己試験を独立ケースから invalid fixture の1サブケースへ移す（-1）。
8. `test_phase12_contract.py::test_vscode_test_discovery_selects_only_the_electron_artifact` を削除する（-1）。CIが pure と Electron の両成果物を実際にビルド・実行するため、不整合は実行時に検出される。
9. `test_phase12_contract.py::test_package_rules_keep_runtime_assets_and_exclude_development_trees` を削除する（-1）。CIは実VSIXを生成し、runtime/contribution必須パスと開発ファイル禁止を成果物に対して検査している。

補足: orchestration test 3件で `tasks.check_protocol_contract` が mock されず、codegen check を合計3回余分に起動している。ケース数を変えずに mock を追加すると Python suite を約1.4秒短縮できる。

### Rust: 154 → 151（-3）

1. `all_typed_hooks_run_through_the_same_transaction_boundary` を削除する。
   - `optional_failure_preserves_the_previous_plugin_commit_for_every_hook` と `every_hook_preserves_optional_and_required_malformed_failure_boundaries` が全hookのcommit/rollbackをより強く検査している。
2. `wasmtime_runtime_enforces_fuel_and_wall_clock_timeout` を `component_runtime_timeout_does_not_interrupt_the_next_invocation` へ吸収する。
   - 両方が `__spin__` と Timeout を検査し、後者は次invoke成功まで確認する。
   - 前者の時間上限 assertion は後者へ移す。
3. `preview_sse_reports_only_the_latest_change_after_last_event_id` を `preview_sse_orders_retained_events_and_never_replays_acknowledged_sequences` へ吸収する。
   - 前者固有の exact headers、`retry: 250`、body framing assertion を後者へ移してから削除する。

## 第二段階: 実施済み

意味上の入力ケースを維持したまま Rust のトップレベル登録をさらに10件減らし、154→141とした。

- CLI の no-args / unknown-command を failure table へ統合（-1）。
- fleximark-wire の signed / unsigned boundary を1 matrixへ統合（-1）。
- protocol の RpcId accept / reject を1 domain matrixへ統合（-1）。
- protocol framing の success / missing・duplicate length を1 matrixへ統合（-1）。
- protocol-codegen の同一schema生成3件を1件へ統合（-2）。
- render-html の resource sanitization 2件を1 matrixへ統合（-1）。
- model の invalid provenance 2件を1 tableへ統合（-1）。
- plugin-host の memory limit / cancellation を runtime limit matrixへ統合（-1）。
- fleximarkd の HTTP header/body boundary 2件を parser boundary matrixへ統合（-1）。

これらは実行時間の短縮より、重複したsetupと登録単位の保守負担を減らす変更である。サブケース名は assertion message に残した。

## 追加削減候補（中リスク）

- `preview-client.test.mts::host requests one fresh frame when a frame cannot be applied`（-1）
  - atomic frame rejection と vscode-host の invalid frame recovery にほぼ包含される。
  - 薄い `PreviewHost` 委譲層を直接通す保証は失われるため、host message testへassertionを移してから削除する。
- `daemon-runtime.test.mts::rejects a corrupt bundled daemon`（Electron -1）
  - pureな release-manifest test に「形式は正しいが実内容とhashが不一致」を追加して移管できる。
  - adapter start失敗後のstate clearは supervisor側へ移す必要がある。
- Python の bidirectional server fixture schema test（-1）
  - Rust registry と生成済みTS validatorでも検査されるが、JSON Schema成果物を独立validatorで検査する層が失われる。
- Python の optional-present schema test（-1）
  - 同じ5 DTOをRustとTSで検査しているが、上記と同じくSchema成果物層が失われる。
- release workflow の shared-action setup/smoke静的検査（-1）
  - CI側は実行で保証されるが、release workflow自体はPRで走らない。actionlint等の代替がある場合のみ削除する。

## 削除しないもの

- `release_artifact.py` の22件: archive traversal、zip bomb、TOCTOU、identity bindingなど脅威モデルが異なる。
- Rust export の journal / identity / swap / recovery: crash phaseと改竄箇所が異なる。
- stale/out-of-sync の engine、LSP、daemonテスト: core、registry、wire publicationという別境界である。
- browser-host と vscode-host: transportとlifecycleが異なる。
- Mermaid、KaTeX、ABC: 外部runtimeごとの統合保証である。
- daemon subprocessのRpcId、protocol DTO、wire primitiveの境界: end-to-end、DTO、primitiveという別レイヤーである。
- OS別 symlink / junction: 重複ではなく排他的なプラットフォーム契約である。

## 実施手順

1. 第一段階の「統合」を行い、各サブケース名を assertion message に残した。
2. 削除対象の固有 assertion を代表テストへ移した。
3. 第二段階のRustテストをtable/matrixへ統合した。
4. Python、pure Node、Electron、Rust workspace、型検査、lintを実行した。
5. 中リスク候補は変更せず、今後のCI運用後に再評価する。

第一・第二段階の実装後、Python、semantic-release Node、pure Node、VS Code Electron、Rust workspace、TypeScript型検査、ESLintを通過した。
