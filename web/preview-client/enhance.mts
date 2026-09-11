export interface PreviewRuntimes {
  mermaid: {
    initialize(options: {
      startOnLoad: boolean;
      securityLevel: "strict";
    }): void;
    render(id: string, source: string): Promise<{ svg: string }>;
  };
  abc: {
    render(target: HTMLElement, source: string): unknown[];
    supportsAudio(): boolean;
    createSynth(): {
      init(options: { visualObj: unknown }): Promise<unknown>;
      prime(): Promise<unknown>;
      start(): void;
      stop(): unknown;
    };
  };
  math: {
    render(
      source: string,
      target: HTMLElement,
      options: {
        displayMode: boolean;
        output: "mathml";
        strict: "error";
        throwOnError: boolean;
        trust: false;
      },
    ): void;
  };
}

export class PreviewEnhancer {
  readonly #runtimes: PreviewRuntimes;
  readonly #audio = new Set<
    ReturnType<PreviewRuntimes["abc"]["createSynth"]>
  >();
  #renderId = 0;
  #generation = 0;
  readonly #blockFingerprints = new Map<string, string>();
  readonly #activeTabs = new Map<string, string>();

  constructor(runtimes: PreviewRuntimes) {
    this.#runtimes = runtimes;
    runtimes.mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
    });
  }

  async render(root: HTMLElement): Promise<void> {
    const generation = ++this.#generation;
    for (const synth of this.#audio) synth.stop();
    this.#audio.clear();
    const blocks = root.hasAttribute("data-fleximark-kind") ? [root] : [];
    blocks.push(...root.querySelectorAll<HTMLElement>("[data-fleximark-kind]"));
    await Promise.all(
      blocks.map(async (block) => {
        const id = block.dataset.fleximarkNodeId;
        const fingerprint = this.#blockFingerprint(block);
        if (
          id &&
          this.#blockFingerprints.get(id) === fingerprint &&
          block.dataset.fleximarkKind !== "tabs" &&
          block.dataset.fleximarkKind !== "abc" &&
          !(
            block.dataset.fleximarkKind === "youtube" &&
            !block.querySelector("iframe")
          )
        )
          return;
        try {
          switch (block.dataset.fleximarkKind) {
            case "mermaid":
              await this.#renderMermaid(block, generation);
              break;
            case "abc":
              this.#renderAbc(block, generation);
              break;
            case "math":
              this.#renderMath(block);
              break;
            case "youtube":
              this.#renderYouTube(block, generation);
              break;
            case "tabs":
              this.#renderTabs(block);
              break;
          }
          delete block.dataset.fleximarkRenderError;
          if (id) this.#blockFingerprints.set(id, fingerprint);
        } catch (error) {
          block.dataset.fleximarkRenderError =
            error instanceof Error ? error.message : String(error);
        }
      }),
    );
    this.#highlightCode(root);
  }

  dispose(): void {
    this.#generation++;
    for (const synth of this.#audio) synth.stop();
    this.#audio.clear();
    this.#blockFingerprints.clear();
    this.#activeTabs.clear();
  }

  async #renderMermaid(block: HTMLElement, generation: number): Promise<void> {
    const source = this.#payload(block);
    const output = this.#output(block);
    const { svg } = await this.#runtimes.mermaid.render(
      `fleximark-mermaid-${++this.#renderId}`,
      source,
    );
    if (generation === this.#generation && output.isConnected)
      output.innerHTML = svg;
  }

  #renderAbc(block: HTMLElement, generation: number): void {
    const output = this.#output(block);
    const visual = this.#runtimes.abc.render(output, this.#payload(block))[0];
    if (!visual || !this.#runtimes.abc.supportsAudio()) return;
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = "Play";
    button.dataset.fleximarkAudio = "play";
    button.addEventListener("click", async () => {
      if (generation !== this.#generation) return;
      button.disabled = true;
      let synth: ReturnType<PreviewRuntimes["abc"]["createSynth"]> | undefined;
      try {
        synth = this.#runtimes.abc.createSynth();
        this.#audio.add(synth);
        await synth.init({ visualObj: visual });
        await synth.prime();
        if (generation !== this.#generation) {
          synth.stop();
          this.#audio.delete(synth);
          return;
        }
        synth.start();
      } catch (error) {
        block.dataset.fleximarkRenderError =
          error instanceof Error ? error.message : String(error);
      } finally {
        button.disabled = false;
      }
    });
    block.append(button);
  }

  #renderMath(block: HTMLElement): void {
    let source: string;
    const payload = [...block.children].find(
      (child) =>
        child instanceof HTMLScriptElement &&
        child.dataset.fleximarkMathSource === "true",
    );
    if (payload) {
      source = JSON.parse(payload.textContent ?? "");
    } else {
      source = block.textContent ?? "";
      const script = document.createElement("script");
      script.type = "application/json";
      script.dataset.fleximarkMathSource = "true";
      script.textContent = JSON.stringify(source);
      block.replaceChildren(script);
    }
    this.#runtimes.math.render(source, this.#output(block), {
      displayMode: block.tagName === "DIV",
      output: "mathml",
      strict: "error",
      throwOnError: false,
      trust: false,
    });
  }

  #renderYouTube(block: HTMLElement, generation: number): void {
    const source = block.dataset.source;
    if (!source) throw new Error("missing YouTube source");
    const url = new URL(source);
    if (url.protocol !== "https:") throw new Error("invalid YouTube URL");
    const id =
      url.hostname === "youtu.be" && /^\/[A-Za-z0-9_-]{11}$/.test(url.pathname)
        ? url.pathname.slice(1)
        : (url.hostname === "youtube.com" ||
              url.hostname === "www.youtube.com") &&
            url.pathname === "/watch"
          ? url.searchParams.get("v")
          : undefined;
    if (!id || !/^[A-Za-z0-9_-]{11}$/.test(id))
      throw new Error("invalid YouTube video id");
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = "Load YouTube video";
    button.dataset.fleximarkYoutubeConsent = "true";
    button.addEventListener("click", () => {
      if (generation !== this.#generation || !button.isConnected) return;
      const frame = document.createElement("iframe");
      frame.src = `https://www.youtube-nocookie.com/embed/${id}`;
      frame.title = "YouTube video";
      frame.loading = "lazy";
      frame.allowFullscreen = true;
      frame.setAttribute(
        "sandbox",
        "allow-scripts allow-same-origin allow-presentation",
      );
      button.replaceWith(frame);
    });
    block.replaceChildren(button);
  }

  #renderTabs(block: HTMLElement): void {
    const panels = [...block.children].filter(
      (child): child is HTMLElement =>
        child instanceof HTMLElement && child.dataset.fleximarkKind === "tab",
    );
    if (!panels.length) throw new Error("tabs requires at least one tab");
    block.querySelector(":scope > [role='tablist']")?.remove();
    const tablist = document.createElement("div");
    tablist.setAttribute("role", "tablist");
    const containerId = block.dataset.fleximarkNodeId ?? "";
    const buttons = panels.map((panel, index) => {
      const button = document.createElement("button");
      const suffix = `${++this.#renderId}-${index}`;
      button.type = "button";
      button.id = `fleximark-tab-${suffix}`;
      button.textContent = panel.dataset.tabLabel ?? `Tab ${index + 1}`;
      button.setAttribute("role", "tab");
      button.setAttribute("aria-controls", `fleximark-panel-${suffix}`);
      panel.id = `fleximark-panel-${suffix}`;
      panel.setAttribute("role", "tabpanel");
      panel.setAttribute("aria-labelledby", button.id);
      const activate = () => {
        for (let position = 0; position < panels.length; position += 1) {
          const active = position === index;
          panels[position].hidden = !active;
          buttons[position].setAttribute("aria-selected", String(active));
          buttons[position].tabIndex = active ? 0 : -1;
        }
        const panelId = panel.dataset.fleximarkNodeId;
        if (containerId && panelId) this.#activeTabs.set(containerId, panelId);
      };
      button.addEventListener("click", activate);
      button.addEventListener("keydown", (event) => {
        const next =
          event.key === "ArrowRight"
            ? (index + 1) % panels.length
            : event.key === "ArrowLeft"
              ? (index + panels.length - 1) % panels.length
              : event.key === "Home"
                ? 0
                : event.key === "End"
                  ? panels.length - 1
                  : undefined;
        if (next === undefined) return;
        event.preventDefault();
        buttons[next].click();
        buttons[next].focus();
      });
      tablist.append(button);
      return button;
    });
    block.prepend(tablist);
    const active = this.#activeTabs.get(containerId);
    buttons[
      Math.max(
        0,
        panels.findIndex((panel) => panel.dataset.fleximarkNodeId === active),
      )
    ].click();
  }

  #blockFingerprint(block: HTMLElement): string {
    const kind = block.dataset.fleximarkKind;
    if (kind === "tabs")
      return JSON.stringify(
        [...block.children]
          .filter(
            (child): child is HTMLElement =>
              child instanceof HTMLElement &&
              child.dataset.fleximarkKind === "tab",
          )
          .map((panel) => [
            panel.dataset.fleximarkNodeId,
            panel.dataset.tabLabel,
            panel.textContent,
          ]),
      );
    if (kind === "youtube") return block.dataset.source ?? "";
    if (kind === "mermaid" || kind === "abc") return this.#payload(block);
    if (kind === "math") {
      const payload = [...block.children].find(
        (child) =>
          child instanceof HTMLScriptElement &&
          child.dataset.fleximarkMathSource === "true",
      );
      if (!payload) return block.textContent ?? "";
      const source: unknown = JSON.parse(payload.textContent ?? "null");
      return typeof source === "string" ? source : "";
    }
    return block.textContent ?? "";
  }

  #highlightCode(root: HTMLElement): void {
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

  #payload(block: HTMLElement): string {
    const script = [...block.children].find(
      (child) =>
        child instanceof HTMLScriptElement && child.type === "application/json",
    );
    const value = JSON.parse(script?.textContent ?? "null");
    if (typeof value !== "string") throw new Error("invalid preview payload");
    return value;
  }

  #output(block: HTMLElement): HTMLElement {
    block.querySelector(":scope > [data-fleximark-output]")?.remove();
    block.querySelector(":scope > [data-fleximark-audio]")?.remove();
    const output = document.createElement("div");
    output.dataset.fleximarkOutput = "true";
    block.append(output);
    return output;
  }
}
