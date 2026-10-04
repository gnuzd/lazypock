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
	import {
		Bold,
		Italic,
		Underline,
		Strikethrough,
		Heading2,
		Heading3,
		List,
		ListOrdered,
		Quote,
		SquareCode,
		Image as ImageIcon,
		Link as LinkIcon,
		Table as TableIcon,
		Minus,
		RemoveFormatting,
		Images,
		Upload,
		Undo2,
		Redo2
	} from '@lucide/svelte';
	import { client } from '$lib/client';
	import { promptDialog } from '$lib/dialog.svelte';
	import MediaLibrary, { type MediaItem } from '$lib/components/MediaLibrary.svelte';

	let { value = $bindable(), disabled = false }: { value?: unknown; disabled?: boolean } = $props();

	let editor = $state<Editor | null>(null);
	let mediaOpen = $state(false);
	let uploadError = $state('');
	/** Insert-image dropdown: upload a new file or pick one from the library. */
	let imageMenuOpen = $state(false);
	let imageMenu = $state<HTMLDivElement | null>(null);
	let fileInput = $state<HTMLInputElement | null>(null);
	/** Bumped on every transaction so toolbar active/undo states re-evaluate. */
	let revision = $state(0);

	/** Last Markdown we emitted, so a `$effect` can tell our own update apart. */
	let lastEmitted = '';

	/** Embedded images always use the `content` preset, never the original. */
	function contentUrl(item: MediaItem): string {
		return item.variants?.content ?? item.url ?? `/api/files/${item.id}/scale/content`;
	}

	function insertImage(item: MediaItem) {
		editor
			?.chain()
			.focus()
			.setImage({ src: contentUrl(item), alt: item.filename })
			.run();
	}

	async function uploadFiles(files: File[]) {
		uploadError = '';

		for (const file of files) {
			if (!file.type.startsWith('image/')) continue;

			try {
				// Editor uploads are GC'd if they never end up in a record.
				const meta = { origin: 'editor' } as unknown as Parameters<typeof client.files.upload>[3];
				const res = await client.files.upload(file, file.name, undefined, meta);
				if (res?.id) insertImage(res as unknown as MediaItem);
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

	/** Hidden file input: upload the chosen image and insert it. */
	function onPick(event: Event) {
		const input = event.currentTarget as HTMLInputElement;
		const files = input.files;
		input.value = '';
		if (files && files.length > 0) void uploadFiles(Array.from(files));
	}

	/** Dropdown → upload a new image through the file picker. */
	function openUploadPicker() {
		imageMenuOpen = false;
		fileInput?.click();
	}

	/** Dropdown → pick an existing image from the media library. */
	function openLibraryPicker() {
		imageMenuOpen = false;
		mediaOpen = true;
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
			// The store re-emits the *same* Editor reference on every transaction,
			// so `editor` alone is not reactive. Bump a revision so the toolbar's
			// active / undo / redo states re-evaluate on selection and typing.
			onTransaction: () => {
				revision += 1;
			},
			editorProps: {
				handlePaste: (_view, event) => handleFiles((event as ClipboardEvent).clipboardData?.files),
				handleDrop: (_view, event) => handleFiles((event as DragEvent).dataTransfer?.files)
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

	async function insertLink() {
		const url = await promptDialog({
			title: 'Insert link',
			message: 'Link URL',
			placeholder: 'https://example.com',
			confirmLabel: 'Apply'
		});

		if (url) exec('setLink', { href: url });
	}

	function exec(cmd: string, attrs?: Record<string, unknown>) {
		const chain = editor?.chain().focus();
		if (chain && typeof (chain as Record<string, unknown>)[cmd] === 'function') {
			((chain as Record<string, unknown>)[cmd] as (a?: unknown) => unknown)(attrs);
			chain.run();
		}
	}

	function clearFormatting() {
		editor?.chain().focus().unsetAllMarks().clearNodes().run();
	}

	/** Keep focus/selection in the editor when a toolbar button is clicked. */
	function preventDefault(event: MouseEvent) {
		event.preventDefault();
	}

	function isActive(cmd: string, attrs?: Record<string, unknown>): boolean {
		void revision;
		return editor?.isActive(cmd, attrs) ?? false;
	}

	function canUndo(): boolean {
		void revision;
		return editor?.can().undo() ?? false;
	}

	function canRedo(): boolean {
		void revision;
		return editor?.can().redo() ?? false;
	}

	$effect(() => {
		if (editor && editor.isEditable === disabled) {
			editor.setEditable(!disabled);
		}
	});

	// Close the insert-image dropdown on outside click / Escape.
	$effect(() => {
		if (!imageMenuOpen) return;

		const onPointerDown = (event: PointerEvent) => {
			if (imageMenu && !imageMenu.contains(event.target as Node)) imageMenuOpen = false;
		};
		const onKeydown = (event: KeyboardEvent) => {
			if (event.key === 'Escape') imageMenuOpen = false;
		};

		document.addEventListener('pointerdown', onPointerDown);
		document.addEventListener('keydown', onKeydown);

		return () => {
			document.removeEventListener('pointerdown', onPointerDown);
			document.removeEventListener('keydown', onKeydown);
		};
	});

	$effect(() => {
		if (!editor) return;
		void editor;

		const incoming = String(value ?? '');
		// Our own emission (or an unchanged value) — nothing to sync.
		if (incoming === lastEmitted) return;

		lastEmitted = incoming;

		(
			editor as unknown as {
				commands: {
					setContent: (c: string, o?: { contentType?: string; emitUpdate?: boolean }) => void;
				};
			}
		).commands.setContent(incoming, { contentType: 'markdown', emitUpdate: false });
	});
</script>

<div class="rich-editor" class:disabled>
	{#if editor}
		<div class="toolbar" role="toolbar" aria-label="Formatting">
			<button
				type="button"
				class="toolbar-btn"
				class:active={isActive('bold')}
				title="Bold"
				aria-label="Bold"
				aria-pressed={isActive('bold')}
				onmousedown={preventDefault}
				onclick={() => exec('toggleBold')}><Bold size={15} /></button
			>
			<button
				type="button"
				class="toolbar-btn"
				class:active={isActive('italic')}
				title="Italic"
				aria-label="Italic"
				aria-pressed={isActive('italic')}
				onmousedown={preventDefault}
				onclick={() => exec('toggleItalic')}><Italic size={15} /></button
			>
			<button
				type="button"
				class="toolbar-btn"
				class:active={isActive('underline')}
				title="Underline"
				aria-label="Underline"
				aria-pressed={isActive('underline')}
				onmousedown={preventDefault}
				onclick={() => exec('toggleUnderline')}><Underline size={15} /></button
			>
			<button
				type="button"
				class="toolbar-btn"
				class:active={isActive('strike')}
				title="Strikethrough"
				aria-label="Strikethrough"
				aria-pressed={isActive('strike')}
				onmousedown={preventDefault}
				onclick={() => exec('toggleStrike')}><Strikethrough size={15} /></button
			>

			<span class="sep"></span>

			<button
				type="button"
				class="toolbar-btn"
				class:active={isActive('heading', { level: 2 })}
				title="Heading 2"
				aria-label="Heading 2"
				aria-pressed={isActive('heading', { level: 2 })}
				onmousedown={preventDefault}
				onclick={() => exec('toggleHeading', { level: 2 })}><Heading2 size={15} /></button
			>
			<button
				type="button"
				class="toolbar-btn"
				class:active={isActive('heading', { level: 3 })}
				title="Heading 3"
				aria-label="Heading 3"
				aria-pressed={isActive('heading', { level: 3 })}
				onmousedown={preventDefault}
				onclick={() => exec('toggleHeading', { level: 3 })}><Heading3 size={15} /></button
			>

			<span class="sep"></span>

			<button
				type="button"
				class="toolbar-btn"
				class:active={isActive('bulletList')}
				title="Bullet list"
				aria-label="Bullet list"
				aria-pressed={isActive('bulletList')}
				onmousedown={preventDefault}
				onclick={() => exec('toggleBulletList')}><List size={15} /></button
			>
			<button
				type="button"
				class="toolbar-btn"
				class:active={isActive('orderedList')}
				title="Numbered list"
				aria-label="Numbered list"
				aria-pressed={isActive('orderedList')}
				onmousedown={preventDefault}
				onclick={() => exec('toggleOrderedList')}><ListOrdered size={15} /></button
			>
			<button
				type="button"
				class="toolbar-btn"
				class:active={isActive('blockquote')}
				title="Blockquote"
				aria-label="Blockquote"
				aria-pressed={isActive('blockquote')}
				onmousedown={preventDefault}
				onclick={() => exec('toggleBlockquote')}><Quote size={15} /></button
			>
			<button
				type="button"
				class="toolbar-btn"
				class:active={isActive('codeBlock')}
				title="Code block"
				aria-label="Code block"
				aria-pressed={isActive('codeBlock')}
				onmousedown={preventDefault}
				onclick={() => exec('toggleCodeBlock')}><SquareCode size={15} /></button
			>

			<span class="sep"></span>

			<button
				type="button"
				class="toolbar-btn"
				class:active={isActive('link')}
				title="Link"
				aria-label="Link"
				aria-pressed={isActive('link')}
				onmousedown={preventDefault}
				onclick={() => void insertLink()}><LinkIcon size={15} /></button
			>
			<div class="toolbar-menu" bind:this={imageMenu}>
				<button
					type="button"
					class="toolbar-btn"
					class:active={imageMenuOpen}
					onmousedown={preventDefault}
					onclick={() => (imageMenuOpen = !imageMenuOpen)}
					title="Insert image"
					aria-label="Insert image"
					aria-haspopup="menu"
					aria-expanded={imageMenuOpen}><ImageIcon size={15} /></button
				>

				{#if imageMenuOpen}
					<div class="toolbar-dropdown" role="menu">
						<button
							type="button"
							role="menuitem"
							class="toolbar-menu-item"
							onclick={openUploadPicker}
						>
							<Upload size={14} />
							<span>Upload image</span>
						</button>
						<button
							type="button"
							role="menuitem"
							class="toolbar-menu-item"
							onclick={openLibraryPicker}
						>
							<Images size={14} />
							<span>Choose from library</span>
						</button>
					</div>
				{/if}
			</div>
			<button
				type="button"
				class="toolbar-btn"
				onmousedown={preventDefault}
				onclick={() => exec('insertTable', { rows: 3, cols: 3, withHeaderRow: true })}
				title="Insert table"
				aria-label="Insert table"><TableIcon size={15} /></button
			>
			<button
				type="button"
				class="toolbar-btn"
				onmousedown={preventDefault}
				onclick={() => exec('setHorizontalRule')}
				title="Horizontal rule"
				aria-label="Horizontal rule"><Minus size={15} /></button
			>
			<button
				type="button"
				class="toolbar-btn"
				onmousedown={preventDefault}
				onclick={clearFormatting}
				title="Clear formatting"
				aria-label="Clear formatting"><RemoveFormatting size={15} /></button
			>

			<span class="sep"></span>

			<button
				type="button"
				class="toolbar-btn"
				onclick={() => exec('undo')}
				title="Undo"
				aria-label="Undo"
				disabled={!canUndo()}><Undo2 size={15} /></button
			>
			<button
				type="button"
				class="toolbar-btn"
				onclick={() => exec('redo')}
				title="Redo"
				aria-label="Redo"
				disabled={!canRedo()}><Redo2 size={15} /></button
			>
		</div>
	{/if}

	<EditorContent editor={editor!} class="editor-content" />
</div>

<input bind:this={fileInput} type="file" accept="image/*" class="file-input" onchange={onPick} />

{#if uploadError}
	<p class="upload-error">{uploadError}</p>
{/if}

<MediaLibrary
	bind:open={mediaOpen}
	title="Insert image"
	onSelect={(items) => {
		if (items[0]) insertImage(items[0]);
	}}
/>

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
		align-items: center;
		gap: 2px;
		padding: 6px 12px;
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
		border-radius: 6px;
		background: none;
		color: color-mix(in oklab, var(--color-base-content) 70%, transparent);
		cursor: pointer;
		font-size: 0.8125rem;
		outline: 0;
		transition:
			background 0.1s,
			color 0.1s;
	}

	/* Lucide icons inherit the button colour. */
	.toolbar-btn :global(svg) {
		display: block;
	}

	.toolbar-btn:hover {
		background: color-mix(in oklab, var(--color-base-content) 10%, transparent);
		color: var(--color-base-content);
	}

	.toolbar-btn.active {
		background: color-mix(in oklab, var(--color-primary) 20%, var(--color-base-100));
		color: var(--color-primary);
	}

	.toolbar-btn:disabled {
		opacity: 0.35;
		cursor: default;
	}

	.toolbar-btn:disabled:hover {
		background: none;
		color: color-mix(in oklab, var(--color-base-content) 70%, transparent);
	}

	/* Insert-image dropdown (upload vs. library) */
	.toolbar-menu {
		position: relative;
	}

	.toolbar-dropdown {
		position: absolute;
		top: calc(100% + 4px);
		left: 0;
		z-index: 20;
		min-width: 12rem;
		padding: 4px;
		border: 1px solid var(--color-base-300);
		border-radius: 8px;
		background: var(--color-base-100);
		box-shadow: 0 10px 30px color-mix(in oklab, var(--color-base-content) 18%, transparent);
	}

	.toolbar-menu-item {
		display: flex;
		align-items: center;
		gap: 8px;
		width: 100%;
		padding: 8px 10px;
		border: none;
		border-radius: 6px;
		background: none;
		color: color-mix(in oklab, var(--color-base-content) 80%, transparent);
		cursor: pointer;
		font-size: 0.8125rem;
		text-align: left;
		transition:
			background 0.1s,
			color 0.1s;
	}

	.toolbar-menu-item:hover {
		background: color-mix(in oklab, var(--color-base-content) 8%, transparent);
		color: var(--color-base-content);
	}

	.toolbar-menu-item :global(svg) {
		flex-shrink: 0;
		color: color-mix(in oklab, var(--color-base-content) 50%, transparent);
	}

	.file-input {
		display: none;
	}

	.sep {
		display: inline-block;
		width: 1px;
		height: 20px;
		margin: 0 4px;
		background: color-mix(in oklab, var(--color-base-content) 15%, transparent);
		align-self: center;
	}

	.upload-error {
		margin: 4px 0 0;
		font-size: 0.8125rem;
		color: var(--color-error, #dc2626);
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

	/* Node selection (e.g. clicking an image) is otherwise invisible. */
	:global(.editor-content .ProseMirror-selectednode) {
		outline: 2px solid var(--color-primary);
		outline-offset: 2px;
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
</style>
