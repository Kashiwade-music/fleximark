# FlexiMark Completion Roadmap

## 現在地

主要な runtime と product flow は実装され、Rust 105件、VS Code 44件、型検査、lint、Clippy、
architecture inventory 検査はローカルで成功している。性能 gate も二つの O(n²) 経路を除去した後、
Windows release build で成功している。残る判断は3 platformでの再現性と、未実装のcancel/E2E証拠である。

## M0: CI を信頼できる状態にする

- [x] Windows export の directory flush を修正する
- [x] nondeterministic notification order のテストを修正する
- [x] export acknowledgement test を実際の suite に登録する
- [x] Clippy、ESLint、VSIX source exclusion を修正する
- [x] benchmark に update/render の段階別時間を出す
- [x] performance timeout時に最後の完了段階を出力する
- [x] `verify:all` に Rust、integration、performanceを含む完全検査を追加する

完了条件: CI の失敗から、失敗した層と再現コマンドを一意に特定できる。

## M1: 大文書更新を実用時間にする

- [x] 同位置 NodeId の差分生成に O(1) fast path を追加する
- [x] source position検証で共有line indexを使い O(n²) 走査を除去する
- [x] Windows release buildで既存performance budgetを通過する
- [ ] Linux/macOS release buildでbudgetを測定する
- [ ] 必要性が実測された場合にblock render cacheを追加する
- [ ] 必要性が実測された場合にNodeId→現在位置 indexを追加する
- [ ] 必要性が実測された場合にregion invalidationと部分parseを追加する
- [ ] full fallback reason と頻度を記録する
- [ ] 1k/10k/100k、先頭/中央/末尾編集、構造変更 fixture を測定する

完了条件: release build の benchmark が全対象 platform で timeout せず、承認済みbudgetを満たす。

## M2: cancellation と復旧を完成させる

- [ ] daemon request loop と worker execution を分離する
- [ ] `$/cancelRequest` と document-generation cancellation を実装する
- [ ] stale generation の snapshot/patch/diagnostic をpublishしない
- [ ] unsaved buffer を含む実daemon crash/restart/replay E2Eを追加する
- [ ] retry/backoff/session replay の相関ログを追加する

完了条件: cancel/restart中にも古い結果がUIへ出ず、open bufferとpreviewが復旧する。

## M3: 証拠と安全境界を完成させる

- [ ] syntax別 parser/IR/render snapshotを追加する
- [ ] Unicode source mapping と patch equivalence のproperty testを追加する
- [ ] NodeId random edit sequence testを追加する
- [ ] browser token/origin/CORS/CSP/path adversarial testを追加する
- [ ] exportをLinux/macOS/Windowsでfailure injectionする
- [ ] capability inventoryに `implemented` / `verified` / `deferred` を導入する
- [ ] 一つのgeneric markerを複数E2E証拠として数えないよう検査を強化する

完了条件: 必須user flowごとに独立した自動証拠があり、security failureがfail closedになる。

## M4: 配布を完成させる

- [ ] 対応CPUを決定し、必要なarm64 artifactを追加する
- [ ] daemon checksum/signature/attestation方針を確定する
- [ ] platform別VSIX clean installとdaemon compatibility testを追加する
- [ ] clean-break release noteとunsupported configuration errorを確認する

完了条件: clean environmentでinstall、起動、preview、export、updateを再現できる。

## 後回しにするもの

- `JsonDebug` / `PlainText` renderer
- raw HTML allow/sanitize mode
- plugin network capability
- VS Code以外のeditor adapter

これらは現在の完成を妨げない。追加時には独立したcontract、threat model、test evidenceを要求する。
