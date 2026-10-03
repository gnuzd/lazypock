<script lang="ts" module>
	export type MediaItem = {
		id: string;
		filename: string;
		mimeType?: string;
		size?: number;
		url?: string;
		thumbs?: Record<string, string>;
		variants?: Record<string, string>;
		origin?: string;
	};
</script>

<script lang="ts">
	import { client } from '$lib/client';
	import Button from '$lib/components/Button.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Tabs from '$lib/components/Tabs.svelte';

	/**
	 * Shared media library.
	 *
	 * Used by the richtext editor and the record file field so both offer the
	 * same two paths: **pick an image that was already uploaded** (Library tab)
	 * or **upload a new file** (Upload tab). In `manage` mode it is also the
	 * place to review and delete uploads.
	 */
	let {
		open = $bindable(false),
		/** Render the panel directly (no modal) — used by the Media page. */
		inline = false,
		mode = 'pick' as 'pick' | 'manage',
		multiple = false,
		title = 'Media library',
		accept = 'image/*',
		onSelect,
		onClose = () => {}
	}: {
		open?: boolean;
		inline?: boolean;
		mode?: 'pick' | 'manage';
		multiple?: boolean;
		title?: string;
		accept?: string;
		onSelect?: (items: MediaItem[]) => void;
		onClose?: () => void;
	} = $props();

	const PER_PAGE = 24;

	let tab = $state<'Library' | 'Upload'>('Library');
	let items = $state<MediaItem[]>([]);
	let selected = $state<string[]>([]);
	let preview = $state<MediaItem | null>(null);
	let q = $state('');
	let page = $state(1);
	let total = $state(0);
	let loaded = $state(false);
	let loading = $state(false);
	let uploading = $state(false);
	let deleting = $state<string | null>(null);
	let error = $state('');
	let notice = $state('');

	let searchTimer: ReturnType<typeof setTimeout> | undefined;
	let visible = $derived(inline || open);
	let imageOnly = $derived(accept === 'image/*');

	function thumbOf(item: MediaItem): string | undefined {
		if (item.variants?.thumb) return item.variants.thumb;
		if (!item.thumbs) return undefined;
		const sizes = Object.keys(item.thumbs).sort((a, b) => a.length - b.length);
		return sizes.length > 0 ? item.thumbs[sizes[0]] : undefined;
	}

	function bestUrl(item: MediaItem): string {
		return item.variants?.content ?? item.variants?.thumb ?? item.url ?? `/api/files/${item.id}`;
	}

	async function load(reset = false) {
		loading = true;
		error = '';

		if (reset) page = 1;

		try {
			const params: Record<string, string> = {
				page: String(page),
				perPage: String(PER_PAGE)
			};
			if (q.trim()) params.q = q.trim();
			if (imageOnly) params.mime = 'image/';

			const res = await client.http.get<{ items: MediaItem[]; total: number }>('/files', {
				params
			});
			const fetched = res?.items ?? [];
			items = reset ? fetched : [...items, ...fetched];
			total = res?.total ?? items.length;
		} catch (e) {
			error = (e as Error).message || 'Failed to load the library';
		} finally {
			loading = false;
		}
	}

	// Load once per open, and reset when the panel is dismissed.
	$effect(() => {
		if (visible && !loaded) {
			loaded = true;
			void load(true);
		}

		if (!visible && !inline && loaded) {
			loaded = false;
			items = [];
			selected = [];
			preview = null;
			q = '';
			tab = 'Library';
			error = '';
			notice = '';
		}
	});

	function onSearchInput() {
		clearTimeout(searchTimer);
		searchTimer = setTimeout(() => void load(true), 300);
	}

	async function loadMore() {
		page += 1;
		await load(false);
	}

	function choose(list: MediaItem[]) {
		if (list.length === 0) return;
		onSelect?.(multiple ? list : [list[0]]);
		close();
	}

	function toggle(item: MediaItem) {
		if (mode === 'manage') {
			preview = preview?.id === item.id ? null : item;
			return;
		}

		if (!multiple) {
			choose([item]);
			return;
		}

		selected = selected.includes(item.id)
			? selected.filter((id) => id !== item.id)
			: [...selected, item.id];
	}

	function onDrop(e: DragEvent) {
		e.preventDefault();
		void uploadFiles(Array.from(e.dataTransfer?.files ?? []));
	}

	async function uploadFiles(files: File[]) {
		const accepted = files.filter((f) => (imageOnly ? f.type.startsWith('image/') : true));
		if (accepted.length === 0) {
			error = imageOnly ? 'Only image files can be uploaded here.' : 'No files selected.';
			return;
		}

		uploading = true;
		error = '';
		notice = '';

		try {
			const uploaded: MediaItem[] = [];

			for (const file of accepted) {
				// `origin` is understood by lazypock >= 0.10 (library uploads are
				// never garbage-collected); the cast keeps older SDK types happy.
				const meta = { origin: 'library' } as unknown as Parameters<
					typeof client.files.upload
				>[3];
				const res = await client.files.upload(file, file.name, undefined, meta);
				if (res?.id) uploaded.push(res as unknown as MediaItem);
			}

			if (mode === 'pick' && uploaded.length > 0) {
				// "Upload a new file and use it" — insert straight away.
				choose(uploaded);
			} else {
				await load(true);
				tab = 'Library';
				notice = `Uploaded ${uploaded.length} file${uploaded.length === 1 ? '' : 's'}.`;
			}
		} catch (e) {
			error = (e as Error).message || 'Upload failed';
		} finally {
			uploading = false;
		}
	}

	function usageOf(e: unknown): string {
		const body = (e as { data?: { data?: { usage?: Array<Record<string, unknown>> } } }).data;
		const usage = body?.data?.usage ?? [];

		return usage.map((u) => `${u.collection}.${u.field} (${u.recordId})`).join(', ');
	}

	async function remove(item: MediaItem, force = false) {
		deleting = item.id;
		error = '';

		try {
			await client.http.request(
				'DELETE',
				`/files/${item.id}`,
				undefined,
				force ? { params: { force: 'true' } } : undefined
			);

			items = items.filter((i) => i.id !== item.id);
			total = Math.max(0, total - 1);
			selected = selected.filter((id) => id !== item.id);
			if (preview?.id === item.id) preview = null;
			notice = `Deleted ${item.filename}.`;
		} catch (e) {
			const status = (e as { status?: number }).status;

			if (status === 409 && !force) {
				const usage = usageOf(e);
				const question = usage
					? `"${item.filename}" is still used by ${usage}. Delete it anyway?`
					: `"${item.filename}" is still referenced by records. Delete it anyway?`;

				if (confirm(question)) {
					await remove(item, true);
					return;
				}

				notice = 'File is still referenced and was not deleted.';
			} else {
				error = (e as Error).message || 'Delete failed';
			}
		} finally {
			deleting = null;
		}
	}

	function formatSize(bytes?: number): string {
		if (!bytes) return '';
		if (bytes >= 1_048_576) return `${(bytes / 1_048_576).toFixed(1)} MB`;
		if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
		return `${bytes} B`;
	}

	function close() {
		if (!inline) open = false;
		onClose();
	}
