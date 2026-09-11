# FlexiMark リファクタリング計画

## 0. 位置づけと調査範囲

この文書は 2026-09-11 時点のリポジトリを読み取り調査した結果と、既存の外部仕様・挙動を維持するための段階的なリファクタリング計画である。Phase 5 まで完了している。Phase 0ではarchitecture文書、characterization test、独立test入口、local/CI gateを追加し、Phase 1ではprotocol/schema/TypeScript runtime boundaryを方向別の型とvalidatorで固定した。Phase 2ではbuild/release toolingのtarget・subprocess・entry point契約を集約し、build済みCI経路の重複buildを除去した。Phase 3ではversion確定後の最終VSIXを一度だけ生成し、その同一hashを6 target clean install、attestation、GitHub Release、Marketplaceへ引き渡すrelease DAGへ変更した。Phase 4ではfleximark_serviceをprivateな責務別moduleへmove-only分割し、crate rootの公開facadeとfilesystem/exportの挙動を契約testで固定した。Phase 5ではdaemonをtransport、cancellation、routing/handler、preview HTTP、telemetryへ分割し、message/HTTP security orderingを明示的なprivate operationへ整理した。

README.md、README_DEV.md、全 Cargo.toml、package.json、pyproject.toml、mise.toml、build/release scripts、GitHub Actions、tests、主要 entry point、Rust/TypeScript の依存、schemas、capability inventory を確認した。

- リポジトリ管理下に有効な AGENTS.md は存在しない。
- 調査時点で README_DEV.md:7-8 が参照していた ARCHITECTURE.md と ROADMAP.md は存在しなかった。Phase 0 で現行architectureを文書化し、欠落したroadmap参照をこの計画への参照に置き換えた。
- DB と DB schema は存在しない。互換性を守る永続形式は workspace config、plugin manifest/WIT、protocol JSON、export marker/registry/journal、release manifest である。
- Rust の通常依存と TypeScript の静的 import に循環依存は見つからない。
- runtime dependencies の abcjs、katex、mermaid は実使用されており、削除対象ではない。

### 調査時のベースライン

以下は成功した。

- cargo test --workspace --all-targets: 106 tests passed
- cargo fmt --all --check
- cargo clippy --workspace --all-targets -- -D warnings
- TypeScript compiler の noEmit check
- ESLint（adapters web test scripts）
- scripts/verify_architecture.py: 56 capabilities verified

TypeScript tests は49件あるが、pure unit testもVS Code Electron suiteへ束ねられている。完全な mise run verify と6 platformでのpackage smokeは今回の読み取り調査では実行していない。

### Phase 0 完了時のベースライン

- cargo test --workspace --all-targets: 113 tests passed
- Node pure RPC/preview tests: 26 tests passed
- VS Code Electron tests: 49 tests passed
- Python unittest discovery: 15 tests passed
- cargo fmt、cargo clippy、TypeScript noEmit、ESLint、architecture 56 capabilities、performance budget、VSIX内容検査を含む `mise run verify` が成功した。
- 全17 custom methodは共有fixtureをRust serdeとJSON Schemaの両方へ通し、params/result/envelope、optional omission、notification/void result、method set driftを固定した。
- ARCHITECTURE.md と REFACTORING_PLAN.md は `.vscodeignore` で明示的に除外し、Phase 0 前の配布内容を維持した。
- Pythonの限定JSON Schema validatorではJSON型を考慮した `const` / `enum` 比較が必要だった。Python固有の `True == 1` を受理しないnegative testを保持する。

### Phase 1 完了時のベースライン

- cargo test --workspace --all-targets: 115 tests passed
- Node pure RPC/preview/protocol tests: 54 tests passed
- VS Code Electron tests: 75 tests passed
- Python unittest discovery: 18 tests passed
- cargo fmt、cargo clippy、TypeScript noEmit、ESLint、architecture 56 capabilities、production browser/extension build、performance budget、VSIX内容検査を含む `mise run verify` が成功した。
- 全13 custom requestのparams/resultと全4 custom notificationを共有fixtureからTypeScript runtime validatorへ通し、Rust serde、JSON Schema、TypeScriptのmethod set、方向、optional-present DTOを同じ通常gateで固定した。
- malformed envelope/resultはconnectionを終端して既存daemon recoveryへ接続し、未知・late messageはignore、invalid publicationはatomicに拒否してsnapshot/reloadを一度だけ要求する方針を回帰testで固定した。
- schemaの整数はRustの `u64` 全域を表現できる一方、JavaScript runtime validatorは精度を守るためsafe integerを要求する。protocol v1の受理範囲を狭める変更は行わず、将来大きなversion/revision値を必要とする場合はversioned contractとしてmaximumまたはstring表現を決める。

### Phase 2 完了時のベースライン

- cargo test --workspace --all-targets: 115 tests passed
- Node pure RPC/preview/protocol tests: 54 tests passed
- VS Code Electron tests: 75 tests passed
- Python unittest discovery: 40 tests discovered、WindowsではPOSIX executable mode test 1件をskip
- cargo fmt、cargo clippy、TypeScript noEmit、ESLint、architecture 56 capabilities、production browser/extension build、performance budget、VSIX内容検査を含む `mise run verify` が成功した。
- 6 target、platform/arch正規化、実行ファイル名、manifest pathの正本を `scripts/_targets.py` へ集約し、Python behavioral testsとrelease contract testで固定した。
- GitHub Actionsのartifact転送はUnix executable modeを保存しないため、manifest生成時に非Windows daemonを0755へ正規化してからSHA-256を計算する。Windows artifactにはchmodしない契約をtestで固定した。
- `mise run test` 単独のfull buildは維持し、明示的にbuild済みのCI/release検証だけ `mise run test -- --prebuilt` を使用する。最終VSIXのversion/hash/publish identityはPhase 3へ持ち越した。

### Phase 3 完了時のベースライン

