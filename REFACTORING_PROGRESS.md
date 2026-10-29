# リファクタリング進捗ログ

## 状態
- 現在のフェーズ: P0 / P1-T1
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
| P0-T1 | 完了 | | 全 suite の成功を確認。一括コマンドの lockfile 停止は上記に記録 |
| P0-T2 | 完了 | | HEAD は指示書の起点と一致。指摘された旧 API を現コードで確認 |
| P1-T1 | 進行中 | | GPT-6.1 Sol low に所有範囲と暫定 engine 境界を指定 |

## 判断の記録
| 日付 | タスク | 判断 | 理由 |
|---|---|---|---|
| 2026-10-29 | 全体 | 完了後に commit / push を行う | 今回の利用者の明示指示を、指示書 D6 と §13 の push 禁止より優先する |
| 2026-10-29 | P1-T1 | 空の executor は application 内部の engine 境界だけに暫定配置する | 外側の host / SDK の撤去と core 縮約をビルド可能な単位に分ける。P1-T2 で削除する |

## 指示書からの逸脱
| タスク | 逸脱の内容 | 理由 | 影響 |
|---|---|---|---|
| 全体 | 完了後に push する | 利用者の明示指示 | 現ブランチを origin に送る。PR とタグは作成しない |
| P0-T1 | 基準線 test の最終工程を別実行した | 完了前に P1-T1 の manifest 編集を始めたため codegen の `--locked` が停止した | 各 suite は成功したが一括実行の成功時間は未取得。以後の関門は編集を止めて一括実行する |

## 機能チェックリストの結果
フェーズ終了時に F1〜F16 の対応テストと実行結果を記録する。

## テストの観点の対応表（P6-T1）
| 旧テスト | 観点 | 新しいテスト |
|---|---|---|

## 残課題
- 指示書 P1〜P6 の実装・検証・フェーズレビュー。
- 固定 submodule `tooling/xtask-core` の README の配置名は変更しない。実配置 `tooling/xtask-core` と upstream 記述 `vendor/xtask-core` の不一致は所有元の課題として残す。
