# リファクタリング進捗ログ

## 状態
- 現在のフェーズ: 完了（P0〜P6、最終 verify・レビュー完了）
- 最終更新: 2026-10-29
- 起点: `fd5cd1a`、ブランチ `feat/kashiwade/big-update`

## 基準線（P0-T1）
- setup: 成功（権限付き再実行）。
- check: 成功（権限付き再実行）。
- test: Python・root Rust・Zed・plugin fixture は成功。最後の codegen 契約確認は、並行して着手した P1-T1 の Cargo.toml 変更により `--locked` で停止。変更前の契約確認は check で成功している。未変更の TS を別途ビルドして Node 115 テスト成功。
- test の所要時間: 全体の停止まで 164.78 秒、Node 単独 1.98 秒（ビルドを除く）。完全な一括成功の時間とは区別する。
- 依存 crate 数: 328（`cargo tree -p fleximarkd -p fleximark-cli -e normal,build --prefix none` の重複除外行数）。
- 言語別の非空行数 / ファイル数: Rust 27,180 / 130、TypeScript 14,052 / 75、Python 3,655 / 22、JSON 3,596 / 35、CSS 752 / 3、JS 487 / 8。
- 計測対象: git 管理ファイル。submodule、architecture、assets、lockfile、`.generated.*`、`contracts/schemas` を除く。調査レポートの JSON 集計は schema を含むため差がある。最終計測も同じ条件で行う。
- 通常権限では Git の signal pipe、Zed 図の一時 SVG、Python の一時ディレクトリへのアクセスが拒否された。製品の不具合と区別し、必要な検証は権限付きで再実行する。

## タスク
| ID | 状態 | コミット | 備考 |
|---|---|---|---|
| P0-T1 | 完了 | 67980e9 | 基準線の取得。部分実行の制約は上記に明記 |
| P0-T2 | 完了 | 67980e9 | 起点と設計地図、旧APIの確認 |
| P1-T1 | 完了 | 76bdaa6 | application/transport/configからWASM pluginを切離し |
| P1-T2 | 完了 | 2946e5a | coreのplugin/provenance/annotationsと交換用serdeを撤去 |
| P1-T3 | 完了 | 2946e5a | plugin crate/fixture削除、protocol/config versionを1へ |
| P1-T4 | 完了 | 2946e5a | plugin tooling/CI/依存撤去、Zed WASI維持 |
| P1-T5 | 完了 | 2946e5a | 生成契約・VS Code/browserを新形式へ追従 |
| P1-T6 | 完了 | 2946e5a / 最終統合 | 旧README説明と未参照capabilities撤去 |
| P1-T7 | 完了 | 2946e5a / 本記録 | phase reviewとworkspace/standalone/restricted GUI成功 |
| P2-T1 | 完了 | 2df85ba | 詳細AppError・6分類・原因連鎖・RPC code・秘匿logger |
| P2-T2 | 完了 | d7c2b57 | 11 commandとcontext/outcome、唯一の記述子 |
| P2-T3 | 完了 | d7c2b57 | DocumentStore/WorkspaceAuthority/WorkspaceOps、byte APIと共通trust gate |
| P2-T4 | 完了 | d7c2b57 | standard/custom LSP・HTTPを同じtyped dispatchへ |
| P2-T5 | 完了 | d7c2b57 | CLIを同じAPIへ。benchmarkは開発benchへ |
| P2-T6 | 完了 | d7c2b57 | 生成型と操作、migrationは同一planの確認後適用 |
| P2-T7 | 完了 | d7c2b57 / 本記録 | migration導線・循環修正、全関門とGUI成功 |
| P3-T1 | 完了 | 最終統合 | Arc frame/live authorityの所有統一、共通wire mapping |
| P3-T2 | 完了 | 最終統合 | prepare/worker/complete、継続asset refresh、持続SSE、asset memo |
| P3-T3 | 完了 | 最終統合 | folder追加restricted、削除detach、同daemon/sessionを維持 |
| P3-T4 | 完了 | 最終統合 | production変更前に実framingの公開振る舞い基準線を追加 |
| P3-T5 | 完了 | 最終統合 | DaemonConnectionとembedded/external preview状態、世代による無効化 |
| P3-T6 | 完了 | 最終統合 | adapterサービス分割、folder変更時の再起動廃止、test seam撤去 |
| P3-T7 | 完了 | 最終統合 | 共通PreviewSession、順序制御・一回の回復・冪等dispose |
| P3-T8 | 完了 | 最終統合 / 本記録 | 競合・shutdown・10秒遅延・実browser/GUIを検証 |
| P4-T1 | 完了 | 最終統合 | 組込み構文descriptor、補完・描画・YouTubeの正本 |
| P4-T2 | 完了 | 最終統合 | renderer分割、ProtectedFragmentsでsanitize一本化、HTML golden保持 |
| P4-T3 | 完了 | 最終統合 | IR visitor/walker、AssetRef/Sha256Hex、identity正規化 |
| P4-T4 | 完了 | 最終統合 | languageはbyte範囲/内部enum、LSP位置と数値をtransportへ |
| P4-T5 | 完了 | 最終統合 | math fence・inline意味・alert title保持、Debug fallback撤去 |
| P4-T6 | 完了 | 最終統合 | 単一wire表と型/guard生成、fixtureをxtaskへ、verifyの契約確認は一回 |
| P4-T7 | 完了 | 最終統合 | enhancer registry/Disposable、ABCとaudio分割、CSSを機能別に分割 |
| P4-T8 | 完了 | 2df85ba / d7c2b57 / 最終統合 | Rust/TS/Node/Python/CIの配布正本統一、target追加実験成功 |
| P4-T9 | 完了 | 最終統合 / 本記録 | 循環解消、protocol型追加実験、security/描画/配布を再検証 |
| P5-T1 | 完了 | ac4a8d3 | semantic-release撤去、手動タグのdraft releaseへ一本化 |
| P5-T2 | 完了 | 最終統合 | reusable verify workflow、配布正本から6 host matrixを生成 |
| P5-T3 | 完了 | 最終統合 | tasks.pyへ入口統一、責務別module、共有build/watch factory |
| P5-T4 | 完了 | 最終統合 | 未使用tooling/contracts/設定/専用testを撤去 |
| P5-T5 | 完了 | 最終統合 | 変更ファイルだけwatch hashを再計算、削除も検知 |
| P5-T6 | 完了 | 最終統合 | 通常/dev依存policyをmetadataで一致検査、server crate名整合 |
| P5-T7 | 完了 | 最終統合 | package別TS project・import境界・行数制限、自動test発見とhost所属 |
| P6-T1 | 完了 | 最終統合 | 公開振る舞い/featureごとにtest整理、共通fixture。比率目安の例外は下記 |
| P6-T2 | 完了 | 最終統合 | README/ARCHITECTURE/DEVELOPMENT/IMPLEMENTATIONを実装に合わせて書直し |

