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

async function copyAllCssFiles(srcDir, destDir) {
	const entries = await fs.readdir(srcDir, { withFileTypes: true });

	for (const entry of entries) {
		const srcPath = path.join(srcDir, entry.name);
		const destPath = path.join(destDir, entry.name);

		if (entry.isDirectory()) {
			await copyAllCssFiles(srcPath, destPath);
		} else if (entry.isFile() && entry.name.endsWith('.css')) {
			await fs.mkdir(path.dirname(destPath), { recursive: true });
			await fs.copyFile(srcPath, destPath);
			console.log(`✔ Copied: ${srcPath} → ${destPath}`);
		}
	}
}

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
	const mediaEntryPoints = [
		'media/abcjsScripts.mts',
		'media/d2Scripts.mts',
		'media/mermaidScripts.mts',
	];

	const mediaCtx = await esbuild.context({
		entryPoints: mediaEntryPoints,
		bundle: true,
		format: 'iife',
		platform: 'browser',
		minify: production,
		sourcemap: !production,
		outdir: 'dist/media',
		logLevel: 'silent',
	});

	const copyAssets = async () => {
		await copyAllCssFiles('media', 'dist/media');
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
	}
}

main().catch(e => {
	console.error(e);
	process.exit(1);
});
