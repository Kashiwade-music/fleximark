# FlexiMark アーキテクチャ

FlexiMark は純粋な文書処理、状態と native I/O を所有する application、通信境界、editor 製品で構成する。VS Code と Zed は同じ native runtime を使用する。

実依存は [crate 図](../architecture/crates.svg)、内部参照は [module 図](../architecture/modules/)、Zed は [独立 workspace の図](../adapters/zed/architecture/crates.svg) を参照する。図は固定した xtask から生成する。

## 所有者と依存方向

| 配置 | 所有するもの |
|---|---|
| core/model | IR、UTF-8 byte 範囲、SourceIndex、NodeId、構文記述子、visitor、AssetRef/Sha256Hex |
| core/parser | comrak からの IR 変換、構文認識とモデル検証 |
| core/render-html | 安全な HTML、ProtectedFragments、resource policy |
| core/engine | 全文解析、NodeId 照合、候補反映、immutable render snapshot |
| core/language | byte 範囲の補完と semantic 分類。LSP の形や数値は持たない |
| application | DocumentStore、WorkspaceAuthority、WorkspaceOps、preview と native 操作の権威状態 |
| contracts/protocol | 外部 DTO、command/型/method 記述子、protocol version、RPC error code |
| contracts/config | 現形式の設定と検証 |
| contracts/distribution | target・payload・上限の正本、Rust 共通 manifest 検証 |
| transports/wire-mapping | native frame/range と外部 DTO の検査付き変換 |
| transports/lsp | LSP、actor、worker 配送、位置符号化、言語結果と数値の変換 |
| transports/preview-http | loopback HTTP、認証済み browser UI、SSE と配信 sequence |
| runtime/preview-assets | 検証済み runtime 資源の読取り |
| apps/server / apps/cli | fleximark-server crate の fleximarkd binary / fleximark CLI の composition root |
| web/preview / web/workspace-ui | 共通 preview session・DOM・enhancer / workspace 操作フォーム |
| adapters/vscode / adapters/zed | editor API、接続、起動、製品固有 UI |
| migration | 明示された旧ユーザーデータの計画・backup・適用・復旧 |
| tooling/codegen / dev/xtask | 契約生成 / 構造検査と renderer fixture・構文定数生成 |
| dev/test-support | テスト用の一時 workspace builder |

依存は core ← application ← transport ← apps を基本とする。core は filesystem・protocol・browser bundle を import しない。protocol は core/application/runtime を import しない。wire-mapping が両側を接続する。構造 policy は実依存の最小集合を明示する。

## 文書処理

文書本文の権威経路は標準 didOpen・didChange・didClose だけで、全文同期する。内部位置は UTF-8 byte 範囲を使い、SourceIndex が通信境界で UTF-8/16/32 の行・文字位置と変換する。

1. 全文解析とモデル検証で候補 IR を作る。
2. 既存 IR と照合して NodeId を維持し、成功した候補だけを反映する。

DocumentEngine の source・IR・描画設定は Arc で共有できる snapshot である。失敗・取消・counter 上限で以前の公開 frame を失わない。identity は長さ付きの明示符号化と blake3 を使い、資源の完全性には SHA-256 を使う。

構文は model の const 記述子を parser・language・renderer が共用する。math fence も Math として解析する。IR の走査と navigation の重複 NodeId 検査は model に集約する。render-html は leaf 値型、block/inline、resource policy、sanitize、再帰の調停を分離する。ProtectedFragments が内部 marker の保護・ammonia・差し戻しを一度だけ行う。

## 操作と並行性

WorkspaceCommand と CommandContext を受ける WorkspaceOps が共通の認可と操作入口である。COMMAND_DESCRIPTORS が名称、workspace/document/trust 要件、Code Action・browser・CLI 対応を所有する。custom executeCommand、標準 workspace/executeCommand、HTTP、CLI が同じ操作を使い、transport が CommandOutcome を外部 CommandResult へ変換する。

時間のかかる操作は prepare → worker execute → actor complete と進む。worker は source/config/resources の owned snapshot を受け、同じ root の操作は worker 側で直列化する。actor は待たずに文書・言語要求を処理する。root/trust/config の lease が失効した queued 操作は拒否する。終了時には pending reply と permit を解放する。

初期化や migration と本文変更が競合した場合、OperationProgress::Continue で最新 snapshot の asset 解決を同じ worker へ再配送する。native 副作用は一度だけ実行する。commit 済みの操作を後から本文が変わったことだけで失敗にしない。検証済み config は Arc cache、asset は参照集合と mtime/size の memo を使う。cache hit でも containment・nofollow・file identity を再確認する。

