<script lang="ts">
	import { onMount } from 'svelte';
	import { StarterKit } from '@tiptap/starter-kit';
	import Link from '@tiptap/extension-link';
	import { Table, TableRow, TableCell, TableHeader } from '@tiptap/extension-table';
	import Placeholder from '@tiptap/extension-placeholder';
	import Image from '@tiptap/extension-image';
	import { Markdown } from '@tiptap/markdown';
	import { EditorContent, createEditor } from 'svelte-tiptap';
	import type { Editor } from 'svelte-tiptap';
	import { client } from '$lib/client';

	let { value = $bindable(), disabled = false }: { value?: unknown; disabled?: boolean } = $props();

	let editor = $state<Editor | null>(null);

	/** Last Markdown we emitted, so a `$effect` can tell our own update apart. */
	let lastEmitted = '';

	// ── Image picker state ──
	type PickerItem = {
		id: string;
		filename: string;
		url?: string;
		variants?: Record<string, string>;
		thumbs?: Record<string, string>;
	};

	let pickerOpen = $state(false);
	let pickerItems = $state<PickerItem[]>([]);
	let pickerLoading = $state(false);
	let pickerError = $state('');
	let uploadError = $state('');

	/** The `content` preset is what gets embedded — never the original. */
	function contentUrl(item: PickerItem): string {
		return (
			item.variants?.content ??
			item.variants?.thumb ??
			item.url ??
			`/api/files/${item.id}/scale/content`
		);
	}

	function insertImage(item: PickerItem) {
		editor?.chain().focus().setImage({ src: contentUrl(item), alt: item.filename }).run();
	}

	async function uploadFiles(files: File[]) {
		uploadError = '';
		for (const file of files) {
			if (!file.type.startsWith('image/')) continue;
			try {
				// `origin` is understood by lazypock >= 0.10 (editor uploads that are
				// never inserted into a record are garbage-collected); the cast keeps
				// this compiling against older SDK type definitions.
				const meta = { origin: 'editor' } as unknown as Parameters<
					typeof client.files.upload
				>[3];
				const res = await client.files.upload(file, file.name, undefined, meta);
				if (res?.id) insertImage(res as unknown as PickerItem);
			} catch (e) {
				uploadError = (e as Error).message || 'Upload failed';
			}
		}
	}

	/** Paste/drop handler: true means "we handled this event". */
	function handleFiles(files: FileList | null | undefined): boolean {
		if (!files || files.length === 0) return false;
		void uploadFiles(Array.from(files));
		return true;
	}

	async function openPicker() {
		pickerOpen = true;
		pickerError = '';
		pickerLoading = true;
		try {
			const res = await client.files.list({ mime: 'image/', perPage: 60 });
			pickerItems = (res?.items ?? []) as unknown as PickerItem[];
		} catch (e) {
			pickerError = (e as Error).message || 'Failed to load the library';
			pickerItems = [];
		} finally {
			pickerLoading = false;
		}
	}

	function pick(item: PickerItem) {
		insertImage(item);
		pickerOpen = false;
	}

	onMount(() => {
		const edStore = createEditor({
			extensions: [
				StarterKit.configure({
					heading: { levels: [1, 2, 3] }
				}),
				Link.configure({ openOnClick: false }),
				Table.configure({ resizable: true }),
				TableRow,
				TableCell,
				TableHeader,
				// Pasted images must never be inlined as base64.
				Image.configure({ inline: false, allowBase64: false }),
				Placeholder.configure({ placeholder: 'Write something…' }),
				Markdown.configure({
					indentation: { style: 'space', size: 2 },
					markedOptions: { gfm: true }
				})
			],
			contentType: 'markdown',
			content: String(value ?? ''),
			editable: !disabled,
			onUpdate: ({ editor: ed }) => {
				// Serialise once and remember it, so the value-sync effect does not
				// re-serialise the whole document on every keystroke.
				const md = (ed as unknown as { getMarkdown: () => string }).getMarkdown();
				lastEmitted = md;
				value = md;
			},
			editorProps: {
				handlePaste: (_view, event) =>
					handleFiles((event as ClipboardEvent).clipboardData?.files),
				handleDrop: (_view, event) =>
					handleFiles((event as DragEvent).dataTransfer?.files)
			}
		});

		const unsub = edStore.subscribe((ed) => {
			editor = ed;
		});

		return () => {
			unsub();
			editor?.destroy();
			editor = null;
		};
	});

	function exec(cmd: string, attrs?: Record<string, unknown>) {
		const chain = editor?.chain().focus();
		if (chain && typeof (chain as Record<string, unknown>)[cmd] === 'function') {
			((chain as Record<string, unknown>)[cmd] as (a?: unknown) => unknown)(attrs);
			chain.run();
		}
	}

	function isActive(cmd: string, attrs?: Record<string, unknown>): boolean {
		return editor?.isActive(cmd, attrs) ?? false;
	}

	$effect(() => {
		if (editor && editor.isEditable === disabled) {
			editor.setEditable(!disabled);
		}
	});

	$effect(() => {
		if (!editor) return;
		void editor;

		const incoming = String(value ?? '');
		// Our own emission (or an unchanged value) — nothing to sync.
		if (incoming === lastEmitted) return;

		lastEmitted = incoming;

		(editor as unknown as {
			commands: { setContent: (c: string, o?: { contentType?: string; emitUpdate?: boolean }) => void };
		}).commands.setContent(incoming, { contentType: 'markdown', emitUpdate: false });
	});
