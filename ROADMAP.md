# FlexiMark Completion Roadmap

## 現在地

主要な runtime と product flow は実装され、Rust 105件、VS Code 44件、型検査、lint、Clippy、
architecture inventory 検査はローカルで成功している。性能 gate も二つの O(n²) 経路を除去した後、
Windows release build で成功している。cancellation と実利用時の復旧も完了し、残る中心課題は配布である。

## テスト方針

テスト数や網羅率を完成目標にしない。新しいテストは、次のいずれかに該当する場合だけ追加する。

- 実際に発生した不具合の再発を防ぐ
- 公開protocolや永続データの互換性を守る
- security、export、workspace trustなど、失敗時の影響が大きい境界を守る

同じ振る舞いをunit、snapshot、property、E2Eで重複して検査しない。最も安価で原因を特定しやすい
一つの層を選ぶ。OS別テストはOS固有実装に限定し、speculativeなfuzz/property/snapshotは追加しない。
既存テストで十分な場合はテストを増やさず、重複テストは削除してよい。

## M0: CI を信頼できる状態にする

- [x] Windows export の directory flush を修正する
- [x] nondeterministic notification order のテストを修正する
- [x] export acknowledgement test を実際の suite に登録する
- [x] Clippy、ESLint、VSIX source exclusion を修正する
- [x] benchmark に update/render の段階別時間を出す
- [x] performance timeout時に最後の完了段階を出力する
- [x] `verify` に Rust、integration、performanceを含む完全検査を集約する

完了条件: CI の失敗から、失敗した層と再現コマンドを一意に特定できる。

## M1: 大文書更新を実用時間にする

- [x] 同位置 NodeId の差分生成に O(1) fast path を追加する
- [x] source position検証で共有line indexを使い O(n²) 走査を除去する
- [x] Windows release buildで既存performance budgetを通過する

完了条件: CIの基準環境でrelease benchmarkがtimeoutせず、承認済みbudgetを満たす。

## M2: cancellation と復旧を完成させる

- [x] daemon request loop と worker execution を分離する
- [x] `$/cancelRequest` と document-generation cancellation を実装する
- [x] stale generation の snapshot/patch/diagnostic をpublishしない
- [x] retry/backoff/session replay の相関ログを追加する
- [x] 既存のmulti-root integration testを、実daemon crash/replayも確認する形へ拡張する

完了条件: cancel/restart中にも古い結果がUIへ出ず、open bufferとpreviewが復旧する。

## M3: 実利用の仕上げ

- [x] preview、navigation、note、exportを実際のworkspaceで一巡して不具合を修正する
- [x] daemon/adapterのエラー表示と復旧導線を整える
- [x] unsupportedなplugin network capabilityをschemaと表示から除く
- [x] capability inventoryを現行機能の一覧として簡素化する
- [x] 重複している既存テストと、実装を拘束しすぎるテストを整理する

完了条件: 必須user flowを通して使用でき、発見されたblockerが残っていない。

## M4: 配布を完成させる

- [x] 対応CPUをx64/arm64に決定し、Windows/Linux/macOSのartifactを追加する
- [x] VSIX attestationを信頼の基点、内部SHA-256を破損・取り違え検出とする
- [ ] 各配布artifactで一つのclean-install smokeを通す
- [ ] clean-break release noteとunsupported configuration errorを確認する

完了条件: clean environmentでinstall、起動、preview、export、updateを再現できる。

## 後回しにするもの

- `JsonDebug` / `PlainText` renderer
- raw HTML allow/sanitize mode
- plugin network capability
- VS Code以外のeditor adapter

これらは現在の完成を妨げない。必要になった時点で個別に設計する。