- cargo test --workspace --all-targets: 115 tests passed
- Node pure RPC/preview/protocol tests: 54 tests passed
- VS Code Electron tests: 75 tests passed。multi-root testはworkspace folder削除イベントと状態反映を逐次待ち、反復実行で前回のrootを残さない。
- Python unittest discovery: 71 tests discovered、WindowsではPOSIX executable mode test 1件をskip。semantic-release package hookのNode tests 5件もPython gateから実行する。
- cargo fmt、cargo clippy、TypeScript noEmit、ESLint、architecture 56 capabilities、production browser/extension build、performance budget、VSIX内容検査を含む `mise run verify` が成功した。
- 最終versionの `fleximark.vsix` はcustom semantic-release prepareで一度だけ生成し、厳格なSemVer、VSIX metadata、manifest、6 daemon hash、ZIP path/type/sizeを検証してidentity sidecarへSHA-256、tag、source commitを記録する。
- GitHub Releaseはdraftのまま6 target clean installとattestationを待ち、その後に公開する。Marketplaceは公開済みGitHub Releaseと同じActions artifact/hashだけをOIDCでpublishする。
- 完全なdraftまたは公開済みreleaseからの再実行はartifactを再検証して自動復旧する。release commitだけがpushされtagがない状態、またはtag/draft/assetsが不完全・不一致な状態は推測して補修せずfail closedとし、手動復旧を要求する。
- 独立レビューを4巡し、既知の手動復旧境界を除いて未解決P0〜P3がないことを確認した。

### Phase 4 完了時のベースライン

- cargo test --workspace --all-targets: Windowsで120 tests passed。fleximarkd packageはservice unit 25件、daemon 17件、service contract 5件の合計47件が成功した。
- Node pure RPC/preview/protocol tests: 54 tests passed、VS Code Electron tests: 75 tests passed、Python unittest discovery: 71 tests discovered（WindowsではPOSIX executable mode test 1件をskip）。
- cargo fmt、cargo clippy、TypeScript noEmit、ESLint、architecture 56 capabilities、production browser/extension build、performance budget、VSIX内容検査を含む `mise run verify` が成功した。
- `fleximarkd/src/lib.rs` は3,666行から24行のprivate module宣言と明示的re-exportだけのfacadeになった。workspace、notes、theme、assets、plugins、uri、errorと、exportのmodel/journal/filesystem/recoveryを責務別のownerへ移動した。
- crate rootの既存20 functionsと3 typesの公開path/signature、config/default、marker/registry/journalの正確なserialization bytesとdigest chainをservice contract testで固定した。
- Unixのmanaged-entry symlinkとWindowsのcontrol/destination junctionをplatform固有testでfail closedに固定した。ローカルWindows gateに加え、Unix側のcfgと実装同一性を独立レビューし、3 OS CIで各platform testを実行する。
- 関数body、serde shape、export checkpoint、fsync/rename/recovery順序を移動前後で比較した。複数巡の独立レビュー後に未解決P0〜P3はない。

### Phase 5 完了時のベースライン

- cargo test --workspace --all-targets: 135 tests passed。fleximarkd packageはservice unit 25件、daemon 32件、service contract 5件の合計62件が成功した。
- Node pure RPC/preview/protocol tests: 54 tests passed、VS Code Electron tests: 75 tests passed、Python unittest discovery: 71 tests discovered（WindowsではPOSIX executable mode test 1件をskip）。
- cargo fmt、cargo clippy、TypeScript noEmit、ESLint、architecture 56 capabilities、production browser/extension build、performance budget、VSIX内容検査を含む `mise run verify` が成功した。
- `fleximarkd/src/main.rs` は3,770行から81行のCLI/composition facadeになり、transport/stdio、cancellation、telemetry、preview HTTPと、serverのrouting/LSP/RPC/commands/diagnosticsをprivate moduleへ分離した。
- command分類と6 handler、request deserialization/wire error、document changeのedit/assets/publication、HTTPのparse/authority/response write、stdioの1-message処理をnamed private operationへ抽出した。旧処理へ展開した順序、wire bytes、state mutationを独立レビューで照合した。
- mode別routing、全workspace commandのsuccess/trust、exact deserialize error、response/notification順、実LSP obsolete publication抑止、HTTP header/CSP/status、duplicate/size/body境界、SSE replay、navigation revisionをcharacterization testで固定した。4巡の実装・複数独立レビュー後に未解決の新規P0〜P3はない。
- 既存HTTP parserは、最初の `Content-Length` が解析不能で後続値が正しい場合に後続値を採用する。loopback・single-request・`Connection: close` に限定される既存P3だが、修正は挙動変更になるためPhase 5では行わず、現行境界をtest化した。Phase 12後の独立security fixではseen flagを値のparse結果から分離し、両header順を拒否する。
- 2秒slowloris deadline、4 worker/16 queue飽和、Unix/macOSのsocket shutdown/timingはローカルWindowsで負荷再現していない。該当production bodyは移動前と同一であり、platform CIと将来のdeterministic load harnessへ委ねる。

## 1. 現在のアーキテクチャ概要

### 1.1 実行時の流れ

