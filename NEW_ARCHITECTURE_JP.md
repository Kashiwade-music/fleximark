# FlexiMark 新アーキテクチャ（レビュー用要約）

## 1. 決定事項

- 状態: 承認済み
- 対象リリース: 次のメジャーバージョン
- 移行方式: 新旧ランタイムを併存させない一括置換
- 中核実装: Rust
- 最初のエディタ: Visual Studio Code
- 将来候補: Zed、Neovim、Helix、JetBrains IDE など

FlexiMark を、エディタに依存しない Rust 製ドキュメントエンジンとして再構築する。
VS Code 拡張は製品ロジックを持つ本体ではなく、エンジンを利用する最初の薄いクライアントになる。

現行 TypeScript 実装は要件の発見元として使うが、内部 API、生成 HTML、設定形式、
MDAST/HAST フック、`parserPlugin.js` との互換性は保証しない。HTML は正本ではなく、
FlexiMark が所有する Document IR から生成する出力形式の一つとする。

## 2. 目標と対象外

### 目標

- ライブプレビュー、拡張 Markdown、HTML 出力、ノート操作を維持する。
- Mermaid、ABC 記譜、数式、ソース対応、エディタとプレビューの相互移動を支える。
- 同じエンジンをエディタ、CLI、CI、外部ブラウザから利用できるようにする。
- 障害分離、キャンセル、再起動、ログとトレースを設計に含める。
- エディタ SDK、パーサー固有 AST、HTML DOM をコアの公開境界へ漏らさない。
- バージョン付きプロトコルと機能交渉を採用する。
- `parserPlugin.js` を、権限制御された WASM/WASI プラグインに置き換える。

### 対象外

- 現行 HTML のバイト単位での再現
- 現行 TypeScript API やモジュール名の維持
- 既存 JavaScript plugin の実行
- Mermaid のレイアウトや ABC の音声再生を Rust で実装すること
- 初回リリースでの Zed 拡張
- リリース後の旧エンジンへの切り戻し経路、二重書き込み、二重設定

開発作業は分割してよいが、製品として切り替える時点は一度だけとする。

## 3. 全体構成

```text
VS Code / 将来のエディタ / CLI
        │  LSP + fleximark/* JSON-RPC
        ▼
fleximarkd
  session / command / plugin / filesystem / preview server
        │  Rust API
        ▼
FlexiMark core
  parser / Document IR / transform / diff / renderer / source map
        │  preview protocol
        ▼
Webview / Browser
  DOM patch / Mermaid / abcjs / interaction
```

依存方向は常にドメイン側へ向ける。エディタ、ファイルシステム、Web 通信、
プラグイン実行環境、renderer は adapter として実装する。エディタごとの差は名前判定ではなく、
クライアントが宣言した機能で処理する。

想定する主な配置は次のとおり。

```text
crates/
  fleximark-model/       # 公開 domain 型と Document IR
  fleximark-parser/      # Markdown parser
  fleximark-transform/   # 組み込み変換
  fleximark-render-html/ # HTML renderer
  fleximark-engine/      # session、cache、patch
  fleximark-protocol/    # JSON-RPC DTO と version
  fleximark-plugin-sdk/  # WIT と guest 型
  fleximark-plugin-host/ # WASM/WASI host
  fleximark-lsp/         # LSP 機能
  fleximarkd/            # native service
  fleximark-cli/         # CLI
adapters/vscode/         # 薄い TypeScript adapter
web/preview-client/      # Webview/Browser 共通 client
schemas/                 # config/protocol/plugin/capability schema
capabilities/            # release baseline の追跡表
fixtures/                # parser/renderer/protocol/security fixtures
```

crate 数は実装時に統合してよいが、依存境界は維持する。

## 4. Document IR、NodeId、ソース位置対応

### Document IR

コアは、パーサーから独立したシリアライズ可能な IR を所有する。文書は schema version、
document version、URI、metadata、block tree を持つ。各描画可能 block は次を持つ。

- エンジンが管理する安定した `NodeId`
- 種別、属性、子 node
- 元文書との対応を表す `SourceProvenance`

パーサーの arena reference やライブラリ固有型は parser crate の外へ出さない。

### NodeId

`NodeId` は patch、双方向 navigation、selection 表示、plugin 診断、cache に使う。
HTML や兄弟 index だけから生成してはならない。構造、意味内容、ソース位置を使って
文書 version 間で一対一に照合する。