</script>

{#snippet panel()}
	<div class="media-panel">
		{@render toolbar()}
		<Tabs items={['Library', 'Upload']} bind:active={tab} />

		{#if error}
			<p class="media-alert media-alert-error">{error}</p>
		{/if}
		{#if notice}
			<p class="media-alert">{notice}</p>
		{/if}

		{#if tab === 'Library'}
			{@render library()}
		{:else}
			{@render uploader()}
		{/if}
	</div>
{/snippet}

{#snippet toolbar()}
	<div class="media-toolbar">
		<input
			class="media-search"
			type="search"
			placeholder="Search by filename…"
			bind:value={q}
			oninput={onSearchInput}
		/>
		{#if mode === 'pick' && multiple}
			<Button
				class="btn-primary btn-sm"
				disabled={selected.length === 0}
				onclick={() => choose(items.filter((i) => selected.includes(i.id)))}
			>
				Insert {selected.length > 0 ? selected.length : ''}
			</Button>
		{/if}
		<Button class="btn-sm" loading={loading} onclick={() => load(true)}>Refresh</Button>
	</div>
{/snippet}

{#snippet library()}
	{#if loading && items.length === 0}
		<p class="media-empty">Loading…</p>
	{:else if items.length === 0}
		<p class="media-empty">
			No {imageOnly ? 'images' : 'files'} yet — upload one from the <strong>Upload</strong> tab.
		</p>
	{:else}
		<div class="media-grid">
			{#each items as item (item.id)}
				{@const thumb = thumbOf(item)}
				<div
					class="media-cell"
					class:picked={selected.includes(item.id)}
					class:active={preview?.id === item.id}
				>
					<button
						type="button"
						class="media-thumb"
						title={item.filename}
						onclick={() => toggle(item)}
					>
						{#if thumb}
							<img src={thumb} alt={item.filename} loading="lazy" />
						{:else}
							<span class="media-no-thumb">{item.filename}</span>
						{/if}
					</button>

					<div class="media-meta">
						<span class="media-name" title={item.filename}>{item.filename}</span>
						<span class="media-size">{formatSize(item.size)}</span>
					</div>

					<button
						type="button"
						class="media-delete"
						title="Delete"
						aria-label="Delete {item.filename}"
						disabled={deleting === item.id}
						onclick={() => remove(item)}
					>
						{deleting === item.id ? '…' : '🗑'}
					</button>
				</div>
			{/each}
		</div>

		{#if items.length < total}
			<div class="media-more">
				<Button class="btn-sm" loading={loading} onclick={loadMore}>
					Load more ({items.length}/{total})
				</Button>
			</div>
		{/if}
	{/if}

	{#if preview}
		<div class="media-preview">
			<img src={thumbOf(preview) ?? bestUrl(preview)} alt={preview.filename} />
			<div class="media-preview-meta">
				<strong>{preview.filename}</strong>
				<span>{preview.mimeType} · {formatSize(preview.size)}</span>
				<code class="media-url">{bestUrl(preview)}</code>
			</div>
			<div class="media-preview-actions">
				{#if mode === 'pick'}
					<Button class="btn-primary btn-sm" onclick={() => choose([preview!])}>Use this file</Button>
				{/if}
				<Button class="btn-sm" onclick={() => remove(preview!)}>Delete</Button>
			</div>
		</div>
	{/if}
{/snippet}

{#snippet uploader()}
	<div
		class="media-dropzone"
		role="button"
		tabindex="0"
		ondragover={(e) => e.preventDefault()}
		ondrop={onDrop}
	>
		<p>Drag &amp; drop {imageOnly ? 'images' : 'files'} here, or</p>
		<label class="media-file-label">
			choose files
			<input
				type="file"
				accept={accept}
				multiple
				disabled={uploading}
				onchange={(e) => {
					const input = e.target as HTMLInputElement;
					void uploadFiles(Array.from(input.files ?? []));
					input.value = '';
				}}
			/>
		</label>
		{#if uploading}
			<p class="media-uploading">Uploading…</p>
		{/if}
		{#if mode === 'pick'}
			<p class="media-hint">Uploaded files are used immediately.</p>
		{/if}
	</div>
{/snippet}

{#if inline}
	{@render panel()}
{:else}
	<Modal bind:show={open} {title}>
		{@render panel()}
	</Modal>
{/if}

<style>
	.media-panel {
		display: flex;
		flex-direction: column;
		gap: 10px;
		min-width: min(720px, 88vw);
	}

	.media-toolbar {
		display: flex;
		gap: 8px;
		align-items: center;
	}

	.media-search {
		flex: 1;
		min-width: 0;
		padding: 6px 10px;
		font-size: 0.875rem;
		color: var(--color-base-content);
		background: var(--color-base-100);
		border: 1px solid var(--color-base-300);
		border-radius: var(--radius-field, 6px);
	}

	.media-alert {
		margin: 0;
		font-size: 0.8125rem;
		color: color-mix(in oklab, var(--color-base-content) 70%, transparent);
	}

	.media-alert-error {
		color: var(--color-error, #dc2626);
	}

	.media-empty {
		padding: 28px 12px;
		text-align: center;
		font-size: 0.875rem;
		color: color-mix(in oklab, var(--color-base-content) 55%, transparent);
	}

	.media-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(120px, 1fr));
		gap: 10px;
		max-height: min(420px, 52vh);
		overflow-y: auto;
		padding: 2px;
	}

	.media-cell {
		position: relative;
		display: flex;
		flex-direction: column;
		border: 1px solid var(--color-base-300);
		border-radius: 8px;
		overflow: hidden;
		background: var(--color-base-100);
	}

	.media-cell.picked,
	.media-cell.active {
		border-color: var(--color-primary);
		box-shadow: 0 0 0 2px color-mix(in oklab, var(--color-primary) 30%, transparent);
	}

	.media-thumb {
		display: flex;
		align-items: center;
		justify-content: center;
		aspect-ratio: 1;
		width: 100%;
		padding: 0;
		border: none;
		background: color-mix(in oklab, var(--color-base-content) 5%, var(--color-base-100));
		cursor: pointer;
		overflow: hidden;
	}

	.media-thumb img {
		width: 100%;
		height: 100%;
		object-fit: cover;
	}

	.media-no-thumb {
		padding: 8px;
		font-size: 0.6875rem;
		word-break: break-all;
		color: color-mix(in oklab, var(--color-base-content) 60%, transparent);
	}

	.media-meta {
		display: flex;
		flex-direction: column;
		padding: 5px 7px;
		min-width: 0;
	}

	.media-name {
		font-size: 0.75rem;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.media-size {
		font-size: 0.6875rem;
		color: color-mix(in oklab, var(--color-base-content) 50%, transparent);
	}

	.media-delete {
		position: absolute;
		top: 4px;
		right: 4px;
		width: 24px;
		height: 24px;
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 0;
		border: none;
		border-radius: 6px;
		font-size: 0.75rem;
		cursor: pointer;
		background: color-mix(in oklab, var(--color-base-100) 85%, transparent);
		opacity: 0;
		transition: opacity 0.12s;
	}

	.media-cell:hover .media-delete,
	.media-cell:focus-within .media-delete {
		opacity: 1;
	}

	.media-more {
		display: flex;
		justify-content: center;
		padding-top: 4px;
	}

	.media-preview {
		display: flex;
		gap: 12px;
		align-items: center;
		padding: 8px;
		border: 1px solid var(--color-base-300);
		border-radius: 8px;
	}

	.media-preview img {
		width: 72px;
		height: 72px;
		object-fit: cover;
		border-radius: 6px;
	}

	.media-preview-meta {
		display: flex;
		flex-direction: column;
		gap: 2px;
		min-width: 0;
		flex: 1;
		font-size: 0.8125rem;
	}

	.media-url {
		font-size: 0.6875rem;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		color: color-mix(in oklab, var(--color-base-content) 60%, transparent);
	}

	.media-preview-actions {
		display: flex;
		gap: 6px;
	}

	.media-dropzone {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 6px;
		padding: 36px 16px;
		text-align: center;
		border: 2px dashed var(--color-base-300);
		border-radius: 10px;
		font-size: 0.875rem;
		color: color-mix(in oklab, var(--color-base-content) 70%, transparent);
	}

	.media-dropzone p {
		margin: 0;
	}

	.media-file-label {
		cursor: pointer;
		color: var(--color-primary);
		text-decoration: underline;
	}

	.media-file-label input {
		display: none;
	}

	.media-uploading {
		color: var(--color-primary);
	}

	.media-hint {
		font-size: 0.75rem;
		color: color-mix(in oklab, var(--color-base-content) 45%, transparent);
	}
</style>
