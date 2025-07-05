const esbuild = require("esbuild");
const fs = require("fs").promises;
const path = require("path");

const production = process.argv.includes('--production');
const watch = process.argv.includes('--watch');

/**
 * @type {import('esbuild').Plugin}
 */
const esbuildProblemMatcherPlugin = {
	name: 'esbuild-problem-matcher',

	setup(build) {
		build.onStart(() => {
			console.log('[watch] build started');
		});
		build.onEnd((result) => {
			result.errors.forEach(({ text, location }) => {
				console.error(`✘ [ERROR] ${text}`);
				console.error(`    ${location.file}:${location.line}:${location.column}:`);
			});
			console.log('[watch] build finished');
		});
	},
};

async function main() {
	// Extension build
	const extensionCtx = await esbuild.context({
		entryPoints: [
			'src/extension.mts'
		],
		bundle: true,
		format: 'cjs',
		minify: production,
		sourcemap: !production,
		sourcesContent: false,
		platform: 'node',
		outfile: 'dist/extension.cjs',
		external: ['vscode'],
		logLevel: 'silent',
		plugins: [
			/* add to the end of plugins array */
			esbuildProblemMatcherPlugin,
		],
		loader: {
			".css": "text",
		}
	});

	// Script for Webview build
	const mediaCtx = await esbuild.context({
		entryPoints: [
			'media/abcjsScripts.mts',
			'media/mermaidScripts.mts',
		],
		bundle: true,
		format: 'iife',
		platform: 'browser',
		minify: production,
		sourcemap: !production,
		outdir: 'dist/media',
		logLevel: 'silent',
	});

	const copyAssets = async () => {
		// dist/media を事前に作成しておく（無くてもOKなように recursive: true）
		const mediaDir = path.join(__dirname, 'dist', 'media');
		await fs.mkdir(mediaDir, { recursive: true });

		// node_modules/abcjs/abcjs-audio.css
		const abcjsAudioCssPath = path.join(__dirname, 'node_modules', 'abcjs', 'abcjs-audio.css');
		const abcjsAudioDestPath = path.join(mediaDir, 'abcjs-audio.css');
		await fs.copyFile(abcjsAudioCssPath, abcjsAudioDestPath);

		// node_modules/katex/dist/katex.min.css
		const katexCssPath = path.join(__dirname, 'node_modules', 'katex', 'dist', 'katex.min.css');
		const katexDestPath = path.join(mediaDir, 'katex.min.css');
		await fs.copyFile(katexCssPath, katexDestPath);

		// node_modules/katex/dist/fonts
		const katexFontsSrcPath = path.join(__dirname, 'node_modules', 'katex', 'dist', 'fonts');
		const katexFontsDestPath = path.join(mediaDir, 'fonts');
		await fs.mkdir(katexFontsDestPath, { recursive: true });
		await fs.cp(katexFontsSrcPath, katexFontsDestPath, { recursive: true });

		console.log('✔ Copied assets to dist/media');
	};

	if (watch) {
		await extensionCtx.watch();
		await mediaCtx.watch();
		await copyAssets();
	} else {
		await extensionCtx.rebuild();
		await extensionCtx.dispose();
		await mediaCtx.rebuild();
		await mediaCtx.dispose();
		await copyAssets();
	}
}

main().catch(e => {
	console.error(e);
	process.exit(1);
});