- 旧 ID と新 node の対応は双方とも最大一つとする。
- 完全な provenance と意味内容の一致を類似判定より優先する。
- 同点候補が残る場合は誤対応せず、対象へ新しい ID を割り当てる。
- プラグインは既存 ID を読み取り専用で参照する。
- プラグインが作る node は一時的な `CreationKey` を使い、検証後にエンジンが ID を割り当てる。
- 不明・重複 ID、`CreationKey` の再利用、ID の変更はプラグイン出力全体を拒否する。

### SourceProvenance

位置は元 document snapshot 上の 0 始まり半開区間 `[start, end)` とする。
正本は UTF-8 byte offset で、行・文字位置には encoding (`UTF-8`、`UTF-16`、`UTF-32`)
を明記する。LSP 境界では合意した `positionEncoding` と相互変換し、code point や
surrogate pair の途中を指す不正な位置は拒否する。

provenance は次の三種類とする。

- `Original`: 一つ以上の元範囲と primary range を持つ。
- `Derived`: 変換元となった一つ以上の範囲、primary range、transform ID を持つ。
- `Generated`: 元テキストを持たず、必要なら前後関係付き anchor を持つ。

範囲を単一の外接範囲へ潰してはならない。anchor のない生成 node は直接移動できず、
最寄りの対応可能な祖先を使う。見つからなければ editor navigation event を発生させない。

## 5. 解析と変換

初期 parser はソース位置を取得できる、保守が継続中の CommonMark/GFM 実装を使う。
Comrak を第一候補とするが、取得した AST は直ちに FlexiMark IR へ正規化する。

初回仕様には GFM、front matter、数式、admonition、tabs、details、YouTube、Mermaid、
ABC、タイトル・行番号付き code block、security policy に従う raw HTML を含める。

処理順は次のとおり。

```text
元 source snapshot
  → provenance 付き preprocess plugin
  → Markdown parser
  → Document IR
  → 組み込み semantic transform
  → IR plugin
  → 候補の検証、エンジンによる NodeId 割当・照合
  → 診断
  → 構造化 render extension
  → identity 検証
  → renderer
  → target policy と sanitizer
```

preprocess plugin は文字列だけを返してはならない。生成した UTF-8 text と、入力範囲へ戻す
edit map を返す。挿入は `Generated`、置換・結合は全 origin を持つ `Derived` とし、複数の
preprocess を通る場合は edit map を合成する。エンジンは範囲、UTF-8 境界、出力全体の対応を検証し、
不正な map から位置を推測しない。IR transform も分割、結合、移動、生成のたびに provenance を保つ。

組み込み変換は deterministic かつ副作用なしとする。ファイルや network へのアクセスは、
service layer が明示的に与えた capability 経由に限る。

raw HTML は明示設定とし、preview と export で別 policy を持てる。raw HTML の許可は、
network、filesystem、plugin 権限の許可を意味しない。

## 6. 描画とプラグインの安全境界

初期 render target は `HtmlPreview`、`HtmlPortable`、`JsonDebug`、`PlainText` とする。
Mermaid と ABC は typed IR node として保持し、layout、描画、音声再生は Web client が担う。
asset path は service が canonicalize し、許可 root、symlink、local/data/remote URL、
export 名の衝突を検査する。

live preview に任意の HTML 文字列後処理は設けない。preview plugin は typed IR node または
version 付きの構造化 annotation だけを返す。各プラグイン出力は隔離された候補として扱い、
schema、provenance、NodeId を検証する。任意プラグインが失敗した場合は候補を破棄して
直前の正しい値から続行し、診断を出す。必須プラグイン、renderer、sanitizer が失敗した場合は
安全側に停止し、その version の snapshot/patch を公開しない。

sanitizer 後の最終 DOM model についても、NodeId の一意性、既知 node との対応、親子関係、
保護属性、root 数を再検証する。検証済み DOM だけを cache、diff、publish できる。

`unsafe_export_html` は唯一の生 HTML hook とする。次の制約をすべて満たす場合に限り使える。

- `unsafe_html_output` capability を manifest に宣言し、workspace が明示許可している。
- すでに安全に描画・sanitize・serialize された export のコピーへ最後に適用する。
- preview では実行せず、document/session/cache/revision/patch へ結果を戻さない。
- 後段では encoding、size、output file integrity だけを検証し、安全な HTML とは表示しない。

## 7. 文書セッションと差分更新

