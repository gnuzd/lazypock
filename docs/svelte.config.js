import adapter from '@sveltejs/adapter-auto';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';
import { mdsvex } from 'mdsvex';
import hljs from 'highlight.js/lib/core';
import bash from 'highlight.js/lib/languages/bash';
import typescript from 'highlight.js/lib/languages/typescript';
import javascript from 'highlight.js/lib/languages/javascript';
import json from 'highlight.js/lib/languages/json';
import yaml from 'highlight.js/lib/languages/yaml';
import elixir from 'highlight.js/lib/languages/elixir';
import plaintext from 'highlight.js/lib/languages/plaintext';

// Same language set as src/lib/components/CodeBlock.svelte.
hljs.registerLanguage('bash', bash);
hljs.registerLanguage('sh', bash);
hljs.registerLanguage('typescript', typescript);
hljs.registerLanguage('ts', typescript);
hljs.registerLanguage('javascript', javascript);
hljs.registerLanguage('js', javascript);
hljs.registerLanguage('json', json);
hljs.registerLanguage('yaml', yaml);
hljs.registerLanguage('yml', yaml);
hljs.registerLanguage('elixir', elixir);
hljs.registerLanguage('plaintext', plaintext);

// The highlighter's return value replaces the whole code node and lands in
// the generated Svelte component's template, so curlies/backticks from the
// original code must be escaped (same escaping mdsvex applies to its built-in
// Prism highlighter) or the Svelte compiler chokes on them.
const escapeSvelty = (str) =>
	str
		.replace(/[{}`]/g, (c) => ({ '{': '&#123;', '}': '&#125;', '`': '&#96;' }[c]))
		.replace(/\\([trn])/g, '&#92;$1');

function highlighter(code, lang) {
	const language = lang && hljs.getLanguage(lang) ? lang : 'plaintext';
	let highlighted;
	try {
		highlighted = hljs.highlight(code, { language }).value;
	} catch {
		highlighted = code;
	}
	// The return value replaces the whole code node, so the <pre>/<code>
	// wrapper must be included (matching mdsvex's built-in highlighter).
	return `<pre class="language-${language}"><code class="language-${language}">${escapeSvelty(highlighted)}</code></pre>`;
}

/** @type {import('@sveltejs/kit').Config} */
const config = {
	extensions: ['.svelte', '.md'],
	preprocess: [
		vitePreprocess(),
		mdsvex({
			extensions: ['.md'],
			highlight: { highlighter }
		})
	],
	kit: {
		adapter: adapter()
	}
};

export default config;
