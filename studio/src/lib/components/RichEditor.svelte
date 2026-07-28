<script lang="ts">
	import { onMount } from 'svelte';
	import { StarterKit } from '@tiptap/starter-kit';
	import Link from '@tiptap/extension-link';
	import { Table, TableRow, TableCell, TableHeader } from '@tiptap/extension-table';
	import Placeholder from '@tiptap/extension-placeholder';
	import { Markdown } from '@tiptap/markdown';
	import { EditorContent, createEditor } from 'svelte-tiptap';
	import type { Editor } from 'svelte-tiptap';

	let { value = $bindable(), disabled = false }: { value?: unknown; disabled?: boolean } = $props();

	let editor = $state<Editor | null>(null);

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
				value = (ed as unknown as { getMarkdown: () => string }).getMarkdown();
			}
		});

		const unsub = edStore.subscribe((ed) => {
			editor = ed;
		});

		return unsub;
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
		const ed = editor as unknown as {
			getMarkdown: () => string;
			commands: { setContent: (content: string, opts?: { contentType?: string }) => void };
		};
		const md: string | undefined = ed.getMarkdown();
		if (md !== undefined && value !== md) {
			ed.commands.setContent(String(value ?? ''), { contentType: 'markdown' });
		}
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