一つの daemon instance と control connection の中では、同じ URI に一つだけ
authoritative `DocumentSession` を置く。source text、position encoding、document version、
config generation、plugin set、IR、render cache はこの session が所有する。
別の `rpc`/`serve` process は独立した authority であり、ID や patch base を共有しない。

document version は単調増加するが、LSP version は連番でなくてよい。通知順に変更を適用し、
stale version、不正 range、checkpoint hash の不一致を検出したら session を out-of-sync にする。
`fleximark/requestFullText` を送り、full text で復旧するまでは `ContentModified` を返して
新しい render を公開しない。

各 preview は独立した `PreviewSession` と `renderRevision` を持つ。theme、config、plugin、
renderer、asset/security policy は document version を変えずに render を変えられるため、
これらを canonicalize した `rendererFingerprint` を別に持つ。fingerprint が変わる場合は
必ず full snapshot とし、変更をまたぐ patch は生成しない。

patch は最低限、次を含む。

- `previewSessionId`
- `documentVersion`
- `baseRenderRevision` と `resultRenderRevision`
- base/result の `rendererFingerprint`（patch では同値であること）
- `insert`、`remove`、`replace`、`move`、`setAttributes` の操作列。`setAttributes` は
  presentation attribute の allowlist のみを許可し、NodeId、style、event handler、`srcdoc`、URL 属性を拒否する
- target、parent、anchor、必要な precondition

client は live DOM を変更する前に shadow model 上で全操作を順番に検証する。各操作は直前までの
shadow state を参照するため、同じ transaction 内で先に追加した node も後続操作から参照できる。
全操作が成功した場合だけ live DOM へ反映し、途中失敗なら一件も反映しない。revision gap、
fingerprint 不一致、未知 ID、anchor 不在、precondition failure は full snapshot で再同期する。

## 8. サービスとプロトコル

`fleximarkd` は次の mode を持つ。

```text
fleximarkd lsp              # LSP + fleximark/* を同じ接続で処理
fleximarkd rpc              # 独自同期を持つ standalone RPC
fleximarkd serve <document> # standalone preview
```

VS Code adapter は `fleximarkd lsp` を一つだけ起動する。LSP と `fleximark/*` は同じ
`Content-Length` framed JSON-RPC stream、初期化、session registry を共有する。
補助の `rpc` process は起動しない。

LSP は open/change/close、completion、diagnostics、hover、symbols、code actions に使う。
preview 固有機能を標準 LSP message に偽装しない。`didOpen` 後に client は URI、version、
UTF-8 content hash を付けて `fleximark/attachDocument` を呼び、`documentSessionId` を得る。
編集 burst 後と render/export 前には `fleximark/checkpointDocument` で version と hash を照合する。

`fleximark/*` は preview session、snapshot/patch、selection/viewport、theme、export、note、
capability negotiation を扱う。request には daemon instance、document session、期待 version を付ける。

HTTP は preview shell と許可 asset を配信し、WebSocket または SSE は更新と UI event を運ぶだけとする。
文書の authority にはしない。server は loopback と OS 割当 port を既定とし、session token、
Origin/CORS、CSP、allowlist を必須とする。

daemon 再起動時の正本は editor buffer である。adapter は設定と全 open buffer の full text を replay し、
新しい document/preview session と full snapshot を得てから incremental update を再開する。
旧 daemon の ID、token、revision、patch はすべて破棄する。

## 9. プラグインシステム

プラグインはバージョン付き WASM/WASI contract を使い、初期 hook は次とする。

```text
preprocess_source
transform_document
transform_block
extend_render_model
unsafe_export_html
```

manifest は API version、required/optional、workspace read/write、network、environment、
unsafe HTML の capability を宣言する。既定では network と unsafe HTML を許可しない。

host は実行時間、memory、filesystem scope、実行順、API version、出力 schema、cancel を管理する。
各 hook は直前の検証済み値から始まる transaction とし、不正 candidate を部分採用しない。
plugin failure が daemon や active session を破壊してはならない。

## 10. 設定とクリーンブレーク

domain 設定の正本を次へ移す。

```text
.fleximark/
  config.toml
  theme.css
  plugins/
```

VS Code settings に残すのは daemon path、preview pane、auto-open、adapter log など
adapter 固有項目だけとする。
canonical config は notes、assets、security、plugins を持つ。syntax は設定不能な versioned
built-in contract、preview layout は adapter setting、export destination/operation は command parameter
とする。raw HTML は security section で preview/export を別々に設定する。