「最終統合」は本ログを含む最終コミットを指す。交差するAPI・生成物・build境界を一つの検証可能な単位として確定した。

## 判断の記録
| 日付 | タスク | 判断 | 理由 |
|---|---|---|---|
| 2026-10-29 | 全体 | 完了後に commit / push を行う | 今回の利用者の明示指示を、指示書 D6 と §13 の push 禁止より優先する |
| 2026-10-29 | P1-T1 | 空の executor は application 内部の engine 境界だけに暫定配置する | 外側の host / SDK の撤去と core 縮約をビルド可能な単位に分ける。P1-T2 で削除する |
| 2026-10-29 | P1-T1 | config schema のテンプレートを生成元とし、version を Rust 定数から設定する | 従来の schema は生成元がなく手書きだった。既存の安全なパス・予約名パターンを維持してプラグイン項目を除く |
| 2026-10-29 | P1-T1 | レビュー2件を修正する | schema テンプレート読込みの文字コードを UTF-8 に固定し、実行中のキャンセルを暫定 executor と application の反映境界で拒否する |
| 2026-10-29 | P1-T2 | NodeId 正規化を P4-T3 から前倒しする | model の交換用 serde と Debug 依存を安全に撤去するため。意味フィールドを長さ付きで符号化し、identity の hash を blake3 に統一した |
| 2026-10-29 | P1-T3 | 手書き fixture の正本を contracts/protocol/fixtures に移す | 既存 codegen は renderer fixture 2件だけを生成していた。他5件も同じ generate/check に含め、version と生成物を手で複写しない |
| 2026-10-29 | P1-T7 | serialization expect を Result 伝播へ変更する | core の読取りレビューの軽微な指摘に対応。renderer fingerprint の失敗で panic しない |
| 2026-10-29 | P2-T1 | 公開エラーを詳細 enum と kind() に統合する | ownership/journal/session の具体的な意味と下位 cause を失わず、6分類と一つの RPC mapping を実現する |
| 2026-10-29 | P2-T1 | I/O を原因別に分類し config/parser の cause も保持する | レビュー指摘に対応し、NotFound/Denied/Conflict/Invalid を Internal と区別する。型 downcast で末端まで検証した |
| 2026-10-29 | P4-T8 | 配布正本の Rust/TS 接続を前倒しする | application の担当と所有範囲が重ならず進行できるため。共通検証は旧 Zed 検査を保ち、サイズ上限・Windows reparse と Node junction テストを加えた |
| 2026-10-29 | P5-T1 | semantic-release の専用testを撤去し、配布・versionテストは維持する | 撤去対象だけを試すtestであり、手動タグで使う生成・manifest・archiveの観点はPython側に残る。Yarn依存は302 package減少 |

