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
	import { onMount } from 'svelte';
	import { client } from '$lib/client';
	import { confirmDialog } from '$lib/dialog.svelte';
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
	const VIEW_KEY = 'lazypock-media-view';

	let tab = $state<'Library' | 'Upload'>('Library');
	let items = $state<MediaItem[]>([]);
	let selected = $state<string[]>([]);
	let preview = $state<MediaItem | null>(null);
	/** Detail modal — opened by clicking a file in the library. */
	let detailOpen = $state(false);
	let q = $state('');
	let page = $state(1);
	let total = $state(0);
	let loaded = $state(false);
	let loading = $state(false);
	let uploading = $state(false);
	let deleting = $state<string | null>(null);
	let error = $state('');
	let notice = $state('');
	/** Grid vs list presentation (remembered across sessions). */
	let viewMode = $state<'grid' | 'list'>('grid');

	let searchTimer: ReturnType<typeof setTimeout> | undefined;
	let visible = $derived(inline || open);
	let imageOnly = $derived(accept === 'image/*');

	// Remember the last view mode, so the preference is shared by the Media page,
	// the record file picker and the richtext image dialog.
	onMount(() => {
		try {
			const stored = localStorage.getItem(VIEW_KEY);
			if (stored === 'grid' || stored === 'list') viewMode = stored;
		} catch {
			// storage unavailable — keep the default
		}
	});

	function setView(mode: 'grid' | 'list') {
		viewMode = mode;

		try {
			localStorage.setItem(VIEW_KEY, mode);
		} catch {
			// ignore
		}
	}

	/** Legacy `thumbs` map (files uploaded before presets existed). */
	function legacyThumbOf(item: MediaItem): string | undefined {
		if (!item.thumbs) return undefined;
		const sizes = Object.keys(item.thumbs).sort((a, b) => a.length - b.length);
		return sizes.length > 0 ? item.thumbs[sizes[0]] : undefined;
	}

	/** Small preview (list rows, record field previews): the 100px preset. */
	function thumbOf(item: MediaItem): string | undefined {
		return item.variants?.thumb ?? legacyThumbOf(item);
	}

	/**
	 * Grid tile image: the 320px `small` preset is sharp at tile size (up to
	 * ~360 device px on a retina screen, where the 100px `thumb` looked soft).
	 * The chain falls back to the original so a tile is never blank — e.g. for
	 * files uploaded before presets existed.
	 */
	function gridThumbOf(item: MediaItem): string | undefined {
		return item.variants?.small ?? thumbOf(item) ?? item.url;
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
			detailOpen = false;
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

	function openDetail(item: MediaItem) {
		preview = item;
		detailOpen = true;
	}

	async function copyUrl(item: MediaItem) {
		const url = bestUrl(item);

		try {
			if (typeof navigator !== 'undefined' && navigator.clipboard) {
				await navigator.clipboard.writeText(url);
				notice = 'URL copied to the clipboard.';
			} else {
				notice = url;
			}
		} catch {
			notice = url;
		}
	}

	function toggle(item: MediaItem) {
		if (mode === 'manage') {
			openDetail(item);
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

	/** `collection.field (recordId)` strings from a delete-guard 409 response. */
	function usageList(e: unknown): string[] {
		const body = (e as { data?: { data?: { usage?: Array<Record<string, unknown>> } } }).data;
		const usage = body?.data?.usage ?? [];

		return usage.map((u) => `${u.collection}.${u.field} (${u.recordId})`);
	}

	/** Ask first, then delete — the confirmation is always a modal. */
	async function askDelete(item: MediaItem) {
		const ok = await confirmDialog({
			title: 'Delete file',
			message: `Delete "${item.filename}"? This removes the file and all of its variants from storage.`,
			confirmLabel: 'Delete',
			variant: 'error'
		});

		if (ok) await remove(item);
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
			if (preview?.id === item.id) {
				preview = null;
				detailOpen = false;
			}
			notice = `Deleted ${item.filename}.`;
		} catch (e) {
			const status = (e as { status?: number }).status;

			if (status === 409 && !force) {
				// The delete guard: show exactly what still points at the file and let
				// the user decide — always a modal, never window.confirm().
				const refs = usageList(e);

				const forceIt = await confirmDialog({
					title: 'File is still in use',
					message: `"${item.filename}" is still referenced by ${refs.length === 1 ? 'this record' : `${refs.length} records`}. Deleting it leaves those records pointing at a missing file.`,
					details: refs,
					confirmLabel: 'Delete anyway',
					variant: 'error'
				});

				if (forceIt) await remove(item, true);
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
		<div class="media-view-toggle" role="group" aria-label="View mode">
			<button
				type="button"
				class="media-view-btn"
				class:active={viewMode === 'grid'}
				aria-pressed={viewMode === 'grid'}
				title="Grid view"
				onclick={() => setView('grid')}>▦</button
			>
			<button
				type="button"
				class="media-view-btn"
				class:active={viewMode === 'list'}
				aria-pressed={viewMode === 'list'}
				title="List view"
				onclick={() => setView('list')}>☰</button
			>
		</div>
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
	{:else if viewMode === 'grid'}
		<div class="media-grid">
			{#each items as item (item.id)}
				{@const thumb = gridThumbOf(item)}
				<div
					class="media-cell"
					class:picked={selected.includes(item.id)}
					class:active={preview?.id === item.id}
				>
					<div class="media-tile-wrap">
						<button
							type="button"
							class="media-tile"
							title={item.filename}
							onclick={() => toggle(item)}
						>
							{#if thumb}
								<img
									src={thumb}
									alt={item.filename}
									loading="lazy"
									decoding="async"
									onerror={(e) => {
										// A variant that has not been generated yet (e.g. an old file on a
										// CDN-backed bucket) must not leave an empty tile: step down to the
										// smaller preset, then the original.
										const img = e.currentTarget as HTMLImageElement;
										const fallback = item.variants?.thumb ?? item.url ?? '';

										if (fallback && img.src !== fallback) img.src = fallback;
									}}
								/>
							{:else}
								<span class="media-no-thumb">{item.filename}</span>
							{/if}
						</button>

						<button
							type="button"
							class="media-delete"
							title="Delete"
							aria-label="Delete {item.filename}"
							disabled={deleting === item.id}
							onclick={() => void askDelete(item)}
						>
							{deleting === item.id ? '…' : '🗑'}
						</button>
					</div>

					<div class="media-meta">
						<span class="media-name" title={item.filename}>{item.filename}</span>
						<span class="media-size">{formatSize(item.size)}</span>
					</div>
				</div>
			{/each}
		</div>
	{:else}
		<div class="media-list">
			{#each items as item (item.id)}
				{@const thumb = thumbOf(item) ?? item.url}
				<div
					class="media-row"
					class:picked={selected.includes(item.id)}
					class:active={preview?.id === item.id}
				>
					<button
						type="button"
						class="media-row-main"
						title={item.filename}
						onclick={() => toggle(item)}
					>
						<span class="media-row-thumb">
							{#if thumb}
								<img src={thumb} alt={item.filename} loading="lazy" decoding="async" />
							{:else}
								<span class="media-no-thumb">—</span>
							{/if}
						</span>
						<span class="media-row-name">{item.filename}</span>
						<span class="media-row-type">{item.mimeType ?? ''}</span>
						<span class="media-row-size">{formatSize(item.size)}</span>
					</button>

					<button
						type="button"
						class="media-row-delete"
						title="Delete"
						aria-label="Delete {item.filename}"
						disabled={deleting === item.id}
						onclick={() => void askDelete(item)}
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
	<Modal
		bind:show={open}
		{title}
		size="xl"
		bodyClass="flex min-h-0 flex-col overflow-hidden"
	>
		{@render panel()}
	</Modal>
{/if}

<!-- Detail view: clicking a file in the library opens this instead of an
     inline panel at the bottom, so the whole file is visible in one place. -->
<Modal bind:show={detailOpen} size="lg" title="File details" bodyClass="flex min-h-0 flex-col">
	{#if preview}
		<div class="detail">
			<div class="detail-media">
				<img src={bestUrl(preview)} alt={preview.filename} />
			</div>
			<div class="detail-meta">
				<strong class="detail-name" title={preview.filename}>{preview.filename}</strong>
				<span>
					{preview.mimeType ?? 'unknown type'}{preview.size
						? ` · ${formatSize(preview.size)}`
						: ''}
				</span>
				<code class="detail-url" title={bestUrl(preview)}>{bestUrl(preview)}</code>
				<span class="detail-id">id: {preview.id}</span>
			</div>
		</div>
		<div class="detail-actions">
			<Button class="btn-sm" onclick={() => void copyUrl(preview!)}>Copy URL</Button>
			{#if mode === 'pick'}
				<Button class="btn-primary btn-sm" onclick={() => choose([preview!])}>Use this file</Button>
			{/if}
			<Button class="btn-error btn-sm" onclick={() => void askDelete(preview!)}>Delete</Button>
		</div>
	{/if}
</Modal>

<style>
	.media-panel {
		display: flex;
		flex-direction: column;
		gap: 10px;
		/* Fill the modal body so the grid (not the dialog) scrolls. In the
		   inline/page case the flex properties are inert and the page scrolls. */
		flex: 1;
		min-height: 0;
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

	/* ── Grid: square tiles with the name underneath (Drive-like) ── */
	.media-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
		gap: 14px;
		/* Grow into whatever height the modal/page gives us and scroll inside. */
		flex: 1 1 auto;
		min-height: 200px;
		overflow-y: auto;
		padding: 2px;
	}

	.media-cell {
		display: flex;
		flex-direction: column;
		gap: 6px;
		min-width: 0;
	}

	.media-tile-wrap {
		position: relative;
		aspect-ratio: 1;
		border: 1px solid var(--color-base-300);
		border-radius: 8px;
		overflow: hidden;
		background: color-mix(in oklab, var(--color-base-content) 5%, var(--color-base-100));
	}

	.media-cell.picked .media-tile-wrap,
	.media-cell.active .media-tile-wrap {
		border-color: var(--color-primary);
		box-shadow: 0 0 0 2px color-mix(in oklab, var(--color-primary) 35%, transparent);
	}

	.media-tile {
		display: flex;
		align-items: center;
		justify-content: center;
		width: 100%;
		height: 100%;
		padding: 0;
		border: none;
		background: none;
		cursor: pointer;
		overflow: hidden;
	}

	.media-tile img {
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

	/* ── Detail modal ── */
	.detail {
		display: flex;
		gap: 16px;
		flex: 1;
		min-height: 0;
	}

	.detail-media {
		display: flex;
		align-items: center;
		justify-content: center;
		flex: 1 1 auto;
		min-width: 0;
		border-radius: 8px;
		overflow: hidden;
		background: color-mix(in oklab, var(--color-base-content) 5%, var(--color-base-100));
	}

	.detail-media img {
		max-width: 100%;
		max-height: min(62vh, 560px);
		object-fit: contain;
	}

	.detail-meta {
		display: flex;
		flex-direction: column;
		gap: 6px;
		flex-shrink: 0;
		width: 280px;
		min-width: 0;
		font-size: 0.8125rem;
		color: color-mix(in oklab, var(--color-base-content) 75%, transparent);
	}

	.detail-name {
		font-size: 0.9375rem;
		color: var(--color-base-content);
		word-break: break-all;
	}

	.detail-url {
		padding: 6px 8px;
		border-radius: 6px;
		font-size: 0.6875rem;
		background: color-mix(in oklab, var(--color-base-content) 6%, var(--color-base-100));
		word-break: break-all;
	}

	.detail-id {
		font-size: 0.6875rem;
		color: color-mix(in oklab, var(--color-base-content) 50%, transparent);
		word-break: break-all;
	}

	.detail-actions {
		display: flex;
		justify-content: flex-end;
		gap: 8px;
		padding-top: 14px;
		flex-shrink: 0;
	}

	@media (max-width: 720px) {
		.detail {
			flex-direction: column;
		}

		.detail-meta {
			width: auto;
		}
	}

	.media-dropzone {
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 6px;
		padding: 36px 16px;
		flex: 1 1 auto;
		min-height: 200px;
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

	/* ── List view: rows with a small square thumbnail ── */
	.media-list {
		display: flex;
		flex-direction: column;
		flex: 1 1 auto;
		min-height: 200px;
		overflow-y: auto;
	}

	.media-row {
		display: flex;
		align-items: center;
		border-radius: 6px;
	}

	.media-row:hover {
		background: color-mix(in oklab, var(--color-base-content) 5%, transparent);
	}

	.media-row.picked,
	.media-row.active {
		background: color-mix(in oklab, var(--color-primary) 12%, transparent);
	}

	.media-row-main {
		display: flex;
		align-items: center;
		gap: 12px;
		flex: 1;
		min-width: 0;
		padding: 6px 8px;
		border: none;
		background: none;
		cursor: pointer;
		text-align: left;
	}

	.media-row-thumb {
		display: flex;
		align-items: center;
		justify-content: center;
		width: 40px;
		height: 40px;
		flex-shrink: 0;
		border: 1px solid var(--color-base-300);
		border-radius: 6px;
		overflow: hidden;
		background: color-mix(in oklab, var(--color-base-content) 5%, var(--color-base-100));
	}

	.media-row-thumb img {
		width: 100%;
		height: 100%;
		object-fit: cover;
	}

	.media-row-name {
		flex: 1;
		min-width: 0;
		font-size: 0.8125rem;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.media-row-type,
	.media-row-size {
		flex-shrink: 0;
		font-size: 0.75rem;
		color: color-mix(in oklab, var(--color-base-content) 55%, transparent);
	}

	.media-row-type {
		width: 130px;
	}

	.media-row-size {
		width: 72px;
		text-align: right;
	}

	.media-row-delete {
		display: flex;
		align-items: center;
		justify-content: center;
		width: 28px;
		height: 28px;
		flex-shrink: 0;
		margin-right: 4px;
		padding: 0;
		border: none;
		border-radius: 6px;
		background: none;
		cursor: pointer;
		font-size: 0.75rem;
		opacity: 0;
	}

	.media-row:hover .media-row-delete,
	.media-row:focus-within .media-row-delete {
		opacity: 1;
	}

	@media (max-width: 640px) {
		.media-row-type,
		.media-row-size {
			display: none;
		}
	}

	/* ── View toggle ── */
	.media-view-toggle {
		display: inline-flex;
		border: 1px solid var(--color-base-300);
		border-radius: 6px;
		overflow: hidden;
	}

	.media-view-btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 30px;
		height: 30px;
		border: none;
		background: none;
		color: color-mix(in oklab, var(--color-base-content) 60%, transparent);
		cursor: pointer;
		font-size: 0.875rem;
	}

	.media-view-btn:hover {
		background: color-mix(in oklab, var(--color-base-content) 8%, transparent);
	}

	.media-view-btn.active {
		background: color-mix(in oklab, var(--color-primary) 18%, var(--color-base-100));
		color: var(--color-primary);
	}
</style>
