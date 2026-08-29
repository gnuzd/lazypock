<script lang="ts">
	import hljs from 'highlight.js/lib/core';
	import bash from 'highlight.js/lib/languages/bash';
	import typescript from 'highlight.js/lib/languages/typescript';
	import javascript from 'highlight.js/lib/languages/javascript';
	import json from 'highlight.js/lib/languages/json';
	import yaml from 'highlight.js/lib/languages/yaml';
	import elixir from 'highlight.js/lib/languages/elixir';
	import plaintext from 'highlight.js/lib/languages/plaintext';

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

	let { code, lang = 'plaintext' }: { code: string; lang?: string } = $props();

	const highlighted = $derived.by(() => {
		try {
			if (lang && hljs.getLanguage(lang)) {
				return hljs.highlight(code, { language: lang }).value;
			}
			return hljs.highlightAuto(code).value;
		} catch {
			return code;
		}
	});
</script>

<pre
	class="doc-code hljs border border-base-300 my-4 p-4 text-[13px] leading-relaxed"><code class="hljs" data-lang={lang}>{@html highlighted}</code></pre>
