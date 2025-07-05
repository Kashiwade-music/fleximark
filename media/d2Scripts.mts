import { D2 } from "@terrastruct/d2";

let isRenderedD2 = false;

window.addEventListener("load", () => {
  console.log("ページが完全に読み込まれました");
  renderD2();
});

async function renderD2(): Promise<void> {
  if (isRenderedD2) return;
  isRenderedD2 = true;

  const figures = document.querySelectorAll(
    "figure[data-rehype-pretty-code-figure]"
  );

  const d2 = new D2();

  for (const figure of figures) {
    const pre = figure.querySelector('pre[data-language="d2"]');
    const code = pre?.querySelector("code");

    if (!pre || !code) continue;

    // D2コードを構築
    const lines = Array.from(code.querySelectorAll("span[data-line]"))
      .map((span) => span.textContent ?? "")
      .join("\n");

    try {
      const result = await d2.compile(lines, {
        sketch: false,
      });

      const svg = await d2.render(result.diagram, result.renderOptions);

      // 元の pre 要素を削除
      pre.remove();

      // SVG を挿入
      const container = document.createElement("div");
      container.innerHTML = svg;
      figure.appendChild(container);
    } catch (error) {
      console.error("D2の変換に失敗しました", error);
    }
  }
}
