<script lang="ts">
	import { onMount, tick } from 'svelte';

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
		const tinymce = (await import('tinymce/tinymce')).default;

		// Load default skin inline
		await import('tinymce/themes/silver');
		await import('tinymce/icons/default');
		await import('tinymce/skins/ui/oxide/skin.css');

		// Load plugins
		await import('tinymce/plugins/autoresize');
		await import('tinymce/plugins/link');
		await import('tinymce/plugins/lists');
		await import('tinymce/plugins/table');
		await import('tinymce/plugins/code');

		loaded = true;

		// Wait for element to be in DOM
		await tick();

		if (!el) return;

		tinymce.init({
			target: el,
			menubar: false,
			statusbar: false,
			plugins: ['autoresize', 'link', 'lists', 'table', 'code'],
			toolbar:
				'bold italic underline strikethrough | ' +
				'bullist numlist | ' +
				'blockquote code | ' +
				'link table | ' +
				'removeformat',
			skin: false,
			content_css: false,
			branding: false,
			promotion: false,
			elementpath: false,
			convert_urls: false,
			relative_urls: false,
			remove_script_host: false,
			autoresize_bottom_margin: 0,
			min_height: 150,
			max_height: 400,
			placeholder: '',
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
