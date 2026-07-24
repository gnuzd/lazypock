<script lang="ts">
	import 'trix';
	import 'trix/dist/trix.css';

	let {
		name = '',
		value = $bindable(''),
		disabled = false,
	}: {
		name?: string;
		value?: unknown;
		disabled?: boolean;
	} = $props();

	let id = $derived('editor_' + name);
	let editorEl = $state<HTMLDivElement>();

	function handleTrixChange() {
		const el = editorEl?.querySelector('trix-editor');
		if (el) {
			value = (el as unknown as { editor: { getDocument: () => { toString: () => string } } }).editor.getDocument().toString();
		}
	}
</script>

<div class="rich-editor" class:disabled>
	<input type="hidden" id={id} {name} bind:value />
	<div bind:this={editorEl}>
		<trix-toolbar for={id}></trix-toolbar>
		<trix-editor
			input={id}
			ontrix-change={handleTrixChange}
			contenteditable={!disabled ? 'true' : undefined}
		></trix-editor>
	</div>
</div>

<style>
	.rich-editor {
		border-radius: var(--radius-field);
		overflow: hidden;
	}
	.rich-editor.disabled {
		opacity: 0.5;
		pointer-events: none;
	}
	:global(trix-toolbar) {
		background: color-mix(in oklab, var(--color-base-content) 6%, var(--color-base-100));
		border-bottom: 1px solid color-mix(in oklab, var(--color-base-content) 15%, transparent);
		padding: 2px;
	}
	:global(trix-editor) {
		min-height: 150px;
		max-height: 400px;
		overflow-y: auto;
		padding: 10px 12px;
		font-size: 0.9375rem;
		line-height: 1.6;
		border: none;
		background: transparent;
		color: var(--color-base-content);
	}
	:global(trix-editor:focus) {
		outline: none;
	}
	:global(trix-editor h1) {
		font-size: 1.25rem;
	}
	:global(trix-editor blockquote) {
		border-left: 3px solid color-mix(in oklab, var(--color-base-content) 20%, transparent);
		padding-left: 10px;
		margin: 8px 0;
	}
	:global(trix-editor pre) {
		background: color-mix(in oklab, var(--color-base-content) 8%, var(--color-base-100));
		border-radius: 4px;
		padding: 8px;
		font-family: 'SF Mono', 'Fira Code', monospace;
		font-size: 0.8125rem;
	}
</style>