## 指示書からの逸脱
| タスク | 逸脱の内容 | 理由 | 影響 |
|---|---|---|---|
| 全体 | 完了後に push する | 利用者の明示指示 | 現ブランチを origin に送る。PR とタグは作成しない |
| P0-T1 | 基準線 test の最終工程を別実行した | 完了前に P1-T1 の manifest 編集を始めたため codegen の `--locked` が停止した | 各 suite は成功したが一括実行の成功時間は未取得。以後の関門は編集を止めて一括実行する |
| P1-T2〜P1-T5 | core の型削除に必要な crate 撤去・tooling・TS 追従を一つの検証可能な単位で統合する | plugin SDK/runtime/fixture が削除対象 model と plugin-api に直接依存するため、指定順では途中コミットがビルド不能になる | タスク ID ごとに結果を記録し、全 check/test 成功後だけコミットする |
| P1-T2 | CLI benchmark の byte 数を HTML UTF-8 長に変更 | RenderSnapshot serde を撤去し、CLI に transport 依存や serializer の複写を追加しないため | JSON に byteMetric=renderedHtmlUtf8 を明記。P2-T5 の開発 benchmark 移設時に計測経路を整理する |
| P2-T1 / P4-T8 | 配布検証の初期部分とエラー統合を同じ検証単位でコミットする | strict manifest へ移した server fixture、Cargo lock、生成図が両変更にまたがるため | P4-T8 の未完部分を完了扱いにせず、Python/packaging を別のビルド可能な単位で続ける |

## 機能チェックリストの結果
フェーズ終了時に F1〜F16 の対応テストと実行結果を記録する。

P1-T1 の関門: `mise run generate all`、`mise run check`、`mise run test` 成功。application 53 テスト、Node 115 テストを含む。GUI の editor smoke はフェーズ終了時に実行する。ログは `.build/refactoring/p1-t1-{generate,check,test}-final.log`。

P1-T2〜P1-T6 の関門: `mise run generate all`、`mise run check`、`mise run test` 成功。core と契約の局所47テスト、外側91テスト、Python 74テスト、Node 114テストを含む。依存ツリー行数は 328 → 171（−157）。daemon/CLI の tree に wasmtime/cranelift/ed25519-dalek/wit-bindgen はない。ログは `.build/refactoring/p1-{generate,check,test}.log`。

P1-T7: `mise run test editor` 成功。実配布 runtime と VS Code 1.144.0 を使用して workspace/standalone/restricted host を検証した。F1〜F14 の対応 unit/native/Node/editor テストは成功。F15 は runtime staging を検証済み、全 package 検証は最終関門で行う。F16 は render-html/preview のテスト成功。実ブラウザ DOM acceptance は最終 verify で確認する。ログは `.build/refactoring/p1-editor.log`。

P2-T1 / 配布検証初期部分: `mise run generate all`、`mise run check`、`mise run test` 成功。application 61 テストを含む局所111テスト、Rust配布5テスト、Zed4テスト、Node115テストを確認。Node の実ファイル検証は通常ファイルの受理と Windows junction の拒否を検証した。ログは `.build/refactoring/p2-integrated-{generate,check,test}.log`。

## テストの観点の対応表（P6-T1）
| 旧テスト | 観点 | 新しいテスト |
|---|---|---|
| application/tests/engine_pipeline.rs | source/document/frame/revision の取消時保持 | application/src/preview_tests.rs の cancelled_change_retains_source_document_frame_and_revision と既存 publication テスト |
| application の plugin export テスト | export の取消時既存出力保持、旧registry拒否 | native/export/tests.rs の cancelled_export_preserves_existing_destination、old_export_registry_and_marker_fields_are_rejected |

## 残課題
- 製品機能・必須検証の未完了、保留判断、利用者の確認待ちはない。
- P6-T1のTS test/body比率は目安1.2に対し下記の最終値。独立したsecurity/競合/GUIのassertを維持する判断を記録した。
- 固定 submodule `tooling/xtask-core` の README の配置名は変更しない。実配置 `tooling/xtask-core` と upstream 記述 `vendor/xtask-core` の不一致は所有元の課題として残す。

## P2 統合の記録
- `mise run generate all`、`mise run check`、`mise run test`、`mise run package all`、`mise run test editor` 成功。Node119、Python77テストを含む。runtime ZIP・VSIX・Zed ZIPとworkspace/standalone/restricted GUIを確認。ログ: `.build/refactoring/p2-final-{generate,check,test,package,editor}.log`。
- P2-T2〜T6は型削除が全境界を横断するため原子的に統合した。途中にビルド不能なコミットを作らないための分割変更である。P4-T8のPython接続も同じ検証単位に含めた。
- module循環3件はsourceの値型・位置型・export検証の所有を分けて解消した。policyやsubmoduleの免除は追加していない。
- Zed migration Code Actionは認証済みworkspace UIをplanMigration contextで開く。action→execute→showDocument→HTTP stateを試験した。
- command名は生成WORKSPACE_COMMAND_KINDへ統一した。新操作の主要変更点はprotocol command/記述子、application dispatch、native実装、transport結果変換、CLI引数である。引数のないbrowser操作は記述子から選択される。
- server service_contractの4テストをapplication/native/service_contract_tests.rsへ移動した。native byte APIから外したUTF-16/全文同期の観点はserver/language_session.rsの同名テストへ移し、UTF-8/16/32、emoji/CRLF、incremental拒否を保持した。
- benchmarkは `cargo bench -p fleximark-cli --bench document_pipeline -- --json` とする。all-targetsの既定benchでは軽量parse/render/update smokeを実行する。
- P4-T8のtarget追加実験はP5のCI matrix接続後に成功した（下記）。

