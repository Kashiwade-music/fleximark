/* eslint-disable @typescript-eslint/no-explicit-any */
import { performance } from "perf_hooks";

function withTiming(name: string, plugin: any) {
  return function timedAttacher(this: any, ...optionsFromUse: any[]) {
    // （任意）セットアップ時間を取りたい場合は以下を有効化
    // const setupStart = performance.now();

    const realAttacher =
      typeof plugin === "function"
        ? plugin.apply(this, optionsFromUse)
        : plugin;

    // const setupEnd = performance.now();
    // console.log(`[md->hast] ${name} setup: ${(setupEnd - setupStart).toFixed(2)}ms`);

    if (typeof realAttacher !== "function") {
      // 一部プラグインは transformer を返さないことがある
      return realAttacher;
    }

    // eslint-disable-next-line @typescript-eslint/no-this-alias
    const myself = this;
    return async function timedTransformer(tree: any, file: any) {
      const start = performance.now();
      const out = realAttacher.call(myself, tree, file);
      if (out && typeof (out as any).then === "function") {
        const res = await out;
        const end = performance.now();
        console.log(`[md->hast] ${name}: ${(end - start).toFixed(2)}ms`);
        return res;
      } else {
        const end = performance.now();
        console.log(`[md->hast] ${name}: ${(end - start).toFixed(2)}ms`);
        return out;
      }
    };
  };
}

export default withTiming;

/*

# How to use this plugin

async function toMdastFromMarkdown(args: ConvertArgs): Promise<MdastRoot> {
  const processor = unified()
    .use(remarkParse)
    .use(withTiming("remarkFrontmatter", remarkFrontmatter))
    .use(withTiming("remarkYouTube", remarkYouTube), {
      mode: args.convertType === "webview" ? "lazy" : "iframe",
    })
    .use(withTiming("remarkGfm", remarkGfm))
    .use(withTiming("remarkMath", remarkMath))
    .use(withTiming("remarkDirective", remarkDirective))
    .use(withTiming("remarkDirectiveAdmonitions", remarkDirectiveAdmonitions))
    .use(withTiming("remarkDirectiveDetails", remarkDirectiveDetails))
    .use(withTiming("remarkDirectiveTabs", remarkDirectiveTabs));

  const parsed = processor.parse(args.markdown);
  return processor.run(parsed) as Promise<MdastRoot>;
}

*/
