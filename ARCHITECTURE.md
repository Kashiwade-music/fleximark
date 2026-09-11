# FlexiMark Architecture

この文書は将来像ではなく、現在の実装と、次の安定版で採用する設計判断を記録する。
コードと自動テストに反する記述は完成条件の証拠として扱わない。

## 1. 現在の到達点

FlexiMark は、Rust 製の document engine と daemon、薄い VS Code adapter、ブラウザでのみ動く
preview client から構成される。旧 TypeScript parser/server と JavaScript plugin runtime は廃止済みである。

```text
VS Code adapter ── JSON-RPC/LSP ── fleximarkd
                                      │
                                      ├─ document session / IR / NodeId
                                      ├─ parser / renderer / export / notes
                                      └─ WASM plugin host

preview client  ◀── snapshot/patch ──┘
```

実装済みの中核は次のとおり。

- FlexiMark-owned IR、source provenance、UTF-8/16/32 position
- Markdown parse、NodeId reconciliation、HTML snapshot/patch
- 同一 daemon 上の LSP と `fleximark/*` request
- VS Code preview、browser preview、source navigation
- Mermaid、ABC、KaTeX、YouTube consent、tabs、code highlighting
- deny-by-default の WASM component plugin host
- workspace config/theme、notes、transactional HTML export
- platform daemon の build/package/VSIX 検査

## 2. 正本と境界

正本の優先順位を次のようにする。

1. protocol/schema と公開 DTO
2. Rust/TypeScript の実装
3. executable test と計測結果
4. この文書

`capabilities/release-baseline.yaml` は機能インベントリであり、テスト名が存在するだけでは完成を意味しない。
release gate に使う証拠は、実行したコマンド、対象 platform、結果を機械的に検証できるものに限定する。

## 3. 採用する責務分割

### Rust model/parser/engine

- model は editor、filesystem、HTML DOM に依存しない。
- parser は Markdown backend の AST を公開せず、直ちに FlexiMark IR に変換する。
- engine は document version、NodeId、render cache、preview revision の唯一の所有者になる。
- editor adapter は parse、render、diff を行わない。

### daemon

- 一つの daemon が複数 workspace を分離して扱う。
- LSP と独自 protocol は同じ document session を参照する。
- workspace trust、plugin grant、filesystem operation は daemon 側で強制する。
- 長時間処理は request loop から分離し、document generation 単位で cancel できるようにする。

### preview client

- daemon が生成する HTML は raw input を実行可能な HTML として通過させない。
- client は detached DOM で identity、許可属性、URL、inert payload を検査してから atomic に反映する。
- Mermaid、ABC、KaTeX、YouTube は型付きの inert payload からのみ起動する。
- v1 の raw HTML policy は `escape` と `reject` のみとし、任意 HTML の allow/sanitize mode は提供しない。

これは「daemon の typed escaping」と「適用直前の client DOM validation」による二重境界である。
daemon 内にブラウザ同等の DOM sanitizer を持つことは v1 の要件にしない。

### plugin host

- plugin は署名済み WASM component のみとする。
- filesystem/network/process/environment は既定で与えない。
- v1 では network capability を提供しない。manifest/config から未対応能力を明確に拒否する。
- plugin failure、timeout、cancel は candidate 全体を破棄し、authoritative session を変更しない。

## 4. Document engine の再設計

現在の `change_full_text` は更新ごとに全文 parse、全文 IR reconciliation、全文 render を行う。
当初の大文書 timeout は、source position の検証が各行ごとに文書先頭から走査していたことと、
block diff の同位置IDにも線形探索を使っていたことが主因だった。共有 line index と同位置 fast path により、
Windows release build の既存性能budgetは通過している。

今後、実測で必要になった場合だけ次の状態モデルへ段階的に移行する。

```text
DocumentState
├─ SourceBuffer + LineIndex
├─ ParsedRegions
├─ Document IR
├─ NodeIdentityIndex
└─ RenderCache
   └─ (NodeId, content hash, render context fingerprint) -> RenderedBlock
```

変更処理は次の順序にする。

1. editor change range を `SourceBuffer` に適用する。
2. front matter、fence、directive、list 等を考慮して再解析範囲を安全側に拡張する。
3. 対象範囲だけ parse し、IR を splice する。
4. 変更範囲と境界ノードだけ NodeId reconciliation を行う。
5. content hash が変わった block だけ render する。
6. ID→位置 index を使って patch を生成する。
7. 安全な範囲を確定できない場合だけ全文 fallback し、その理由を計測する。

最初の改善として、source line index の共有と、同位置の NodeId を O(1) で判定する fast path を実装した。
次の候補は render cache、ID位置index、region parse だが、3 platform の p95 または実利用traceが必要性を
示した場合だけ着手する。改善が確認できない抽象化は追加しない。

## 5. 完成対象

次の安定版で必須とする user flow は以下に限定する。

- Markdown を開き、VS Code preview または browser preview を開始できる。
- unsaved edit、selection、viewport が preview と同期する。
- daemon crash 後に open buffer と preview を復旧できる。
- completion、hover、symbols、diagnostics、code action が同じ session を使う。
- workspace theme、note creation、admonition collection が動作する。
- trusted workspace で安全に HTML export でき、失敗後に復旧できる。
- signed WASM plugin を capability 制限下で実行できる。
- Windows、Linux、macOS の配布 artifact を検証できる。

`JsonDebug`、`PlainText` renderer、任意 raw HTML、plugin network access、Zed 等の追加 adapter は
この安定版の完成条件に含めない。必要になった時点で個別の feature contract として追加する。

## 6. Release gate

release candidate は次を満たす必要がある。

- formatting、Clippy、既存のRust/TypeScript/VS Code testが成功する。
- release build、manifest、VSIX contents検査が成功する。
- 対応platformごとに一つのclean-install smokeが成功する。
- 性能benchmarkがCIの基準環境でtimeoutせず、budgetを満たす。
- preview、navigation、note、exportの手動release checkでblockerがない。

性能budgetは希望値から決めず、CIの基準環境で再現できる代表値から回帰閾値を決める。

テストは実装量に比例して増やさない。不具合の再発、公開contract、重大なsecurity境界のいずれも
守らないテストは追加しない。同じ要件を複数層で重複検査せず、既存suiteへの最小追加を優先する。

## 7. 直近の実装順

1. request scheduling と cancellation
2. daemon crash replay と observability
3. 実workspaceで主要user flowを一巡してblockerを修正
4. arm64を含む配布範囲の決定とclean-install確認

変更には、再発リスクがある場合だけ最小限のテストを含める。