このリリースには旧設定の移行 command、初回 migrator、互換 reader、旧形式への rollback を設けない。
workspace は新しい layout で直接初期化し、`.fleximark/config.toml`、`.fleximark/theme.css`、
設定済み WASM plugin だけを認識する。

旧 VS Code setting、`.fleximark/fleximark.json`、workspace/global の旧 CSS、
JavaScript parser plugin は unsupported input とする。adapter、service、engine、CLI はそれらを
探索、解析、copy、変換、実行、retire しない。同等の動作が必要な場合は、新しい config と
WASM plugin contract で改めて明示する。これは未リリース置換版の意図的な clean break であり、
互換または data conversion workflow ではない。

## 11. セキュリティ

workspace は client または CLI が明示的に信頼するまで untrusted とする。plugin 実行、raw HTML、
workspace write、remote asset はそれぞれ別に許可する。

### Export の所有権と復旧

非空の出力先を path containment だけで管理対象と判断してはならない。FlexiMark が管理する出力先には
`.fleximark-export.json` と service-owned registry の対応レコードを作る。両者は次を一致させる。

- destination ID
- 単調増加する committed generation
- source/workspace/destination identity
- generated path、type、content hash を含む canonical ownership payload の digest

destination ID だけでは権限の証明にならない。marker と registry の generation/digest が一致しない場合、
通常 export は何も変更せず conflict とする。marker、journal、staging、backup metadata は reserved control
path とし、自己参照を避けるため generated payload manifest には含めず、registry/journal digest で保護する。

既存出力の更新では、前回 manifest にある path だけを削除・上書きできる。ユーザーが追加した通常 file は
保持し、symlink、special file、collision、hash/type/identity の変化は中断理由とする。計画時と commit 直前に
canonical path、containment、symlink/reparse point を再検証し、可能なら no-follow/handle-relative API を使う。

出力は同じ filesystem の sibling staging directory へ作る。staged content と manifest を永続化した後、
`Prepared`、`OldMoved`、`NewInstalled`、`Committed` を記録する durable journal に従って切り替える。
二回の rename を atomic とは呼ばない。registry 更新も同じ transaction に含め、crash 後は destination と
registry を一緒に完了するか、一緒に以前の状態へ戻す。必要な durability primitive がない filesystem では
変更前に失敗させる。

## 12. VS Code と将来のエディタ

VS Code adapter が所有するのは activation、command/UI、Webview、editor event、TextMate grammar、snippet、
daemon lifecycle、DTO 変換、message forwarding だけとする。Markdown parser、HTML generation、diff、
note/domain rule、canonical config 解釈、plugin 実行、二つ目の preview server を置いてはならない。

将来の editor は次の三段階で対応できる。

1. LSP のみ: completion、diagnostics、symbols、code actions
2. LSP + 外部 browser: `fleximarkd serve`
3. rich client: embedded/native preview と selection/viewport 同期

新しい editor のために core へ editor 名条件や parser hook を追加しない。必要な差は protocol version と
capability negotiation で扱う。

## 13. 配布、監視、性能

Windows、macOS、Linux の対応 CPU 向けに native artifact、checksum、release manifest を用意する。
daemon を platform 別 package に同梱するか、署名・checksum を検証して取得するかは、Marketplace size と
supply-chain 要件を満たす方式を選ぶ。

adapter は protocol compatibility、daemon exit、bounded backoff、session 復元、secret を伏せた error、
editor 終了時の child process 停止を扱う。

stdout は framed protocol 専用とし、構造化 log は stderr へ出す。文書内容を既定では記録せず、URI、
document version、parse、transform、render、patch、delivery を trace で関連付ける。

release 前に、cold start、1k/10k/100k 行の初回描画、連続入力、patch size、peak memory、
Mermaid/ABC の再描画頻度、plugin 実行の数値予算を定める。

## 14. 機能の追跡と検証

旧 HTML を正解データにはしないが、ユーザー機能を暗黙に消してよいわけではない。実装開始前に、
baseline tag から machine-readable な capability matrix を作る。command、setting、documented feature、
syntax、永続 file、workflow を安定 ID 単位で一行ずつ登録し、次を持たせる。

- `Keep`、`Change`、`Remove` の判断
- 新仕様の参照先と owner
- fixture/E2E test ID
- `Change`/`Remove` の clean-break 理由と release note ID

