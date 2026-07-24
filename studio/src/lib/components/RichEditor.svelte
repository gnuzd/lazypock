<script lang="ts">
	import { onMount } from 'svelte';

	let {
		name = '',
		value = $bindable(),
		disabled = false,
	}: {
		name?: string;
		value?: unknown;
		disabled?: boolean;
	} = $props();

	let el = $state<HTMLTextAreaElement>();
	let loaded = $state(false);
	let id = $derived('editor_' + name);

	onMount(async () => {
		// Load TinyMCE from CDN (match PocketBase v6.8.6)
		if (!window.tinymce) {
			await new Promise<void>((resolve, reject) => {
				const script = document.createElement('script');
				script.src = 'https://cdn.jsdelivr.net/npm/tinymce@6.8.6/tinymce.min.js';
				script.onload = () => resolve();
				script.onerror = () => reject(new Error('Failed to load TinyMCE'));
				document.head.appendChild(script);
			});
		}

		loaded = true;

		// Wait for textarea to be in DOM
		await new Promise(requestAnimationFrame);

		if (!el) return;

		window.tinymce.init({
			target: el,
			menubar: false,
			statusbar: false,
			plugins: [
				'autolink', 'autoresize', 'code', 'link', 'lists', 'table', 'wordcount',
			],
			toolbar:
				'bold italic underline strikethrough | ' +
				'bullist numlist | ' +
				'blockquote code | ' +
				'link table | ' +
				'removeformat',
			branding: false,
			promotion: false,
			elementpath: false,
			convert_urls: false,
			relative_urls: false,
			remove_script_host: false,
			autoresize_bottom_margin: 0,
			min_height: 150,
			max_height: 400,
			// eslint-disable-next-line @typescript-eslint/no-explicit-any
			setup: (ed: any) => {
				if (value) ed.setContent(String(value));
				ed.on('change', () => {
					value = ed.getContent();
				});
			},
		});
	});
</script>

{#if loaded}
	<textarea bind:this={el} id={id} class="rich-editor" disabled={disabled}></textarea>
{:else}
	<textarea
		class="rich-editor"
		disabled
		value={String(value ?? '')}
		rows="6"
	></textarea>
{/if}

<style>
	.rich-editor {
		border-radius: var(--radius-field);
		min-height: 150px;
		max-height: 400px;
	}
</style>