</script>

<div class="rich-editor" class:disabled>
	{#if editor}
		<div class="toolbar">
			<button
				type="button"
				class="toolbar-btn"
				class:active={isActive('bold')}
				onclick={() => exec('toggleBold')}
				title="Bold"><b>B</b></button
			>
			<button
				type="button"
				class="toolbar-btn"
				class:active={isActive('italic')}
				onclick={() => exec('toggleItalic')}
				title="Italic"><i>I</i></button
			>
			<button
				type="button"
				class="toolbar-btn"
				class:active={isActive('underline')}
				onclick={() => exec('toggleUnderline')}
				title="Underline"><u>U</u></button
			>
			<button
				type="button"
				class="toolbar-btn"
				class:active={isActive('strike')}
				onclick={() => exec('toggleStrike')}
				title="Strikethrough"><s>S</s></button
			>

			<span class="sep"></span>

			<button
				type="button"
				class="toolbar-btn"
				class:active={isActive('bulletList')}
				onclick={() => exec('toggleBulletList')}
				title="Bullet list">•</button
			>
			<button
				type="button"
				class="toolbar-btn"
				class:active={isActive('orderedList')}
				onclick={() => exec('toggleOrderedList')}
				title="Numbered list">1.</button
			>
			<button
				type="button"
				class="toolbar-btn"
				class:active={isActive('blockquote')}
				onclick={() => exec('toggleBlockquote')}
				title="Blockquote">"</button
			>
			<button
				type="button"
				class="toolbar-btn"
				class:active={isActive('codeBlock')}
				onclick={() => exec('toggleCodeBlock')}
				title="Code block">&lt;/&gt;</button
			>

			<span class="sep"></span>

			<button
				type="button"
				class="toolbar-btn"
				onclick={() => exec('setLink', { href: prompt('Link URL:') })}
				title="Link">🔗</button
			>
			<button type="button" class="toolbar-btn" onclick={openPicker} title="Insert image"
				>🖼</button
			>
			<button
				type="button"
				class="toolbar-btn"
				onclick={() => exec('insertTable', { rows: 3, cols: 3, withHeaderRow: true })}
				title="Insert table">⊞</button
			>

			<span class="sep"></span>

			<button type="button" class="toolbar-btn" onclick={() => exec('undo')} title="Undo">↩</button>
			<button type="button" class="toolbar-btn" onclick={() => exec('redo')} title="Redo">↪</button>
		</div>
	{/if}

	<EditorContent editor={editor!} class="editor-content" />
</div>

{#if uploadError}
	<p class="picker-status picker-error">{uploadError}</p>
{/if}

{#if pickerOpen}
	<div class="picker-backdrop" role="presentation" onclick={() => (pickerOpen = false)}>
		<div
			class="picker"
			role="dialog"
			aria-label="Image library"
			onclick={(e) => e.stopPropagation()}
		>
			<div class="picker-head">
				<strong>Image library</strong>
				<label class="picker-upload">
					Upload
					<input
						type="file"
						accept="image/*"
						multiple
						onchange={(e) => uploadFiles(Array.from((e.target as HTMLInputElement).files ?? []))}
					/>
				</label>
				<button type="button" class="picker-close" onclick={() => (pickerOpen = false)}>✕</button>
			</div>

			{#if pickerLoading}
				<div class="picker-status">Loading images…</div>
			{:else if pickerError}
				<div class="picker-status picker-error">{pickerError}</div>
			{:else if pickerItems.length === 0}
				<div class="picker-status">No images yet — upload one above.</div>
			{:else}
				<div class="picker-grid">
					{#each pickerItems as item (item.id)}
						{@const thumb =
							item.variants?.thumb ??
							(item.thumbs ? item.thumbs[Object.keys(item.thumbs)[0]] : undefined)}
						<button type="button" class="picker-cell" onclick={() => pick(item)}>
							{#if thumb}
								<img src={thumb} alt={item.filename} loading="lazy" />
							{:else}
								<span class="picker-no-thumb">{item.filename}</span>
							{/if}
						</button>
					{/each}
				</div>
			{/if}
		</div>
	</div>
{/if}

<style>
	.rich-editor {
		min-height: 150px;
		overflow: hidden;
	}

	.rich-editor.disabled {
		opacity: 0.5;
		pointer-events: none;
	}

	.toolbar {
		display: flex;
		flex-wrap: wrap;
		gap: 2px;
		padding: 4px 12px;
		background: color-mix(in oklab, var(--color-base-content) 4%, transparent);
		border-bottom: 1px solid color-mix(in oklab, var(--color-base-content) 10%, transparent);
	}

	.toolbar-btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 28px;
		height: 28px;
		border: none;
		border-radius: 4px;
		background: none;
		color: var(--color-base-content);
		cursor: pointer;
		font-size: 0.8125rem;
		outline: 0;
		transition: background 0.1s;
	}

	.toolbar-btn:hover {
		background: color-mix(in oklab, var(--color-base-content) 10%, transparent);
	}

	.toolbar-btn.active {
		background: color-mix(in oklab, var(--color-primary) 20%, var(--color-base-100));
		color: var(--color-primary);
	}

	.sep {
		display: inline-block;
		width: 1px;
		height: 20px;
		margin: 4px 2px;
		background: color-mix(in oklab, var(--color-base-content) 15%, transparent);
		align-self: center;
	}

	:global(.editor-content) {
		padding: 10px 12px;
		min-height: 150px;
		max-height: 400px;
		overflow-y: auto;
		font-size: 0.9375rem;
		line-height: 1.6;
		color: var(--color-base-content);
	}

	:global(.editor-content:focus),
	:global(.editor-content:focus-visible),
	:global(.ProseMirror),
	:global(.ProseMirror:focus),
	:global(.ProseMirror:focus-visible) {
		outline: none;
		box-shadow: none;
	}

	:global(.editor-content p) {
		margin: 0;
	}

	:global(.editor-content p.is-editor-empty:first-child::before) {
		content: attr(data-placeholder);
		float: left;
		color: color-mix(in oklab, var(--color-base-content) 40%, transparent);
		pointer-events: none;
		height: 0;
	}

	:global(.editor-content h1) {
		font-size: 1.4rem;
		margin: 0.5rem 0 0.25rem;
	}
	:global(.editor-content h2) {
		font-size: 1.2rem;
		margin: 0.4rem 0 0.2rem;
	}
	:global(.editor-content h3) {
		font-size: 1.1rem;
		margin: 0.3rem 0 0.15rem;
	}

	:global(.editor-content blockquote) {
		border-left: 3px solid color-mix(in oklab, var(--color-base-content) 20%, transparent);
		padding-left: 10px;
		margin: 8px 0;
	}

	:global(.editor-content pre) {
		background: color-mix(in oklab, var(--color-base-content) 8%, var(--color-base-100));
		border-radius: 4px;
		padding: 8px;
		font-family: 'SF Mono', 'Fira Code', monospace;
		font-size: 0.8125rem;
		overflow-x: auto;
	}

	:global(.editor-content code) {
		background: color-mix(in oklab, var(--color-base-content) 8%, var(--color-base-100));
		border-radius: 3px;
		padding: 1px 4px;
		font-size: 0.85em;
	}

	:global(.editor-content ul),
	:global(.editor-content ol) {
		padding-left: 1.5rem;
	}

	:global(.editor-content img) {
		max-width: 100%;
		height: auto;
		border-radius: 4px;
	}

	:global(.editor-content table) {
		border-collapse: collapse;
		width: 100%;
		margin: 8px 0;
	}

	:global(.editor-content th),
	:global(.editor-content td) {
		border: 1px solid color-mix(in oklab, var(--color-base-content) 20%, transparent);
		padding: 6px 10px;
		text-align: left;
	}

	:global(.editor-content th) {
		background: color-mix(in oklab, var(--color-base-content) 6%, var(--color-base-100));
		font-weight: bold;
	}

	:global(.editor-content a) {
		color: var(--color-primary);
		text-decoration: underline;
		cursor: pointer;
	}

	.picker-status {
		padding: 8px 12px;
		font-size: 0.8125rem;
		color: color-mix(in oklab, var(--color-base-content) 60%, transparent);
	}

	.picker-error {
		color: var(--color-error, #dc2626);
	}

	.picker-backdrop {
		position: fixed;
		inset: 0;
		background: rgb(0 0 0 / 0.45);
		display: flex;
		align-items: center;
		justify-content: center;
		z-index: 60;
	}

	.picker {
		width: min(680px, 92vw);
		max-height: 80vh;
		overflow: auto;
		background: var(--color-base-100);
		border-radius: 8px;
		box-shadow: 0 20px 50px rgb(0 0 0 / 0.3);
	}

	.picker-head {
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 10px 14px;
		border-bottom: 1px solid color-mix(in oklab, var(--color-base-content) 12%, transparent);
	}

	.picker-head strong {
		flex: 1;
	}

	.picker-upload {
		cursor: pointer;
		font-size: 0.8125rem;
		padding: 4px 10px;
		border-radius: 4px;
		background: color-mix(in oklab, var(--color-primary) 15%, var(--color-base-100));
	}

	.picker-upload input {
		display: none;
	}

	.picker-close {
		border: none;
		background: none;
		cursor: pointer;
		font-size: 1rem;
		color: var(--color-base-content);
	}

	.picker-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(110px, 1fr));
		gap: 8px;
		padding: 12px;
	}

	.picker-cell {
		border: 1px solid color-mix(in oklab, var(--color-base-content) 12%, transparent);
		border-radius: 6px;
		background: none;
		padding: 0;
		cursor: pointer;
		aspect-ratio: 1;
		overflow: hidden;
		display: flex;
		align-items: center;
		justify-content: center;
	}

	.picker-cell img {
		width: 100%;
		height: 100%;
		object-fit: cover;
	}

	.picker-no-thumb {
		font-size: 0.6875rem;
		padding: 8px;
		word-break: break-all;
	}
</style>
