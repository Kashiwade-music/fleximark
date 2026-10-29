import { BUILTIN_RENDER_KINDS } from "@fleximark/contracts/syntax.generated";

import { renderAbc } from "./abc.mjs";
import { renderMath } from "./math.mjs";
import { renderMermaid } from "./mermaid.mjs";
import { renderTabs } from "./tabs.mjs";
import type { Disposable, Enhancer } from "./types.mjs";
import { renderYouTube } from "./youtube.mjs";

const noResources = (): Disposable => ({ dispose: () => undefined });

export function createEnhancerRegistry(): ReadonlyMap<string, Enhancer> {
  // Keep the user's active panel when a changed tabs block replaces its DOM.
  const activeTabs = new Map<string, string>();
  const enhancers: Enhancer[] = [
    {
      kind: BUILTIN_RENDER_KINDS.MERMAID,
      async enhance(element, context) {
        const dispose = await renderMermaid(
          element,
          context.runtimes.mermaid,
          `fleximark-mermaid-${context.nextId()}`,
          context.current,
        );
        return dispose ? { dispose } : noResources();
      },
    },
    {
      kind: BUILTIN_RENDER_KINDS.ABC,
      enhance: (element, context) =>
        renderAbc(element, context.runtimes.abc, context),
    },
    {
      kind: BUILTIN_RENDER_KINDS.MATH,
      enhance(element, context) {
        renderMath(element, context.runtimes.math);
        return noResources();
      },
    },
    {
      kind: BUILTIN_RENDER_KINDS.YOUTUBE,
      enhance: (element, context) => renderYouTube(element, context.current),
    },
    {
      kind: BUILTIN_RENDER_KINDS.TABS,
      enhance: (element, context) =>
        renderTabs(element, activeTabs, context.nextId),
    },
  ];
  return new Map(enhancers.map((enhancer) => [enhancer.kind, enhancer]));
}