1. package.json:52-56 のactivation eventから、adapters/vscode/src/extension.mtsをesbuildしたdist/extension.cjsが起動する。
2. extension.mtsがproviders、commands、workspace/editor eventsを登録し、adapter.mtsのFlexiMarkAdapterへ委譲する。
3. adapterはfleximarkd lspをchild processとして起動し、rpc.mtsのContent-Length framingでLSPとfleximark/* JSON-RPCを送受信する。
4. crates/fleximarkd/src/main.rsがrouting、cancellation、LSP feature、workspace command、preview HTTP/SSEを統合し、fleximark-lsp::SessionRegistryがauthoritative document stateを保持する。
5. engineはplugin preprocess、parse、provenance remap、block/document transform、validation、render-model extension、HTML render、full/patch publicationの順に処理する。
6. web/preview-clientはpublicationを検証してatomicにDOMへ適用し、Mermaid、ABC、KaTeX、YouTube、tabs等をenhanceする。VS Code webviewと外部browserはcore clientを共用する。
7. crates/fleximarkd/src/lib.rsのfleximark_serviceがworkspace command、asset、exportを担当する。exportはportable composition、明示的unsafe hook、ownership/journal付きfilesystem transactionの順である。
8. fleximark-cliはtransportを経由せず、engineとfleximark_serviceを直接利用する。

### 1.2 Rustの依存方向

~~~text
fleximark-model
├─ fleximark-parser
├─ fleximark-render-html
├─ fleximark-plugin-sdk
│  └─ fleximark-plugin-host
│     └─ fleximark-engine
│        └─ fleximark-lsp
├─ fleximark-protocol ─────────┘
└─ fleximarkd（service lib + daemon bin）
   └─ fleximark-cliはservice libを再利用
~~~

modelはIR/provenance/navigation、parserはComrak AST変換、rendererは安全なHTML、plugin-sdk/hostはversioned WITとsandbox、engineはdocument/render state、protocolはwire DTO/framing、lspはsession registryとedit/navigationを担当する。fleximarkdがcomposition rootである。

### 1.3 Build、test、release

- mise.tomlが利用者向け入口、scripts/tasks.pyがPython orchestrationである。
- build順はscripts/tasks.py:24-29のbrowser bundle → release daemon → platform staging → release manifest → extension/preview bundle。
- capability ownerはcapabilities/feature-inventory.jsonに宣言され、scripts/verify_architecture.pyが一部を検査する。
- releaseは6 target daemon、universal VSIX、6環境clean install、semantic-release、GitHub attestation、Marketplace OIDC publishingで構成される。

## 2. 発見した主要な問題

優先度は互換性・security・release correctnessへの影響、ROIは変更量に対する事故防止と保守性改善で判断した。

### P0 / ROI高

1. 公開する最終VSIXが6環境でsmokeしたartifactと同一ではない。
   - release.yml:144-152,181-192はpre-version candidateを検証する。
   - semantic-release prepare中にscripts/semantic-release-package.mjsがversion更新後のVSIXを再生成する。
   - release.yml:255-278,302-308は最終物をzip検査・attest・publishするだけで、clean installとdaemon起動を確認しない。

2. Protocolの正本が分散し、構造的driftを検出できない。
   - Rust DTO: crates/fleximark-protocol/src/lib.rs。
   - TS DTO: adapters/vscode/src/protocol.mts:11-86、web/preview-client/index.mts:1-104。
   - schema: schemas/protocol.schema.json。
   - verify_architecture.py:105-142は定義名とmethod文字列中心で、required/optional、enum、camelCase、result対応の同値性を確認しない。

3. 外部入力をtype assertionだけで信頼している。
   - rpc.mts:63-78のrequest<T>はparse結果を任意Tとして返す。
   - adapter.mts:1022-1082はdaemon通知をas assertionで扱う。
   - browser-host.mts:45-48もSSE JSONを無検証でassertする。
   - version不一致、破損daemon、malformed eventが未制御例外や不正URI処理につながり得る。

### P1 / ROI高

4. daemonとserviceが巨大な責務集合になっている。
   - fleximarkd/src/main.rsは3,655行、同lib.rsは3,644行。
   - Server（main.rs:465-2093）がconnection、LSP、custom RPC、trust、command、preview、diagnostics、cancellationを担当する。
   - execute_commandは182行（:1752-1933）、serve_preview_requestは268行（:2421-2688）。
   - service側のexport_html_transactionは262行（lib.rs:772-1033）、recover_exportは205行（:1378-1582）。

5. engineのauthoritative pipelineが重複する。
   - plugin付き初回open（engine/src/lib.rs:524-557）と更新（:1025-1058）にpreprocess → parse → remap → reconcile → validate → transform → validateが重複する。
   - full/patch renderも:787-843と:862-897でfingerprint、block render、revision/cache構築が重複する。
   - 一方だけに変更が入るdriftはNodeId、navigation、preview revisionを壊し得る。

6. VS Code adapterがGod object化している。
   - adapter.mtsは1,290行の単一classでworkspace、daemon recovery、manifest、document sync、preview、commands、diagnostics、navigation、loggingを所有する。
   - openPreview :277-396、launch :782-857、message dispatch :1012-1106、recovery :1195-1263。
   - process、clock、RPCを差し替えられずstate machineのunit testが困難。

7. error taxonomyが層を越えると失われる。
   - EngineErrorの多くはlsp/src/lib.rs:942-948でSessionError::Engine(String)になる。
   - fleximarkd/src/main.rs:2705-2717では多くが-32602に潰れる。
   - workspace commandはmain.rs:1752-1932でto_string化され、全て-32020になる。
   - 外部code/messageを変えず、内部型とwire mapperを分ける必要がある。

8. plugin hostが複数責務と反復を持つ。
   - plugin-host/src/lib.rsは2,550行にWasmtime、署名/hash、hook transaction、edit-map、candidate validationを同居させる。
   - trust gate、response variant、required abort/optional diagnosticが:596-915の5 hookで反復する。
   - hook固有transactionまでgeneric化すると逆に不明瞭になる。

9. 重要なstate machineの局所testが不足する。
   - adapter unitはhelper中心で、launch失敗、backoff上限、workspace revision race、shutdown、debounce、diagnostics、preview disposalのdeterministic unit testがない。
   - multi-root-runtime.test.mts:11-167は固定sleep/pollを含む単一巨大integration test。
   - Python build/release scriptsには実動作unit testがなく、test/release.test.mtsは主にsource文字列を検査する。

### P1 / ROI中

10. Previewのsecurity transactionと機能処理が大きな単位に集約される。
    - web/preview-client/index.mtsは590行、applyPatch :402-572はprecondition、clone transaction、asset、navigation、highlight、commitを一括処理する。
    - host.mts:52-68のguardはnested fieldsを検証せず、enhance.mtsのasync rejectionも複数箇所で未回収。
    - atomic clone-then-commit自体は重要なので維持する。

11. service/daemon/package境界が弱い。
    - fleximarkd packageがfleximark_service libraryとdaemon binaryを同居させる。
    - serviceはfleximark-protocolのCommandResult等を直接返す（fleximarkd/src/lib.rs:15,58-184）。
    - CLIもservice再利用のためdaemon packageへ依存する。新crate化ではなく既存package内module分割を先行すべきである。

12. LSP registryに旧単一workspace経路とmulti-root経路が併存する。
    - lsp/src/lib.rs:120-129がplugin_host/render_configとworkspace_configsの両方を保持する。
    - configure_plugins :186-199はrepo内production call siteがなく、daemonはconfigure_workspacesを使う。
    - URI↔sessionの二重mapが手動同期され、複数のexpect("session index is consistent")に依存する。

### P2

13. Release/platform/build定義が重複する。
    - OS/arch判定はstage_daemon.py:10-26とsmoke_vsix.py:20-36に重複する。
    - 同じ6 targetはcreate_release_manifest.py:10-17、CI/release matrix、release.test.mts:112-121に重複する。
    - CI/releaseは明示build後にmise run test内で再びfull buildする。

14. Error/log/subprocess方針が不統一である。
    - Python scriptsはtraceback、error:正規化、独自subprocess wrapperが混在する。
    - adapterのredaction regexはadapter.mts:800-809と:1269-1279に重複し、reload/disposeでは別方針である。
    - user-visible error textやredaction結果を変える場合は互換リファクタとは別承認が必要。

15. 小さいが実在する重複がある。
    - YouTube判定: parser :257-262 / renderer :441-446。
    - SHA-256 lowercase検証: plugin-sdk :453-457 / plugin-host :339-344 / engine inline。
    - CLI export/ack destination: fleximark-cli/src/main.rs:201-214,257-270。
    - adapter daemon reset: :742-747,772-777,1201-1207。

16. Docs、MSRV、依存衛生に穴がある。
    - ARCHITECTURE.md/ROADMAP.mdが欠落する。
    - README_DEV.md:299-302のl10n説明とl10n_export.py:16-29のtemp rename実装が一致しない。
    - Cargo.tomlはRust 1.85を宣言するがmise/CIは1.95しか検証しない。
    - fleximark-cli/Cargo.tomlのfleximark-parserはsourceから直接参照がなく、不要direct dependency候補。

### P3 / dead code候補

- DocumentState.version（adapter.mts:35,414,432,444）、WorkspaceRuntime.removed（:57,234,668）、PreviewState.url（:45,300,334,990）は代入後に読まれない。
- #checkpointのruntime引数も未使用である。
- SessionRegistry::configure_plugins等のpublic symbolはrepo内参照がなくても外部利用が不明なので削除しない。
- capabilities/v0.16.14-inventory.jsonはrepo内参照がないが、監査履歴の可能性があるため確認前に削除しない。

### 問題ではないと判断した点

- 循環依存は確認されなかった。
- PreparedExport / ResolvedExportはsafe composition後だけunsafe hookを適用できる順序制約であり、不要抽象化ではない。
- PreviewRuntimesはtest DIに使われる。
- plugin transactionのDocument cloneとpreview patchのshadow DOM cloneはrollback/atomicityのため、計測なしに除去しない。

## 3. 推奨する改善方針

1. Characterization first: valid wire、error、CLI、filesystem format、release artifact flowをtestで固定してから移動・抽出する。
2. Moveとlogic changeを分ける: module分割はbodyを変えないcommitにし、重複除去は後続commitにする。
3. 既存facadeを維持する: FlexiMarkAdapter、Rust public methods、binary entry、command IDsを残し、内部だけ委譲する。
4. 境界で検証する: JSONはunknownとして受け、daemon/adapter/webview/browserのtrust boundaryで共通validatorを通す。
5. Protocolの全面codegenは急がない: 先にRust serde例、schema、TS typeの構造的contract testを作る。
6. Errorの内部型と外部表現を分離する: typed categoryを保持し、唯一のmapperで現行code/messageへ変換する。
7. Security-sensitive処理はnamed state/stepにする: export、plugin、preview DOM、daemon recoveryに汎用frameworkを持ち込まない。
8. 依存削除はcall site、cargo tree、全target build/testで証明できたものだけにする。

## 4. Phase分割したリファクタリング計画

各Phaseは原則1つのPRとし、維持すべき挙動をreview checklistへ転記する。

### Phase 0 — 挙動固定とarchitecture baseline

- 状態: 2026-09-11 完了。production code/schema/public API/VSIX内容を維持したまま、計画した文書・fixture・test入口・gateを追加し、複数の独立レビューと `mise run verify` を通過した。
- 優先度 / ROI: P0 / 最高。最初の最小Phase。
- 目的: production codeを変えず、後続Phaseの互換性を判定可能にする。
- 問題点: architecture docsが欠落し、protocol/error/release/scriptの契約が文字列検査または巨大integrationに依存する。
- 対象ファイル: 新規ARCHITECTURE.md、README_DEV.md、scripts/verify_architecture.py、 新規scripts/tests、RPC/preview tests、fleximark-protocol/fleximarkd/CLI tests。
- 具体的な変更:
  - 現在のDAG、owner、entry point、trust boundary、永続/wire形式を文書化する。
  - 全custom methodの代表request/response、optional field omission、casing、現行error code/messageをcharacterizationする。
  - Python stdlib unittestでplatform mapping、manifest、RPC framing/checksum failureを実動作testする。
  - pure RPC/preview testのElectron外入口を追加し、既存Electron suiteも残す。
- 維持すべき挙動: production bytecode、public API、protocol/schema、CLI output、build artifacts、VSIX内容。
- リスク: 低。nondeterministic ID/token/pathをgoldenに含めない。test entryの登録漏れに注意する。
- 必要なテスト: Rust/TS/Python discoveryと既存49 TS/106 Rust testsの維持。
- Validation:
  - uv run --frozen python -m unittest discover -s scripts/tests -p "test_*.py"
  - uv run --frozen python scripts/verify_architecture.py
  - yarn exec tsc --noEmit
  - yarn exec eslint adapters web test scripts
  - cargo test --workspace --all-targets
  - mise run test
- 他Phaseへの依存: なし。全Phaseの前提。

### Phase 1 — Protocol/schema/runtime boundaryの強化

- 状態: 2026-09-12 完了。3周の独立レビューで見つかったRPC終端/recovery、合法array params、direction drift、session/revision相関、host failure隔離、optional DTOの不足を修正し、最終レビューで未解決P0〜P3なし、`mise run verify` 成功を確認した。
- 優先度 / ROI: P0 / 高。
- 目的: Rust、schema、TS間のdriftとmalformed messageによる未制御例外を防ぐ。
- 問題点: method→params/result対応が型で表現されず、request<T>とasが未検証JSONを信頼する。
- 対象ファイル: schemas/protocol.schema.json、fleximark-protocol、protocol.mts、rpc.mts、adapter.mts、preview hosts/core、contract tests。
- 具体的な変更:
  - methodとparams/resultを対応付けるTS type mapを1箇所に置く。
  - JSON-RPC envelopeと各notification/resultに副作用のないruntime validatorを追加する。
  - Rust serde fixtureをschema contract testへ通し、required/optional、enum、numeric constraints、casingを検査する。
  - invalid input時のconnection close / ignore / full snapshot request方針を固定する。
  - 全面codegenは行わない。
- 維持すべき挙動: protocol version、method/field、valid wire bytes、error code/message、message token、valid publication結果。
- リスク: 中。合法なoptional DTOを拒否するとreconnect loopやpreview不表示になる。
- 必要なテスト: 全method fixture、nested malformed、境界値、split/multiple/oversized frames、broken SSE、Rust→schema→TS互換。
- Validation: Phase 0一式、cargo test -p fleximark-protocol、browser/production extension build。
- 依存: Phase 0。

### Phase 2 — Build/release toolingのtest化と重複整理

- 状態: 2026-09-12 完了。2系統の独立レビューを反復し、Windows PATHEXTの移植可能な検証、daemon初期化失敗時のchild cleanup、artifact転送後のUnix executable mode消失を修正した。最終レビューで未解決P0〜P3なし、`mise run verify` 成功を確認した。
- 優先度 / ROI: P1 / 高。
- 目的: platform/arch、subprocess、build順序を単一の検証可能な定義へ寄せる。
- 問題点: 6 targetとOS正規化が重複し、scriptsにunit testがなく、CIがfull buildを重ねる。
- 対象ファイル: scripts/_tools.py、stage_daemon.py、smoke_vsix.py、create_release_manifest.py、build.py、tasks.py、performance script、scripts/tests、CI/release YAML、release tests。
- 具体的な変更:
  - Python側のsupported target、platform/arch normalization、executable名を1 moduleへ集約する。
  - subprocess timeout、stderr/stdout付きerror変換、entry point exit policyを共通化する。
  - mise run test単独では従来どおりfull buildし、build済みCI経路だけ重複buildを避ける。
  - YAML抽象化はartifact flowが読める範囲に限定する。
- 維持すべき挙動: 6 target、bin layout、manifest schema/hash、mise/package入口、--no-dependencies、partial manifest、watch cleanup。
- リスク: 中。Windows PATHEXT/path、arm64名、child cleanup、artifact pathを壊し得る。
- 必要なテスト: 全target、unsupported OS/arch、partial/require-all、stage path、timeout/nonzero exit、build順、watch cleanup。
- Validation: Python unit、mise run build/test/verify/package、各OS smoke。
- 依存: Phase 0。Phase 1とは独立。

### Phase 3 — 最終VSIXを一度だけ組み立て、同一bitsを検証・公開

- 状態: 2026-09-12 完了。release commit作成前に最終VSIXとidentity sidecarを生成し、tag/source identityを検証してGitHub draftとActions artifactへ引き渡し、6 target clean install、attestation、GitHub公開、Marketplace OIDCを同一SHA-256で直列化した。non-release、rerun、ZIP攻撃面、shell injection、tag/source identityを複数の独立レビューで反復検証し、`mise run verify` に成功した。
- 優先度 / ROI: P0 / 高。release副作用が大きいため単独Phase。
- 目的: clean-installしたartifactとGitHub Release/Marketplaceへ出すartifactをbyte-for-byte同一にする。
- 問題点: candidate smoke後、semantic-release prepareがversionの異なるVSIXを再packageする。
- 対象ファイル: release.yml、release.config.mjs、semantic-release-package.mjs、release contract tests。
- 具体的な変更:
  - release version確定後にuniversal VSIXを一度だけ生成する。
  - SHA-256をjob間で渡し、6 target clean-install、attestation、GitHub Release、Marketplaceが同じhashを使うことをgate化する。
  - 少なくともMarketplace publishは6 target smoke成功後に限定する。
  - version/changelog/commit規則は維持する。
- 維持すべき挙動: semantic version、changelog、release commit、universal VSIX、attestation、OIDC publishing、asset名。
- リスク: 高。semantic-release lifecycle、部分公開、artifact handoff、version順序。
- 必要なテスト: non-publish workflow、hash一致、6 target daemon initialize、zip/content、no-release path。
- Validation: workflow contract、local package/smoke、test repositoryまたはnon-publish end-to-end rehearsal。
- 依存: Phase 0、2。製品コードPhaseと混ぜない。

### Phase 4 — fleximark_serviceのmove-only module分割

- 状態: 2026-09-12 完了。3,666行のcrate rootを24行のprivate facadeへ縮小し、定義とtestを責務別moduleへ移動した。公開pathとvisibilityを維持し、export state machineのlogic整理は行っていない。永続化bytes/digestとUnix symlink・Windows junction境界を追加contract testで固定し、独立レビューを複数巡した後に `mise run verify` に成功した。
- 優先度 / ROI: P1 / 高。
- 目的: filesystem/security-sensitive exportと通常workspace commandのreview範囲を分ける。
- 問題点: 3,644行にnote/theme/config/plugin/assets/export/recoveryが同居する。
- 対象ファイル: fleximarkd/src/lib.rs、新規workspace、notes、theme、assets、plugins、uri、export/model/journal/filesystem/recovery modules、tests。
- 具体的な変更: 最初は定義/testの移動とre-exportのみ。関数body、public path、visibility、serde型を変えない。export state machine整理は別commitにする。
- 維持すべき挙動: public API、config/default、note、theme policy、asset allowlist、marker/registry/journal bytes、digest、fsync/rename順、symlink/swap防御、recovery、user file保護。
- リスク: 中。Rust privacy、Windows cfg、fault injection、include path。logic変更を同時に行うと非常に高リスク。
- 必要なテスト: 既存25 service tests、全journal crash point、3 OS filesystem、symlink/junction/swap、config/note/theme/assets。
- Validation: cargo test -p fleximarkd --lib、workspace fmt/test/clippy、mise run verify。
- 依存: Phase 0。Phase 3とは独立。

### Phase 5 — daemon routing、commands、preview HTTPの責務分離

- 状態: 2026-09-12 完了。daemonを責務別private moduleへ分割し、command、request decode、change-document、transport、preview HTTPを小さなnamed operationへ整理した。追加characterization testと4巡の実装・複数独立レビューで順序・security境界を照合し、初回full gateで見つかったrelease testの旧owner参照も新moduleへ追従させた後、`mise run verify` に成功した。
- 優先度 / ROI: P1 / 高。
- 目的: Serverのstate ownershipとrequest/response変換を明確にする。
- 問題点: transport、routing、LSP、RPC、commands、preview、cancellation、telemetryが一体化し、guard/deserialize/error mappingが反復する。
- 対象ファイル: fleximarkd/src/main.rs、新規transport/stdio、server/lsp/rpc/commands、preview_http、cancellation、diagnostics、telemetry modules。
- 具体的な変更:
  - move-only module化後、共通request guard/deserializer/wire error mapperを抽出する。
  - execute_commandをcommand別private handlerへ分け、wire Stringは維持し内部だけenum化する。
  - change_documentのedit、asset refresh、preview/diagnostics publishをnamed operationへ分ける。
  - HTTP socket I/O、parse、route/policy、responseを分ける。web frameworkは追加しない。
- 維持すべき挙動: stdio/LSP/CLI、message order、cancellation、trust、HTTP status/header/CSP、token/host/origin/rate-limit/SSE、revision、error wire。
- リスク: 高。race/order、cancellation、preview security、browser navigation。
- 必要なテスト: byte-level RPC、cancel timing、LSP/RPC同一document、全command、HTTP malformed/oversize/security、SSE、navigation revision。
- Validation: cargo test -p fleximarkd --bin fleximarkd、workspace gate、browser build、mise run test/verify。
- 依存: Phase 0、1。Phase 4と直列化する。

### Phase 6 — engine document/render pipelineの一元化

- 優先度 / ROI: P1 / 高。
- 目的: authoritative parse/plugin/validate/render順序を1つのprivate pipelineにする。
- 問題点: open/update、plugin有無、change/resync、full/patchで類似処理が重複する。
- 対象ファイル: fleximark-engineをsession、pipeline、render、diff、identity、provenance、assets等へ段階分割。
- 具体的な変更:
  - previous document、version、source、plugins、cancellationからcandidate/diagnosticsを作る共通operationを導入する。
  - sync-state transitionとsource installを分ける。
  - fingerprint、rendered blocks、revision準備を共通化し、full/patchはpublication差だけにする。
  - full_snapshotがenumを返してunreachableでunwrapする:877-886はprivate RenderSnapshot builderへ単純化する。
  - public methods/signaturesはfacadeとして維持する。
- 維持すべき挙動: hook順、required/optional、diagnostic順、provenance、NodeId、rollback、checkpoint、fingerprint、revision、full fallback、patch bytes。
- リスク: 高。NodeId、unicode provenance、cache revision、policy change時full fallback。
- 必要なテスト: open/update/resync equivalence、plugin有無、各cancellation、ambiguous move、unicode edit-map、config/asset変更、wire golden。
- Validation: cargo test -p fleximark-engine、workspace gate、performance budget、mise run verify。
- 依存: Phase 0、1。Phase 5と同時変更しない。

### Phase 7 — plugin hostの分割と限定的重複除去

- 優先度 / ROI: P1 / 中〜高。
- 目的: sandbox、package verification、hook orchestration、edit-map/candidateを別々にreview可能にする。
- 問題点: 2,550行の単一fileと5 hookの反復。
- 対象ファイル: fleximark-plugin-host/src/lib.rs、新規runtime/wasmtime、package、pipeline、edit_map、candidate、error modules。
- 具体的な変更: move-only分割後、trust gate、required/optional failure disposition、diagnostic constructionだけをhelper化する。hook固有validation/transactionは残す。
- 維持すべき挙動: WIT/serde、signature/hash、config order、resource limits、WASI preopens、cancellation、hook order、optional rollback、required abort、unsafe grant。
- リスク: 高。共通化しすぎるとhookごとのcommit単位を変える。
- 必要なテスト: 既存20 tests、全hook×required/optional、trust×grant、limits/cancel、malformed、unicode、fixture component。
- Validation: cargo test -p fleximark-plugin-host --all-targets、workspace gate、fixture wasm、mise run verify。
- 依存: Phase 0。Phase 6と別PR。

### Phase 8 — LSP session index、workspace authority、内部error型の整理

- 優先度 / ROI: P1 / 中。
- 目的: URI↔session invariantとmulti-root configurationを1つの内部モデルに寄せる。
- 問題点: 二重map、単一workspace互換state、String ID/errorへの早期変換。
- 対象ファイル: fleximark-lsp/src/lib.rs、daemon contract tests。
- 具体的な変更:
  - private SessionIndexにdocuments/session URIs同期を閉じ込める。
  - internal newtypeを用い、wire boundaryだけStringへ変換する。
  - configure_pluginsは削除せずcanonical workspace経路へのcompatibility wrapperにする。
  - workspace matchingを隔離し、現行の長さ順prefix semanticsを維持する。
  - EngineError categoryをwire mapperまで保持する。
- 維持すべき挙動: public methods、IDs、UTF-8/16/32、attach/checkpoint、full-text request、nested multi-root、trust、error wire。
- リスク: 中〜高。URI normalization、case、percent encoding、nested rootsはsecurity-sensitive。
- 必要なテスト: open/change/close/attach/checkpoint、index invariant、duplicate/stale、nested roots、invalid root隔離、UTF-16、reconfigure rollback。
- Validation: cargo test -p fleximark-lsp、daemon tests、workspace gate、mise run test。
- 依存: Phase 0、1。Phase 6後が望ましい。

### Phase 9 — VS Code adapterのpure seamとdaemon supervisor分離

- 優先度 / ROI: P1 / 高。
- 目的: process/recovery state machineをdeterministic unit test可能にする。
- 問題点: helper、manifest、spawn、backoff、workspace revision、logging、document/preview stateが単一classに集中し、resetも重複する。
- 対象ファイル: adapter.mts、rpc.mts、新規release-manifest、daemon-supervisor、workspace-selection、position、error-policy modules、tests。
- 具体的な変更:
  - pure helperとmanifest verificationを先に移す。
  - DaemonSupervisorへprocess、RPC、timer/backoff、workspace revision、state resetを集約し、spawn/clock/timer/log/statusを注入可能にする。
  - FlexiMarkAdapter public APIはfacadeとして残す。
  - redaction/log/reportを集約するがuser-visible textは変えない。
  - 未使用private stateと引数は小commitで除去する。
- 維持すべき挙動: commands、single daemon/multi-root、manifest hash/path、最大5回、30秒reset、backoff、replay、status/output、shutdown。
- リスク: 高。launch/restart race、dispose、late child exit、workspace更新中replay。
- 必要なテスト: fake process/RPC/clockでspawn failure、上限/reset、concurrent ensure、workspace change、dispose、late exit、multi-root recovery。
- Validation: Node unit、tsc、ESLint、Electron adapter、mise run test/verify。
- 依存: Phase 0、1。Phase 2後が望ましい。

### Phase 10 — document/preview coordinatorとextension wiringの分離

- 優先度 / ROI: P1 / 高。
- 目的: document sync、preview lifecycle、diagnostics/navigation、registrationを独立test可能にする。
- 問題点: openPreview、checkpoint/change、message dispatch、source navigation、providers/commands/eventsが密集する。
- 対象ファイル: adapter.mts、extension.mts、新規document-coordinator、preview-coordinator、diagnostics、commands、providers、tests。
- 具体的な変更:
  - didOpen/didChange/checkpoint/full-text requestをdocument coordinatorへ移す。
  - CSP/token、preview session/revision/dispose/recreate、navigation/echoをpreview coordinatorへ移す。
  - DTO converter、command registration、event wiringを小関数moduleへ分ける。class/interface階層は増やさない。
- 維持すべき挙動: 150ms checkpoint、unsaved recovery、CSP/token、column、revision/session、open/ack順、echo、diagnostics、command/provider IDs。
- リスク: 高。event順、double dispose、stale generation、selection echo、subscription漏れ。
- 必要なテスト: late edit、debounce、requestFullText、stale token/generation/revision、close/remove/recreate、diagnostics、全registration、multi-root。
- Validation: Node unit、tsc/ESLint、production build、Electron integration、mise run verify。
- 依存: Phase 1、9。

### Phase 11 — Preview core/enhancerの分割と失敗隔離

- 優先度 / ROI: P1 / 中〜高。
- 目的: DOM security transactionと機能別async rendererを分け、失敗したenhancerがpreview全体を壊さないようにする。
- 問題点: PreviewDocument.applyPatchとPreviewEnhancerが多機能・多状態で、host間のvalidation/error処理が不一致。
- 対象ファイル: web/preview-clientのindex、enhance、host、browser/vscode host、navigation、runtimes、preview tests。
- 具体的な変更:
  - DTO、content/security parser、asset staging、patch transactionをprivate modulesへ分け、PreviewDocument facadeを維持する。
  - Mermaid/ABC/audio/math/YouTube/tabs/highlightを単純な機能moduleへ移す。機能ごとのclass群は作らない。
  - enhancer promise、SSE parse、navigation POSTの失敗方針をhost共通にする。
  - asset二重atobの解消は別commit。identityMap最適化はbenchmark後だけ行う。
- 維持すべき挙動: atomic clone-commit、allowlist、CSP/token、fallback、asset制限/revoke、各renderer、user gesture/consent、a11y、async cancellation。
- リスク: 高。XSS、partial commit、stale async、audio cleanup、blob leak。
- 必要なテスト: snapshot/patch/security/assets/runtimes/navigation/hosts別suite、nested malformed、途中失敗、dispose中render、broken SSE/fetch。
- Validation: Node DOM unit、tsc/ESLint、browser/extension bundle、Electron、performance、mise run verify。
- 依存: Phase 0、1。Phase 10と別PR。

### Phase 12 — Test/build境界、dependency hygiene、低リスクcleanup

- 優先度 / ROI: P2 / 中。
- 目的: feedbackを高速化し、確認済みの不要依存・dead state・docs driftだけを除去する。
- 問題点: Node/DOM/VS Code ambient typesが単一tsconfigに混在し、pure testsもElectron実行。MSRV未検証。
- 対象ファイル: tsconfig群、test entries、build/tasks/CI、fleximark-cli/fleximarkd Cargo.toml、dependabot、README、capability history。
- 具体的な変更:
  - base、Node adapter、browser、unit、Electronのtsconfigを分け、tsc -bで境界違反を検出する。
  - pure unitとElectron integrationを別gateにし、固定sleepをevent/pollUntilへ置換する。
  - fleximark-cliの未使用direct fleximark-parserを全target確認後に削除し、fleximarkdのbase64 versionをworkspace指定へ統一する。
  - Rust 1.85 MSRV checkをCIへ追加する。MSRVは引き上げない。
  - l10n説明を実装へ合わせる。歴史inventoryは用途確認後に保持理由を記すか削除する。
- 維持すべき挙動: build/test/package入口、bundle、public types、Cargo features/lock、MSRV、監査要件。
- リスク: 中。tsconfig/esbuild差、test globals、CIずれ、MSRV問題の顕在化。
- 必要なテスト: unit/Electron discovery、cargo tree before/after、1.85/current toolchain、lock diff、package contents。
- Validation: yarn exec tsc -b、unit/integration、cargo +1.85 check --workspace --all-targets、current fmt/test/clippy、cargo tree -p fleximark-cli -e normal、mise run verify、package/smoke。
- 依存: Phase 2、9〜11。

## 5. 各Phaseのリスク

| Phase | リスク | Revert単位 |
| --- | --- | --- |
| 0 | 低 | docs、Python tests、contract testsを個別revert |
| 1 | 中 | validatorをboundaryごとにrevert |
| 2 | 中 | Python共通化とCI build削減を別commitにする |
| 3 | 高 | release workflowだけを一括revert |
| 4 | 中 | move-only module単位。logic変更を混ぜない |
| 5 | 高 | transport、handler、HTTPを別commitにする |
| 6 | 高 | document pipelineとrender pipelineを分ける |
| 7 | 高 | move-onlyとhook共通化を分ける |
| 8 | 中〜高 | index、workspace matching、error型を分ける |
| 9 | 高 | helper、supervisor、dead state除去を分ける |
| 10 | 高 | document、preview、wiringを分ける |
| 11 | 高 | core move、enhancer、最適化を分ける |
| 12 | 中 | tsconfig/test、Cargo依存、docsを分ける |

Phase 6〜11は並行実施せず、直前のfull gateがgreenであることを着手条件とする。

## 6. テスト・検証方法

### 各commitの最小gate

~~~sh
cargo fmt --all --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
yarn exec tsc --noEmit
yarn exec eslint adapters web test scripts
uv run --frozen python scripts/verify_architecture.py
~~~

Phase 12後はTypeScriptの正規入口をyarn exec tsc -bにする。

### Phase/PR完了gate

~~~sh
mise run verify
~~~

filesystem/exportを触るPhase 4はWindows/Linux/macOS、releaseを触るPhase 3は6 platform/archで確認する。

### 固定すべき外部契約

- package.json: activation、command IDs、settings/default/enum、main path。
- JSON-RPC/LSP: version、method、casing、optional、error、notification order、frame上限。
- Preview: full/patch、revision/fingerprint、atomicity、CSP/token、asset、navigation。
- Config/plugin: TOML version、unknown field、WIT/API、signature/hash、deny-by-default。
- Filesystem/export: destination、marker、registry/journal、digest、recovery、user file/symlink。
- CLI/network: subcommand、stdout/stderr/exit、loopback、HTTP/SSE。
- Distribution: 6 target、manifest SHA、VSIX内容、tested/published artifact hash一致。

Performance最適化はcapabilities/performance-budgets.jsonと追加benchmarkで必要性を確認してから行う。

## 7. 実施しない方がよい変更

- protocol/public command/field/casing、config/plugin/WIT/schema、journal/marker形式の整理目的変更。
- 初手のprotocol全面codegen、Rust crate統合・大量新設、fleximark_service別crate化。
- Adapter、daemon、export recovery、engine pipelineの一括rewrite。
- parser/renderer統合、web framework置換、exportへのasync runtime導入。
- plugin hooksのmacro/高度generic framework化。
- PreparedExport / ResolvedExport、PreviewRuntimes、shadow DOM transaction、CSP/token、hash/signature、--no-dependencies、performance budgetの削除。
- 計測なしのclone除去、in-place DOM patch、dependency upgrade。
- URL/hash/path helperを用途差の確認なしに共通util crateへ移すこと。
- repo内call siteがないpublic APIの削除。compatibility wrapperに留める。
- error code/message、user-visible text、redaction挙動をリファクタと同時変更すること。
- skipLibCheck解除やnoUncheckedIndexedAccess等の全体一括導入。
- 未検証MSRVへの対処としてRust 1.85を単に1.95へ上げること。
- final smokeなしでcandidate smokeを削除すること、またはplatform別VSIXでuniversal exact-artifact性を失うこと。

## 8. 最初に実施すべき最小Phase

最初の実施単位はPhase 0だけとする。

現状は主要static checkと106 Rust testsがgreenだが、今後触る箇所はprotocol、release、security state machine、daemon recoveryである。Phase 0はproduction codeを変えず、次の4点だけを追加するためindependently reviewable、testable、revertableである。

1. 欠落したarchitecture文書にDAG、owner、entry point、trust boundary、不変条件を書く。
2. session error、workspace command error、全custom methodのserde/schema例をcharacterizationする。
3. Python build/release scriptsを実行するunit testを追加する。
4. pure RPC/preview testsの独立入口を作るが、既存Electron suiteは残す。

Phase 0は2026-09-11、Phase 1〜5は2026-09-12に完了した。固定した外部挙動を基準に、現在の実施指示どおりPhaseを混在させずPhase 6へ進む。
