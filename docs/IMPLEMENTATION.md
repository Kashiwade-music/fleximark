# 実装の所有境界

全体の依存方向と用語は [ARCHITECTURE](ARCHITECTURE.md)、実行入口は [DEVELOPMENT](DEVELOPMENT.md) に記す。この文書は変更を加える場所と維持する条件を整理する。

## pure core

| 変更 | 実装場所・条件 |
|---|---|
| 組込み構文 | model/syntax.rs の const 記述子。parser・補完・render kind・生成TS定数が参照する |
| IR と位置 | model/nodes.rs、validation.rs、traversal.rs。内部範囲は UTF-8 byte、SourceIndex を境界で使う |
| 解析 | parser。comrak の対応を明示し、未知ノードを Debug 文字列にしない |
| identity | engine/identity。意味フィールドを長さ付きで符号化し、既存IRとの照合でNodeIdを維持 |
| 描画 | render-html の types/escaping/resource_policy/block/inline/rendering/sequences/sanitize |
| 補完とtoken | language。byte範囲と内部enumを返し、LSPのDTO・数値・位置単位を持たない |

DocumentEngine は open/change/resync/set_render_config/render に集約する。source、document、render config の Arc clone は worker snapshot に使える。parse・検証と identity 照合が成功した候補だけを反映し、失敗時に以前の文書とframeを壊さない。

ProtectedFragments が raw HTML の保護・浄化・復元を共通化する。内部markerとの衝突、data-fleximark属性の剥奪、禁止URLのlabel描画を保持する。rendering が再帰を調停し、blockとsanitizeへcallbackを渡してmodule循環を作らない。navigationと重複NodeIdの検査はmodelに委ねる。

## application

DocumentStore が本文、workspace所属、previewを所有する。native module は非公開で、外部には必要な値型と WorkspaceAuthority/WorkspaceOps を公開する。ApplicationError は詳細と下位causeを保持し、kind() が境界の分類に使われる。

WorkspaceOps::prepare は command記述子の要件と権限を検証し、owned snapshotを取り出す。workerのexecute後、completeがroot leaseとdocument epochを照合してstateを反映する。Continueはasset再解決だけを再配送し、元の書込みを再実行しない。同期CLIも同じ進行を最後まで回す。

workspace rootの追加はrestricted、削除はdetached文書へ変換する。検証済みconfigはWorkspaceAuthorityのArc cacheで保持する。config/theme/trust/root変更でasset memoを無効化し、参照集合とmtime/sizeが同じ場合だけbytes/hashを再利用する。cache hitでもcanonical containment、linked path拒否、cap_std nofollow openとfile identity確認を省略しない。

RenderFrameはimmutable snapshotとsourceを持つ。application内のPreviewHandleがframe Arc、live authority、source epoch、publication tokenを共有する。failed renderや同versionの再同期でも古いnavigationを受理しない。counter上限・取消では旧frameを保持し、closeはhandleを失効させる。

exportはnative/exportのvalidation、ownership、journal、transactionへ分ける。ownershipとjournalの検査、ユーザー資源の保護、置換意図と復旧を維持する。migration計画はworkspaceと入力snapshotを所有し、同じplanIdの確認後だけ適用する。

## protocol と transport

protocolは自身のwire DTOだけを定義する。WIRE_TYPESの一表がschema callback・型名・semantic keyを持ち、method記述子が型キーを参照する。受信serdeとschemaの両方でunknown fieldと安全なcounter/hash/asset形式を検証する。

codegenはprotocol/config/distributionからschemaとTS契約を生成し、coreには依存しない。xtask fixturesがrenderer由来のfixtureと組込み構文のTS定数を生成する。Pythonのprotocol生成入口が両者を呼び、verifyでは契約確認を一回にする。

wire-mappingがnative frame/rangeを外部DTOへ検査付きで変換する。LSPが交渉したUTF-8/16/32位置と数値enum、diagnostic/Code Action等を構築する。本文同期は標準didOpen/didChange/didCloseだけを使う。

stdio actorはpending request/reply/permitを保持し、workerはcompletion eventだけを返す。同じworkspace laneで処理を直列化する。HTTP workspace操作も同じtyped commandを使い、1秒で応答を打ち切らない。終了時にはpendingを取り消してreplyを解放する。

HTTPはapplication handleを参照し、page tokenと配信sequenceを所有する。frameとversionを複写しない。SSEは最大16の独立した接続、更新Condvar、heartbeatを使う。Host/Origin/token/CSP、requestサイズ、絶対read期限とwrite timeoutを維持する。

## editor と browser

VS Code adapterは接続・command・editor event・preview controllerの配線を行う。DaemonConnectionのabortで旧非同期処理を失効させる。PreviewControllerの交換処理だけがbindingとremote previewの寿命を管理する。embeddedは配送用frameを持ち、externalは確認済revisionだけを保持する。

workspace増減は標準folder通知、trustはreconfigureWorkspaceで伝える。spawn cwdはruntimeに固定し、workspace集合の変化ではprocessを再起動しない。test入口はExtensionMode.Testのextension APIへ閉じる。

PreviewSessionはframe適用・navigation順序・回復を共通化する。hostはbridge/SSE認証と配送に限定する。完全frame検証は受信境界で行い、DOM側の整合性とrollback検査は保持する。enhancer registryのDisposableがelement除去・取消・画面終了で非同期描画と音声を解放する。CSSは機能別sourceから一つのfleximark.cssへbundleする。

Zedは通常editorのCode Action、workspace/executeCommand、window/showDocumentを使用する。継続的なviewport追跡は契約に含めない。browser workspace UIはcontext選択と型付きformを提供し、migrationの確認を経て同じplanIdを適用する。

sourceNavigationEventsがtrueのclientにはsource移動通知だけ、falseまたは省略時にはwindow/showDocumentだけを配送する。VS Codeはtrueを宣言し、selectSourceとrevealSourceの選択・scrollの違いをadapterで保持する。

## 配布・開発

release.tomlを製品version、contracts/distribution/distribution.jsonをtarget/payload/上限の正本とする。Rust共通RuntimeDistribution、Python組立・検証、生成Node/TS/schema、CI matrixが同じ仕様を使う。runtime ZIPをVSIXへそのまま取り込み、Zedも同じmanifest検証を使う。

task入口はtasks.py、Plan/Stepとworkflow・製品・JS・watch・検査・release操作はtask_*.pyへ分ける。独自module CLIやprepublish経路を持たない。source、.build、.artifactsを分離する。CIとtag releaseはbuild.ymlでビルドとパッケージ作成を共用し、draftを手動確認してから公開する。全検証はverify.ymlとローカルverifyで手動実行し、CodeQLも手動実行する。

構造検査は固定tooling/xtask-coreを使い、依存allowlist、module循環、Role、1000未満のcode linesを検査する。TSはpackage projectとbuiltin ESLintで境界と行数を強制する。pure coreのテストはNode出力を必要としない。一時workspace builderはdev/test-supportに共通化し、export wire/security、worker競合、実framingのadapter、browser/editor、配布artifactを各境界で検証する。
