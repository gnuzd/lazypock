#!/usr/bin/env node
// Syncs per-SDK markdown docs from their repos into the docs site.
//
// Source of truth for SDK docs lives in each SDK repo (e.g. gnuzd/lazypock-ts
// -> docs/*.md). This script merges those files into ONE scrollable page per
// SDK (src/routes/sdk/<slug>/+page.md) — each file becomes an <h2> section
// with an id anchor, mirroring the Server Guide's in-page scrolling — and
// regenerates src/lib/sdk-nav.generated.ts from the rendered sections.
//
// Resolution order for an SDK's source:
//   1. $LAZYPOCK_TS_DIR (env field per SDK in the registry) — local clone override
//   2. a cached git clone in .sdk-cache/<slug>
//   3. (fallback) the already-committed merged +page.md — nav is re-derived
//      from its <h2 id> sections, so the site always builds offline
//
// Usage: node scripts/sync-sdks.mjs   (from docs/)
import { execSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url))); // docs/
const routesDir = path.join(root, 'src', 'routes', 'sdk');
const libDir = path.join(root, 'src', 'lib');
const cacheDir = path.join(root, '.sdk-cache');

/** Registry: every SDK the docs site documents. Add lazypock-swift here. */
const SDKs = [
	{
		repo: 'gnuzd/lazypock-ts',
		slug: 'typescript',
		name: 'TypeScript',
		docsDir: 'docs',
		env: 'LAZYPOCK_TS_DIR',
		// Section order; pages present in the repo but missing here get appended.
		order: [
			'install',
			'quick-start',
			'type-safety',
			'queries',
			'realtime',
			'files',
			'auth',
			'api-reference'
		],
		intro:
			'The official TypeScript client for Lazypock — fully typed via codegen, ' +
			'PocketBase-compatible API surface, and runtime schema support.'
	}
];

function git(args, cwd) {
	try {
		execSync(`git ${args}`, { stdio: 'inherit', cwd });
		return true;
	} catch (err) {
		console.warn(`  [!] git ${args} failed (${err?.message ?? err})`);
		return false;
	}
}

function resolveSource(sdk) {
	if (sdk.env && process.env[sdk.env] && existsSync(process.env[sdk.env])) {
		return { dir: process.env[sdk.env], from: `env override (${sdk.env})` };
	}
	const dir = path.join(cacheDir, sdk.slug);
	mkdirSync(cacheDir, { recursive: true });
	if (existsSync(path.join(dir, '.git'))) {
		console.log(`  refreshing cached clone ${dir}`);
		if (git(`fetch origin --depth 1`, dir)) {
			git(`reset --hard origin/main`, dir);
		}
	} else {
		console.log(`  cloning https://github.com/${sdk.repo} -> ${dir}`);
		git(`clone --depth 1 https://github.com/${sdk.repo}.git "${dir}"`, cacheDir);
	}
	return { dir, from: 'cached clone' };
}

function parseFrontmatter(md) {
	const m = md.match(/^---\r?\n([\s\S]*?)\r?\n---/);
	if (!m) return { title: null, body: md };
	const title = m[1].match(/^title:\s*["']?(.+?)["']?\s*$/m)?.[1]?.trim() ?? null;
	return { title, body: md.slice(m[0].length) };
}

function stripLeadingH1(body) {
	// Remove the file's own `# Title` (allow leading blank lines after the
	// frontmatter slice) — the section header is emitted separately.
	return body.replace(/^\s*#\s+.+\r?\n\r?\n?/, '');
}

function demoteHeadings(body) {
	// Demote every heading one level (## -> ###) so per-file subsections nest
	// under the merged section headers. Fenced code blocks are skipped — a
	// line starting with `# ` inside a bash block is not a heading.
	let inFence = false;
	return body
		.split('\n')
		.map((line) => {
			const trimmed = line.trim();
			if (trimmed.startsWith('```')) inFence = !inFence;
			if (inFence) return line;
			return line.replace(/^(#{1,5}) /, (_, hashes) => '#' + hashes + ' ');
		})
		.join('\n');
}

function syncSdk(sdk) {
	const targetDir = path.join(routesDir, sdk.slug);
	mkdirSync(targetDir, { recursive: true });

	const { dir, from } = resolveSource(sdk);
	const srcDir = path.join(dir, sdk.docsDir);
	if (!existsSync(srcDir)) {
		console.log(`  [!] ${sdk.name}: no ${sdk.docsDir}/ in ${from} — keeping committed page`);
		return;
	}
	const available = readdirSync(srcDir)
		.filter((f) => f.endsWith('.md'))
		.map((f) => f.replace(/\.md$/, ''));
	const slugs = [
		...sdk.order.filter((slug) => available.includes(slug)),
		...available.filter((slug) => !sdk.order.includes(slug)).sort()
	];
	if (slugs.length === 0) {
		console.log(`  [!] ${sdk.name}: no markdown pages in ${srcDir} — keeping committed page`);
		return;
	}

	const sections = [];
	for (const slug of slugs) {
		const md = readFileSync(path.join(srcDir, `${slug}.md`), 'utf8');
		const { title, body } = parseFrontmatter(md);
		const content = demoteHeadings(stripLeadingH1(body)).trim();
		sections.push(`<h2 id="${slug}">${title ?? slug}</h2>\n\n${content}`);
	}

	const page = `---\ntitle: ${sdk.name} SDK\n---\n\n# ${sdk.name} SDK\n\n${
		sdk.intro ?? ''
	}\n\n${sections.join('\n\n---\n\n')}\n`;

	// The merged +page.md is the only file we keep in the SDK's route dir.
	for (const entry of readdirSync(targetDir)) {
		rmSync(path.join(targetDir, entry), { recursive: true, force: true });
	}
	writeFileSync(path.join(targetDir, '+page.md'), page);
	console.log(`  ✓ ${sdk.name}: merged ${slugs.length} sections from ${from}`);
}

// Regenerate sdk-nav.generated.ts from the <h2 id> sections of each merged page.
function generateNav() {
	const entries = SDKs.map((sdk) => {
		const mdPath = path.join(routesDir, sdk.slug, '+page.md');
		const pages = [];
		if (existsSync(mdPath)) {
			const md = readFileSync(mdPath, 'utf8');
			for (const m of md.matchAll(/<h2 id="([^"]+)">([^<]+)<\/h2>/g)) {
				pages.push({ slug: m[1], label: m[2].trim() });
			}
		}
		return { slug: sdk.slug, name: sdk.name, pages };
	});
	const ts = `// AUTO-GENERATED by scripts/sync-sdks.mjs — do not edit.
// Regenerated on every build; reflects the <h2> sections of each SDK page.

export interface SdkPage {
	slug: string;
	label: string;
}

export interface SdkNavEntry {
	slug: string;
	name: string;
	pages: SdkPage[];
}

export const sdkNav: SdkNavEntry[] = ${JSON.stringify(entries, null, '\t')};
`;
	writeFileSync(path.join(libDir, 'sdk-nav.generated.ts'), ts);
	const total = entries.reduce((n, e) => n + e.pages.length, 0);
	console.log(`nav: ${entries.length} SDK(s), ${total} sections`);
}

console.log('Syncing SDK docs…');
for (const sdk of SDKs) syncSdk(sdk);
generateNav();
console.log('Done.');