重複・欠落 ID、参照切れ、必要 test の失敗を CI で拒否し、product と engineering の双方が承認する。

主要機能の扱いは次のとおり。

| 機能 | 扱い |
| --- | --- |
| VS Code/Browser preview、reload、scroll/selection 同期 | 維持。session/revision protocol を変更 |
| HTML export と asset copy | 維持。所有権と復旧仕様を変更 |
| GFM、拡張構文、Mermaid、ABC、数式、highlight | 維持。新仕様で挙動を定義 |
| note category、template、filename、collect admonitions | 新設定のみで維持 |
| `.fleximark/fleximark.json` | 削除。unsupported であり読み込まない |
| workspace/global の旧 CSS | 削除。新しい workspace `theme.css` を作成する |
| `parserPlugin.js` | 削除。読み込まず、新しい WASM plugin を明示設定する |
| TextMate grammar と snippet | VS Code 固有機能として維持 |

最低限、parser conformance、IR snapshot、source provenance/NodeId property、Unicode、fuzz、
`apply(old, diff) == new`、cancel/stale rejection、protocol version、restart/replay、plugin sandbox、
DOM patch、Mermaid/ABC、VS Code E2E、CLI/platform、install/update を検証する。

security test には path traversal、symlink、CSP/CORS/token、export ownership、TOCTOU、forged marker、
reserved path、registry update 中の crash、journal recovery を含める。旧設定を読み込まないことと
新形式の clean initialization を検証する。

## 15. 実装単位

1. **仕様と契約**: syntax、IR/provenance、NodeId、config/plugin manifest、session/protocol、security/performance budget、capability matrix
2. **Rust engine**: parser、transform、diagnostics、renderer、session/cache/patch、note operation、plugin host/SDK
3. **Service/CLI**: LSP/RPC、preview transport、filesystem、CLI、logging/recovery
4. **Client**: preview client、Mermaid/ABC/math/media、VS Code adapter、UI event mapping
5. **Release**: platform build/sign、manifest、package/download、install/update test、旧 TypeScript runtime の削除

作業は並行化してよいが、仕様と protocol の internal gate を通過してから依存実装を進める。

## 16. リリース条件

次をすべて満たすまで置換版を公開しない。

- Rust core が VS Code、Node.js、browser、network に依存せず build/test できる。
- CLI 単独で render/export できる。
- VS Code adapter に parsing、rendering、diff、note/domain logic がない。
- LSP と `fleximark/*` が同じ daemon/session/version authority を使う。
- unsaved buffer を含む daemon restart/replay が成功する。
- patch が preview session、base/result revision、fingerprint、precondition で保護される。
- source preprocess と全 transform が元 snapshot への provenance を維持する。
- preview plugin が NodeId や最終 sanitizer を迂回できない。
- `parserPlugin.js` の実行経路がなく、設定の正本が `.fleximark/` にある。
- 旧設定、旧 CSS、旧 marker、JavaScript plugin を探索も読み込みもしない。
- 新形式の clean initialization と旧 layout の unsupported 動作を検証する。
- export が registry と generation/digest で所有権を証明し、未管理の非空 directory を変更しない。
- export の TOCTOU、forged marker、crash recovery、journal recovery test が通る。
- capability matrix の全 `Keep`/`Change` が必要 test に結び付いている。
- security/performance budget と全対応 platform の clean install test を満たす。
- 現行 TypeScript conversion/server を削除している。

## 17. 主なリスクと見積り

- 独自構文、source provenance、NodeId 照合が parser の拡張範囲を超える可能性
- ID の不安定さによる大きな patch、ちらつき、scroll 同期の破綻
- 早すぎる plugin ABI 固定
- native binary の署名、配布、更新の複雑さ
- preview asset routing による local file 漏えい
- 一括置換による納期リスクの集中

対策は、syntax と protocol の先行確定、property/edit-sequence/security test、full snapshot fallback、
小さく version 付きの plugin DTO、再現可能な platform build、内部 test gate とする。

Rust core、daemon、LSP、独自 protocol、VS Code adapter、Web preview、export/note、WASM plugin、
cross-platform package、security/E2E を含む現時点の見積りは **26〜36人週**である。
他 editor の native preview API 実装は含まない。

## 18. 最終原則

FlexiMark はエディタ非依存の document engine である。エディタは交換可能な client であり、
HTML は出力形式であって document model ではない。VS Code は最初の adapter にすぎない。
