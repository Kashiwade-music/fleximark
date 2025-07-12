import mermaid from "mermaid";

window.addEventListener("load", () => {
  renderMermaid();
});

window.renderMermaid = renderMermaid;

function renderMermaid(): void {
  const figures = document.querySelectorAll(
    "figure[data-rehype-pretty-code-figure]"
  );

  figures.forEach((figure) => {
    const pre = figure.querySelector('pre[data-language="mermaid"]');
    if (!pre) return;

    const code = pre.querySelector('code[data-language="mermaid"]');
    if (!code) return;

    // mermaidコードを抽出
    const lines = Array.from(code.querySelectorAll("span[data-line]")).map(
      (span) => span.textContent || ""
    );
    const mermaidCode = lines.join("\n");

    // mermaid用のdivを作成
    const mermaidDiv = document.createElement("div");
    mermaidDiv.className = "mermaid";
    mermaidDiv.textContent = mermaidCode;

    // 既存の<pre>を置き換え
    figure.innerHTML = ""; // <figure>内部を全削除
    figure.appendChild(mermaidDiv);
  });

  // mermaid描画
  mermaid.initialize({ startOnLoad: false });
  mermaid.run();
}
