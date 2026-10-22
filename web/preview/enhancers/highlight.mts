export function highlightCode(root: HTMLElement): void {
  const languages = new Set([
    "javascript",
    "js",
    "typescript",
    "ts",
    "json",
    "rust",
    "bash",
    "sh",
  ]);
  for (const code of root.querySelectorAll<HTMLElement>(
    "pre > code[class*='language-']",
  )) {
    const language = [...code.classList]
      .find((name) => name.startsWith("language-"))
      ?.slice("language-".length)
      .toLowerCase();
    if (!language || !languages.has(language)) {
      delete code.dataset.fleximarkHighlighted;
      continue;
    }
    if (code.dataset.fleximarkHighlighted === language) continue;
    const pattern =
      /(\/\/[^\n]*|#[^\n]*|\/\*[\s\S]*?\*\/|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|\b(?:const|let|var|function|return|if|else|for|while|struct|enum|impl|fn|pub|use|match|true|false|null|undefined)\b|\b\d+(?:\.\d+)?\b)/g;
    const lines = [
      ...code.querySelectorAll<HTMLElement>(":scope > .fleximark-code-line"),
    ];
    for (const target of lines.length ? lines : [code]) {
      const source = target.textContent ?? "";
      const fragment = document.createDocumentFragment();
      let offset = 0;
      for (const match of source.matchAll(pattern)) {
        fragment.append(source.slice(offset, match.index));
        const token = document.createElement("span");
        token.className = /^\d/.test(match[0])
          ? "fleximark-token-number"
          : /^(?:\/\/|\/\*|#)/.test(match[0])
            ? "fleximark-token-comment"
            : /^["']/.test(match[0])
              ? "fleximark-token-string"
              : "fleximark-token-keyword";
        token.textContent = match[0];
        fragment.append(token);
        offset = (match.index ?? 0) + match[0].length;
      }
      fragment.append(source.slice(offset));
      target.replaceChildren(fragment);
    }
    code.dataset.fleximarkHighlighted = language;
  }
}
