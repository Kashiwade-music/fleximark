# リファクタリング進捗ログ

## 状態
- 現在のフェーズ: P2-T2（P4-T8 の残りを独立作業として前倒し）
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
| P0-T1 | 完了 | 67980e9 | 全 suite の成功を確認。一括コマンドの lockfile 停止は上記に記録 |
| P0-T2 | 完了 | 67980e9 | HEAD は指示書の起点と一致。指摘された旧 API を現コードで確認 |
| P1-T1 | 完了 | 76bdaa6 | host/SDK/config/unsafe export/診断を撤去。レビュー2件を修正。generate all、check、test の全工程成功 |
| P1-T2 | 完了 | 本コミット | engine API 縮約、単一 range、plugin/provenance/annotations/型状態 export 撤去、CancellationToken を application へ移設 |
| P1-T3 | 完了 | 本コミット | plugin crate/fixture/依存を削除、protocol/config を 1 にリセット、schema と全 fixture を再生成 |
| P1-T4 | 完了 | 本コミット | tooling/CI/policy から plugin fixture 工程を削除、Zed の wasm target は維持 |
| P1-T5 | 完了 | 本コミット | TS の annotations DOM/watcher/trust 文言を追従、旧 field 拒否を検証 |
| P1-T6 | 完了 | 本コミット | README を更新。capabilities の旧 JSON は全件未参照であり P5-T4 で削除する |
| P1-T7 | 完了 | 2946e5a / 本記録 | 最終 phase review 指摘なし。VS Code GUI結合テスト成功（workspace、standalone、restricted） |
| P2-T1 | 完了 | 2df85ba | ApplicationError / ErrorKind、原因チェーン、RPC定数、共通の秘匿ログを統一。レビュー2件修正、全関門成功 |
| P2-T2 | 進行中 | | 型付き command と記述子を設計し、protocol/codegen を統合する |
| P4-T8 | 進行中 | 2df85ba（初期部分） | 配布正本、Rust共通検証、runtime/Zed、生成schema/TS/JS、Node検証まで接続。Python/全package関門は残作業 |
| P5-T1 | 完了 | 本コミット | semantic-release の設定・script・専用test・7直接依存を削除。手動タグの draft release を維持し開発手順を記載 |

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
- 指示書 P1〜P6 の実装・検証・フェーズレビュー。
- 固定 submodule `tooling/xtask-core` の README の配置名は変更しない。実配置 `tooling/xtask-core` と upstream 記述 `vendor/xtask-core` の不一致は所有元の課題として残す。
