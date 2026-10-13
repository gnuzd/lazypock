import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** @type {import('@sveltejs/kit').Config} */
const config = {
	preprocess: vitePreprocess(),

	compilerOptions: {
		// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
		runes: ({ filename }) => (filename.split(/[/\\]/).includes('node_modules') ? undefined : true)
	},

	kit: {
		// adapter-auto only supports some environments, see https://svelte.dev/docs/kit/adapter-auto
		// for a list. If your environment is not supported, or you settled on a specific
		// environment, switch out the adapter.
		// See https://svelte.dev/docs/kit/adapters for more information about adapters.
		adapter: adapter({
			// Build inside the Studio's own tree rather than the deleted Elixir
			// layout. When `crates/studio-embed` lands (PLAN.md track J) this moves
			// to its `dist/`, which is what gets embedded in the binary.
			pages: 'build/studio',
			assets: 'build/studio',
			fallback: 'index.html',
			precompress: false,
			strict: true
		}),

		paths: {
			base: '/_'
		}
	}
};

export default config;