ApplicationError が下位 cause を保持し、ErrorKind が NotFound・Denied・Conflict・Invalid・Cancelled・Internal を区別する。RPC/HTTP の変換は各境界に一つだけ置く。ログは raw URI・本文・token を出さず、分類と hash を記録する。

## preview の所有

application が Arc<RenderFrame> と単調な revision の正本を持つ。PreviewHandle は同じ frame と live document authority を共有する。source epoch・同期状態・公開 token・close 状態により、同じ document version の再同期でも旧 handle を失効させる。HTTP は handle を参照し、LSP は revision/version を複写しない。

完全な frame を初回・更新・再接続で push し、読取りでは revision を進めない。通知後の frame 再取得は行わない。navigation は daemon identity、document session/version、preview revision、NodeId を照合する。

sourceNavigationEvents を宣言する VS Code には source 移動通知だけを送る。Zed など宣言しない client には標準 window/showDocument だけを送る。同じ移動を両経路で配送して選択や scroll を上書きしない。

HTTP は 127.0.0.1 の動的 port を使い、page token・Host・Origin と CSP を検証する。SSE は通常 request pool と別に最大16接続を持ち、更新通知と約15秒の heartbeat で接続を維持する。再接続は最新 frame から始まり、終了時に全 socket を閉じる。通常 request は絶対 read 期限と write timeout を使う。

表示側は PreviewSession が frame/navigation の順序と一度の回復を調停する。bridge/SSE 境界で完全検証し、その後は型付き frame を渡す。DOM の topology・整合性・rollback 検査は残す。変更のない DOM は保持し、enhancer registry の Disposable を element に結び付ける。除去・取消・画面終了で audio、非同期 diagram、dialog を破棄する。

## editor と workspace

接続は標準 initialize 一回で成立し、experimental.fleximark が daemon identity と能力を返す。VS Code の DaemonConnection は rpc/generation/identity/AbortSignal をまとめ、交換時に旧非同期完了を失効させる。PreviewController は embedded/external を区別し、binding の交換と remote disposal を一か所で行う。external は revision だけを保持する。

workspace/didChangeWorkspaceFolders が root を増減する。追加 root は restricted、reconfigureWorkspace.trusted が trust の変更を伝える。削除した root の文書は detached に戻し、theme/assets を除いて本文・session・preview を維持する。workspace の増減や単独 Markdown の追加では daemon を再起動しない。再起動は crash 回復だけに使う。

trust がない文書も本文を preview できるが、workspace 書込み・theme・local asset の権限は推測しない。cap_std と nofollow で symlink/junction と包含を検証する。Zed は Code Action と標準 executeCommand/showDocument から browser preview・workspace UI・ソース移動を利用する。

## 契約・配布・移行

WIRE_TYPES の一表が schema callback、実際の型名と semantic validator key を所有し、method 記述子が表を参照する。codegen は core に依存しない。renderer fixture と syntax 定数は xtask が生成する。

release.toml が製品 version、distribution.json が target・VS Code 対応・payload・サイズ上限を所有する。Rust、Python、生成 Node/TS/schema、CI matrix が同じ配布仕様を使う。runtime ZIP は二つの binary と preview/workspace 資源、manifest を含む。VSIX は同じ archive を取り込み、Zed も同じ検証器を使う。Zed 拡張自体の WASM build は維持する。

protocol/config/runtime manifest は version 1、リリースまでは形式変更でも上げない。旧形式の fallback や document schema は持たない。WASM plugin 機構は撤去した。config の plugins は通常の未知フィールドとして拒否する。

旧 JSON/theme/editor 設定は明示 migration だけで読む。planMigration の内容を確認し、同じ planId を applyMigration へ渡す。workspace と入力 snapshot を再検証し、再利用を拒否する。legacy JavaScript plugin は移行せず代替も提供しない。export は ownership と journal による transaction と復旧を保持する。

## 用語

| 文書内の語 | コード上の名前・意味 |
|---|---|
| preview revision | renderRevision / render_revision。preview ごとの公開順序 |
| document version | documentVersion / document_version。editor が送る本文の版 |
| daemon identity | daemonInstanceId。同じ process の識別子 |
| connection generation | DaemonConnection.generation。editor の接続交換の世代 |
| document session | documentSessionId。同じ URI の close/reopen も区別 |
| workspace | trust・config・操作権限の単位 |
| root | WorkspaceAuthority に登録された workspace URI |
| folder | editor の WorkspaceFolder。通知境界で root に変換 |
| detached document | workspace_uri を持たない文書。本文 preview は維持 |

検証とリリース操作は [開発手順](DEVELOPMENT.md)、実装を変更する単位は [実装境界](IMPLEMENTATION.md) を参照する。
