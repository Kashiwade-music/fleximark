# 公開runtime振る舞いテストの観点対応

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

T5/T6の安定点ではP4-T6の型metadataにentry.schemaで最小追従した。以下はその後のP4-T6作業として分けた変更である。

## P4-T6の型とvalidator

| 旧知識・検査                                             | 新しい正本・対応                                                                                |
| -------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| phantom型のboolean Validator                             | 実type predicateと生成WireTypeMap。protocol-method-map.type-testでunknownからの型絞り込みを検査 |
| DTO名別semantic switchとschema検査の別経路               | 生成entry.schema/semantic→validateWire→4semantic handlerの一経路                                |
| EditorNavigationEvent/逆向きPreviewNavigationEvent alias | 生成PreviewNavigationEvent（browser→source）/RenderNavigationEvent（source→render）を直接利用   |
| rpc側のenvelope複写                                      | contracts/protocolの公開message/response envelopeを利用                                         |
| 4か所のobject guard                                      | contracts/guardsのisRecordを利用                                                                |
| NavigationUpdate/PreviewNodeEvent/位置と範囲/token一覧   | 生成navigation型（必要部分はOmit）、TextPosition/TextRange、SEMANTIC_TOKEN定数を利用            |
| validator成功後のconsumer cast                           | typed guardとgeneric RPC completionから除去。LSP標準methodの明示的untyped入口は維持             |

protocol-contractではframe predicate、汎用型guard、createPreview result、previewChanged通知の全経路でfingerprint、重複node/block、navigationの存在と一意性、範囲、asset参照・base64長、envelope/session/revision整合を検査する。既存invalid fixtureは変更せず、positive生成fixtureのouter identityは親がinner frameに一致させた。

semantic handlerへの型接続は、schema検査後に限る内部dispatch adapter一か所に閉じた。公開guardはschema検査とsemantic検査を実際に通し、型だけで値を信頼するguardは作らない。