## P3〜P5 の実装記録

以下は統合中の判断の記録。現在の完了状態と最終関門は上のタスク表・末尾の最終結果を参照する。

- 独立したcore/toolingを前倒ししP3と並行実装した。大きな公開API変更の途中ではコミットせず、共有checkout全体が関門を通ってから検証可能な単位を確定する。
- P3レビュー: 末尾slash付きroot削除の影響文書検索を正規化し、旧theme/assetsを残さず同previewへ再配信する回帰テストを追加した。
- P3の競合処理: migration/initと本文編集が重なりasset policyが変わる場合、最新source/configのasset解決をContinueとして同じworker laneへ再配送する。元のnative副作用は一度だけ実行し、同じpending replyを最後まで保持する。
- P3 Rust局所: app85テスト（継続処理の追加suiteは別途最終実行）、LSP/server43テスト＋doctest、HTTP10テスト成功。10秒遅延中の補完、1.2秒の実HTTP、終了時待機解除、同root直列化、最新asset復旧を確認。
- P4レビュー: descriptorのrender kindとenum対応の重複を、const fnからの導出へ変更した。model/renderer/HTTPの構造検査による循環はleaf値型と一方向の描画callbackへ分割して修正済み。policyやsubmoduleで免除しない。
- P4の数式不具合は実CLIでCodeBlock出力を確認してから修正した（`.build/refactoring/math-baseline.log`）。parser/renderer双方でmath fenceを検証した。
- 配布targetの追加実験: distribution.jsonに一時targetを一行追加し、schema/Node/TS定数、Python validator、Rust tests、CI matrixへ到達しpackage dry-runも成功。正本と生成物は復旧済（`p4-target-experiment.log`）。
- protocol型の追加実験: DTO定義とwire表一行だけでschema・TS型・validatorが生成され、protocol/codegenテスト成功。型と生成物を復旧済（`p4-protocol-probe.log`）。Windowsの一時ファイルロックは再実行で解消した。
- P4 CSSは現行119 selector/declarationを色変数の解決後に比較して保持。HTTPのshellテストは旧selector3件の存在確認から注入CSS全体の包含確認へ変更し、認証・headers・fontの観点は維持。GUI・実browserと分割前後の外観比較も成功した。
- P5 Pythonは69テスト成功。Plan/dry-runと失敗時停止、runtime/VSIX/identityの観点を新moduleへ移設した。廃止callback専用testを削除、mixed VSIX testの配布安全性は保持。phase12の文字列/count検査は廃止し、suite所属/TS所属/locked metadataの実用的検査をtest_source_boundariesへ移した。
- CIのdraft自動公開を発見したためD5に合わせて停止した。Marketplace機能は公開イベントで同じ配布物を再検証するworkflowへ移した。実際のtag/release/Marketplace操作は行っていない。
- rust-cacheはSwatinem/rust-cache v2.9.2のSHA `6323deb102c322ba6fcbdcafc7e3dddab59af2b6`を使用（[upstream commit](https://github.com/Swatinem/rust-cache/commit/6323deb102c322ba6fcbdcafc7e3dddab59af2b6)）。P5レビューの関数内動的importは通常importへ修正した。

## P6-T1 Rustテスト移動前の観点表
| 移動元 | 保持する観点 | 移動先 |
|---|---|---|
| native/service_contract_tests::initialization_preserves_active_defaults_and_typed_resource_locations | 初期設定・themeの正確な既定値、返却URI | native/workspace のtest module |
| native/service_contract_tests::export_metadata_preserves_marker_registry_and_journal_wire_schema | marker/registry/journalの独立したkeys・型・field順・整形・digest連鎖・全7状態 | native/export/contracts_tests.rs |
| native/service_contract_tests::managed_export_rejects_a_symlinked_entry_without_touching_the_target | Unix symlink先の保持・置換拒否 | native/export/contracts_tests.rs |
| native/service_contract_tests::managed_export_rejects_a_junctioned_entry_without_touching_the_target | Windows junction先の保持・置換拒否 | native/export/contracts_tests.rs |
| native/service_contract_tests::workspace_control_junction_is_rejected_without_touching_its_target | Windows workspace control junction拒否・target不変更 | native/workspace のtest module |

既存export/testsのownershipは変更拒否・世代・ユーザー資源保護、journalは改竄・回復を検証する。上記の独立wire形式の検査とは異なるため、同じ題材であっても削除せず機能単位へ統合する。一時workspace作成の実装だけをdev/test-supportへ共通化する。

## TS振る舞いの観点対応（P3-T4〜T6 / P6-T1）


P3-T4では既存テストを削除せず、公開adapter APIと実LSP framingで検証する基準線を追加した。

| 既存テスト                                          | 維持する観点                                         | 新しいruntime-behaviorの対応ケース                                                    |
| --------------------------------------------------- | ---------------------------------------------------- | ------------------------------------------------------------------------------------- |
| preview-coordinator / document-lifecycle            | 開く、全文更新、閉じる、embedded/external            | opens, pushes unsaved updates, closes and changes preview targets                     |
| daemon-runtime / daemon-supervisor                  | 切断、文書とpreviewの再同期                          | reconnects after a daemon disconnect and replays the latest document and preview      |
| preview-coordinator / document-coordinator          | 古い接続の保留応答と遅延イベントを破棄               | invalidates a delayed response when its connection dies and rejects old daemon events |
| browser-navigation-runtime / adapter-preview-events | embeddedのnavigation送信、externalからsourceへの移動 | forwards authenticated embedded navigation and browser source navigation              |
| multi-root-runtime / standalone-runtime             | workspace追加・削除後も開いた文書とpreviewを維持     | updates workspace membership while keeping the document preview usable                |

既存のDOM保持、認証、asset、navigation範囲、trust、echo、未保存文書、実browser/SSEの検査は元のテストに残す。内部依存bagのテストを縮小するのはP6-T1で観点の対応を確認してから行う。

folder増減の基準線は通知またはinitializeのwire内容でmembershipの反映を確かめる。現在の再起動をテスト要件にはしない。再起動禁止の期待値はP3-T6で追加する。

pollingはsupport/polling.mtsのpollUntil/untilへ統一する。standalone-runtimeとmulti-root-runtimeは置換済み。browser-dom-harnessとbrowser-navigation-runtimeはweb担当へ同じAPIの置換を引き継いだ。

## P3-T5/T6で移した単体テスト

preview-coordinator.test.mtsの18観点はpreview-controller.test.mtsへ移し、実framingを使うPreviewHarnessから公開Controller APIを検査する。旧依存bagや本番test seamには接続しない。

| 旧観点                                                 | 新PreviewControllerテスト                                                       |
| ------------------------------------------------------ | ------------------------------------------------------------------------------- |
| same-chunkでcreate/rebind直後に来るframe               | keeps same-chunk pushed frames while opening and rebinding                      |
| create完了直後の接続断                                 | removes an uncommitted preview when its connection closes after creation        |
| rpc/generation/daemon identityの違い                   | connection identity is immutable and disposal aborts pending work first         |
| stale candidateのversion/session/文書object/membership | rejects stale create results before committing UI                               |
| frameを埋め込まないCSP shell                           | builds a CSP shell without embedding frame data                                 |
| rerender完了時の旧binding                              | rerenders only a current binding and fences stale completion                    |
| push配送とbinding交換後のlate post                     | delivers pushed frames without reading and fences replaced bindings             |
| readyまで最新frame保持と再配送                         | keeps the latest frame until ready and recovers one rejected delivery           |
| frame配送成功後のnavigation                            | forwards matching navigation after successful asynchronous frame delivery       |
| 配送errorの報告                                        | reports delivery failure after the one permitted recovery                       |
| 古いdaemonからのchanged通知                            | ignores changed notifications from a stale daemon identity                      |
| external revisionとbrowser navigation                  | keeps only external revision and validates browser source navigation against it |
| viewport navigation                                    | forwards editor viewport navigation to the embedded preview                     |
| rootからdetached文書への追従                           | moves an embedded preview from its root to detached Markdown                    |
| dead sessionをdisposeせずpanel再接続                   | reattaches a disconnected panel without disposing its dead remote session       |
| replay不能時のpanel保持                                | retains disconnected panels when replay is unavailable                          |
| dispose応答待ち前のmembership削除                      | removes membership before awaiting remote disposal                              |
| disconnected panelを閉じる                             | closes disconnected panels without disposing dead remote sessions               |

追加で、delivery session自体を交換した後のlate completionによるrevision逆行を検査する。source-navigationの非同期identityとechoの既存assertはadapter-preview-eventsに保持し、公開SourceNavigation/EditorEventForwarderへ接続した。redaction corpusはerror-policy.testへ移動した。

supervisorのworkspace変更による再起動期待値をlive daemon保持へ置換し、古いexitの破棄・process終了待ち・停止後の再起動禁止は維持した。multi-root editorテストの削除時daemon identity/generation期待値を不変へ変更した。実fake daemonテストはfolder追加→trust通知→削除→workspace外Markdownを同一daemonで処理する。

テスト用のcrash/snapshot/external preview入口はExtensionMode.Testだけが作るextension-test-apiへ移した。editorテストで使う補助クラスはtests/supportに置き、本番Adapterにはテストメソッドも再exportも残さない。

P4-T6の型metadata変更は、この段階ではentry.schemaを読む最小追従だけを行った。semantic registryとnavigation型名の整理は次のP4-T6作業として分ける。

P6 Rustの移動後は独立wire DTOと全assertを維持した。application90・共通fixture2・CLI7・server全suiteと4crate clippy成功。root workspaceのall-targetsも統合前半で成功し、最終関門でも再実行して成功した。

## P6-T1 TS fixture共通化前の観点表
| 対象 | 共通化する準備 | 維持する観点 |
|---|---|---|
| preview-client / preview-extended | frame/block/entry/specialFrame/inertRuntimes | 全DOM保持・asset・ABC/Mermaid寿命・navigation assert |
| browser-host / vscode-host | 同じframe fixtureのoverride | host認証・origin/source/token・revision・future nav・回復上限 |
| preview-client / browser-host / vscode-host | linkedom global descriptorの一時設定・復旧 | window.parent隠蔽・異origin拒否・認証の各個別assert |
| preview-clientのassetケース | object URLの保存・置換・復元 | URL再利用・commit後revoke・AVIF MIME・巨大asset拒否 |

実GUIやsecurityの独自ケースは削除しない。調査で安全に減らせる見積りは全体350〜500行で、比率の数字のためにassertを削らない。共通supportも行数に含めた最終計測を下記に記載する。

## P6-T1 adapterテスト整理前の対応表
| 対象 | 共通化・置換 | 保持する観点 |
|---|---|---|
| unit/document-coordinator | runtime/connection/documentの反復準備をローカルfixtureへ | Unicode全文同期・reopen競合・RPC拒否/transport失敗の区別・close時panel保持・所属一意 |
| editor/wiring | provider registrar収集とplain/snippet入力のケース化 | selector/trigger・型変換・params・retirement中の成功/失敗 |
| editor/wiring watcher | glob文字列exact検査をevent挙動へ | config/theme create/change/delete・所属root・retired event/遅延失敗の抑止 |
| editor/multi-root-runtime | folder event観測/timeout cleanupのhelper | 実VS Code eventを必須とし、同daemonでmembershipと設定が反映されること |
| unit/protocol-contract | combinator/JSON equalityの重複準備を名前付きケースへ | 全入力と期待値、depth制限、全semantic検査 |

fake runtimeと完全に重なるように見えるケースにもobject保持やrootの独立性があるため、同等の公開観測を置かず削除しない。redaction corpusはunit/error-policyへ移設済みで、process終了待ち・late exit・停止後restart禁止はsupervisor側に保持する。

## 統合製品・外観の確認
- `mise run package all` 成功。runtime ZIP・VSIX・Zed ZIPを新しいworkspace importsと分割CSSで組み立てた（`p5-package-integrated.log`）。
- browserで同じframeを分割前後のCSSに適用して目視した。admonition・数式・code・表・Mermaid・ABCの表示を確認し、同じ1000px viewportで9 top-level blockの幅・高さ・文字色・背景色が完全一致した。一時browser tabs・viewport・daemon/serverは終了した。
- package境界の否定試験はstdinだけで実施し、web previewからvscodeへのimportをno-restricted-importsが拒否した。sourceへの違反は残していない（`p5-boundary-negative.log`）。

## 最終検証・レビュー（P3〜P6 / §13）
- 親が境界・所有範囲・移行順・完了条件を設計してから詳細指示を配布した。実装・レビューの全サブエージェントは利用者指定の **GPT-6.1 Sol low** を使用した。
- `mise run generate all`、`mise run check`、`mise run test`、`mise run package all`、最終 `mise run verify` はすべて成功。verifyは25工程を省略せず、Rust/Python/Node、high/critical audit、release build、runtime/VSIX/Zed ZIP、性能、実Chromium、VS Codeのworkspace/standalone/restricted、identity、clean-install smokeを実行した。
- 最終ログ: `.build/refactoring/final-generate.log`、`final-check.log`、`final-test.log`、`p5-package-integrated.log`、`final-verify.log`。Node単体は136件・17 suite・skipなし。契約codegen checkはverify内で一回だけ実行した。
- readonly reviewerが起点`fd5cd1a`からのブランチ全体と未commit統合差分を不変条件・機能・完了条件に照らして検査した。root末尾slash、descriptor重複、依存循環、draft自動公開等を修正後、最終指摘なし。fixture整合assertと監査修正も追補レビュー済み。
- 最終GUIで残っていた旧external revision期待値を、初期の正のrevision・GET前後の不変・navigation後の整合という公開観測に更新した。Windows watcher fixture削除はfolder除外と安全なpath確認後に限ってfs.rmの短い再試行を追加した。scroll・watcher・認証のassertは保持した。
- さらに実GUIでrevealSourceのAtTopが選択付きshowDocumentに上書きされる二重配送を検出した。sourceNavigationEvents能力を明示し、VS Codeはtyped通知だけ、Zed等は標準showDocumentだけにした。省略/false/true×select/revealの6条件で単一配送とstale/close拒否を回帰確認し、元の厳密なscroll条件を保持した。
- auditは既存間接依存の脆弱性を検出したため、DOMPurify3.4.17、cheerio経由Undici7.30.1、node-gyp経由Undici8.12.2、identity経由MSAL Node7.1.2へ限定resolutionを設定した。severity/ignore条件は変更せず、最終auditは`No audit suggestions`。MSALのmajor更新は削除APIを使わないことを読取り確認し、identityのpublic/confidential両flowを資格情報・ネットワークなしでAPI smoke確認した。ログ: `final-audit.log`、`msal-compatibility.log`。
- 監査根拠: [DOMPurify advisory](https://github.com/advisories/GHSA-cg2g-xh64-v4q7)、[Undici advisory](https://github.com/advisories/GHSA-xjj6-f33q-929c)、[MSAL advisory](https://github.com/advisories/GHSA-xcx6-jx8p-x3h5)、[MSAL変更履歴](https://raw.githubusercontent.com/AzureAD/microsoft-authentication-library-for-js/dev/lib/msal-node/CHANGELOG.md)。
- 固定submoduleは変更していない。`REFACTORING_INSTRUCTIONS.md`と`REFACTORING_ASSESSMENT.md`を保持した。生成図・schema・型・fixture・policyは正本から再生成し、循環や禁止依存を免除していない。
- stage検査で新規goldenのCRLF混在を発見し、入力Markdownと期待HTMLをリポジトリ指定LFへ統一した。codeのliteral CR一個、Mermaid/ABC JSONのescaped CRLF四個だけが入力のLF化に対応する差分で、tag/属性/NodeId/安全なescapeは同一。readonly reviewとrenderer全23テストで厳密なHTML比較を保持することを確認した（`final-renderer-newlines.log`）。

## 最終の機能チェックリスト
全行成功。下表のRust/Nodeは`final-test.log`と`final-verify.log`、GUI/実browser/配布は`final-verify.log`で確認した。

| ID | 確認方法 |
|---|---|
| F1 | preview-client/preview-controller、server preview_session、実editorの本文・DOM保持 |
| F2 | browser-host、preview-http SSE、実Chromiumで同一ページの未保存変更をSSE受信 |
| F3 | browser-navigation-runtimeの実DOM→HTTP→daemon→editor scrollとpreview-controllerのsource移動 |
| F4 | standalone hostのfile/未保存文書・文書切替・crash recovery |
| F5 | multi-root-runtimeの同daemonでroot追加/除外・未保存文書/preview保持 |
| F6 | restricted editor host、applicationの書込み拒否・trust取消・古い完了拒否 |
| F7 | server language_session、wire mapping、Unicode/CRLF/encodingとfence内token抑止 |
| F8 | application native/workspace・theme、command_dispatch、workspace_browser |
| F9 | native/notes、note-options、workspace workflowのカテゴリ/テンプレート/名前検証 |
| F10 | applicationとserverのcollectAdmonitions、typed command/result |
| F11 | native/exportのownership/journal/recovery/取消/改竄/リンク拒否、export-ack |
| F12 | migration crate、CLI、workspace workflowのplan確認→同一planId適用 |
| F13 | Zed adapter、server standard Code Action/executeCommand/showDocument、認証済workspace UI |
| F14 | supervisor/runtime-behavior、実browser/editorでdaemon再起動後の未保存source回復 |
| F15 | 配布manifest/実fs検証、全package、VSIX identity/clean-install smoke、Zed WASM/ZIP |
| F16 | renderer golden/security、preview-extendedのmath/Mermaid/ABC再生寿命等、実Chromium数式描画とCSS外観比較 |

## 最終計測（73c9ac1時点、P0と同じ除外条件）
| 言語 | 基準 非空行 / file | 最終 非空行 / file | 行数差 |
|---|---|---|---|
| Rust | 27,180 / 130 | 30,776 / 155 | +3,596 |
| TypeScript | 14,052 / 75 | 14,998 / 98 | +946 |
| Python | 3,655 / 22 | 3,716 / 31 | +61 |
| JSON | 3,596 / 35 | 4,381 / 44 | +785 |
| CSS | 752 / 3 | 830 / 14 | +78 |
| JS | 487 / 8 | 482 / 6 | -5 |

- daemon/CLIのnormal/build依存の重複除外行数: **328 → 173（−155）**。wasmtime/cranelift/ed25519-dalek/wit-bindgenはない。Zed独立WASIはこの集計の対象外。
- unit全体の成功時間は26.45秒（`final-test-time.json`）。基準は164.78秒で契約確認停止＋Node単独1.98秒だったため、完全成功同士の速度比較とはしない。verifyの性能budgetも成功した。
- TS本体6,496行、tests/support込み8,502行、比率**1.309**。1.2以下という目安には未到達。公開振る舞い・security・競合・GUIの独立assertを残し、比率のための本体水増し・集計移動・テスト削除はしない。fixture共通化は実施したが、watcherの新しい実挙動検査等も加えた。
- 総行数が一律に減ったとは主張しない。typed境界・worker/競合試験・厳格配布検証を追加した。P0と同じ規則のため、contracts/schemas外の新しい配布schemaはJSON集計に含む。
- workspace操作一件は基本5箇所（protocol command/記述子・outcome、application dispatch、native実装、transport結果、CLI引数）。引数なしbrowser操作は記述子から選択でき、専用UIが必要ならform/翻訳を別途加える。
- wire型一件は型定義＋単一wire表一行。schema/TS型/validator/型mapへ伝播することを一時型追加で実証して復旧済み（`p4-protocol-probe.log`）。
- 配布target一件はdistribution.json一行＋再生成。Rust/TS/Node/Python/CI matrix/package dry-runへ伝播することを一時target追加で実証して復旧済み（`p4-target-experiment.log`）。

## 最終の判断・逸脱
- P3〜P6は共有API・生成型・wire mapping・project分割・自動test発見が交差するため、全体関門を通した一つの最終統合コミットにまとめた。指示書の「1タスク1コミットを基本」からの逸脱として明記し、42タスクの結果と前段コミットを上表で対応させた。ビルド不能な途中コミットは作っていない。
- P6-T1の比率は上記の目安超過を記録した。安全性/機能の検証を守る判断についてreadonly reviewで追加懸念はなかった。
- 必須auditを満たすため既存間接依存の修正版を固定した。新たな機能依存は導入していない。
- 利用者の明示指示により、最終commit後に現ブランチをoriginへ通常pushする。PR・タグ・実releaseの作成は行わない。


## PR #5 の CI 追補（2026-10-29）
- 実CIの起点は73c9ac1、run 40876511597。ローカル全verifyの成功とGitHub上の成功を区別する。失敗は公開認証の前段で発生した。
- [quality失敗](https://github.com/Kashiwade-music/fleximark/actions/runs/40876511597/job/123456606143): offline構造解析前の依存取得が不十分で、clean Linux runnerのambient-authority未cacheで停止した。共通setupにroot/Zed両workspaceの`cargo fetch --locked`を追加し、host targetに絞らず全依存を事前取得する。固定xtask submodule、offline解析、lockfile固定は維持する。
- [dependency-review失敗](https://github.com/Kashiwade-music/fleximark/actions/runs/40876511597/job/123456605427): fast-uri3.1.7のmoderate脆弱性3件。ローカルがhigh以上、PR reviewがmoderate以上という閾値差を見落としていた。fast-uri3.1.9へ固定し、ローカルauditもmoderate以上へ厳格化した。全体のmoderate監査で追加検出したqsは6.16.1へ更新した。
- npmのdeprecation通知はadvisoryと異なるため`--no-deprecations`とする。脆弱性のpackage除外、advisory除外、CI閾値の緩和は行わない。
- setupの両lockfile準備順・失敗時停止、quality内でnetwork auditを実行しないこと、明示auditが新閾値で実行されることを検証した。GPT-6.1 Sol lowの実装agentとreadonly reviewerを使用し、追加懸念なし。
- `mise run setup`、`mise run check`、`mise run test`、`mise run verify`全25工程成功。Python76件・Node136件を含む。ログは`.build/refactoring/pr5-{setup,check,test,verify}.log`。実CIの各platformの結果は[PR #5のChecks](https://github.com/Kashiwade-music/fleximark/pull/5/checks)でcommitごとに確認する。
- 249df32の再実行40879206035ではdependency-reviewとCodeQLが成功し、Linuxのoffline構造検査・clippyも通過した。その先のapplication unitでUnix symlinkのremove_dir二件とbare file:///のauthority判定一件が失敗した。symlink cleanupをUnix remove_file/Windows remove_dirに分け、replace_rootsでは最初に正規化した同一URIから重複canonical検査・canonical root・keyを導出するよう修正した。元のout-of-root拒否・nested alias・重複root検査を保持し、invalid URIのmembership拒否も明示した。readonly review追加指摘なし。ローカル全check/test成功（`pr5-unix-{check,test}.log`）。
- 683adf8の実CI40880640488でqualityが成功し、Linux/Windowsのx64/ARM64はいずれも製品smokeまで成功した。Zedを含むuploadは隠しdirectory `.artifacts`がupload-artifact v7既定の除外対象で失敗したため、既存のarchive/identity拡張子filterを維持したままinclude-hidden-filesをtrueにした。macOSはcase-insensitive filesystemをWindowsだけとしたtest期待が失敗したため、fixtureの実canonicalize結果からcase特性を観測してmembershipとfingerprintの両分岐をassertするよう修正した。productionの追加変更なし。readonly review・全check/test成功（`pr5-matrix-{check,test}.log`）。683adf8ではローカルverify全25工程も再成功した（`pr5-unix-verify.log`）。
- 63b0105の実CI40882536474では、stdin EOF直後にdetached writerのframeが途中で切れる既存の終了競合をrpc_id_contractが検出した。出力queueのclose/drainとwriter joinを所有し、正常終了・actor error・panic時も受理済み応答をflushしてから終了するよう修正した。writer故障はactorへ通知しpending HTTPを解放し、実FrameError原因を優先保持する。stdin readerはjoinしない。即stdin closeと元のunsafe ID/code検査を保った128×4KiB IDのburst、close後enqueue拒否・全量drain、writer panic/error・pending解放を検証した。readonly reviewer signoff、全check/test成功（`pr5-stdio-{check,test}.log`）。
