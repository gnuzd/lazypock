<script lang="ts">
	import { CircleAlert, ImageOff, Images, X } from '@lucide/svelte';
	import { client } from '$lib/client';
	import type { MediaItem } from '$lib/components/MediaLibrary.svelte';

	/**
	 * Image-only picker modal.
	 *
	 * A clean grid of already-uploaded images with paging — the counterpart to
	 * the editor's "Upload image" action, which handles new files. Kept
	 * separate from `MediaLibrary` (search / upload / delete / list view) so the
	 * insert flow stays focused.
	 */
	let {
		open = $bindable(false),
		title = 'Choose image',
		onSelect,
		onClose = () => {}
	}: {
		open?: boolean;
		title?: string;
		onSelect?: (item: MediaItem) => void;
		onClose?: () => void;
	} = $props();

	const PER_PAGE = 24;

	let items = $state<MediaItem[]>([]);
	let page = $state(1);
	let total = $state(0);
	let loading = $state(false);
	let error = $state('');
	/** Per-item image load state (skeleton until the thumbnail loads). */
	let loadedImages = $state<Record<string, boolean>>({});
	/** Guards against an out-of-order response overwriting a newer one. */
	let request = 0;
	/** Whether the first page has been requested for this open. */
	let fetched = $state(false);

	let totalPages = $derived(Math.max(1, Math.ceil(total / PER_PAGE)));
	let pageWindow = $derived(pageNumbers(page, totalPages));

	function pageNumbers(current: number, count: number): Array<number | '…'> {
		if (count <= 7) return Array.from({ length: count }, (_, i) => i + 1);

		const wanted = new Set([1, count, current - 1, current, current + 1]);
		const sorted = [...wanted].filter((p) => p >= 1 && p <= count).sort((a, b) => a - b);
		const out: Array<number | '…'> = [];

		for (let i = 0; i < sorted.length; i++) {
			if (i > 0 && sorted[i] - sorted[i - 1] > 1) out.push('…');
			out.push(sorted[i]);
		}

		return out;
	}

	/** Legacy `thumbs` map (files uploaded before presets existed). */
	function legacyThumbOf(item: MediaItem): string | undefined {
		if (!item.thumbs) return undefined;
		const sizes = Object.keys(item.thumbs).sort((a, b) => a.length - b.length);
		return sizes.length > 0 ? item.thumbs[sizes[0]] : undefined;
	}

	function thumbOf(item: MediaItem): string | undefined {
		return item.variants?.small ?? item.variants?.thumb ?? legacyThumbOf(item) ?? item.url;
	}

	function fileTypeLabel(item: MediaItem): string {
		const subtype = (item.mimeType ?? '').split('/')[1] ?? '';
		return subtype.split('+')[0].toUpperCase() || 'FILE';
	}

	function formatSize(bytes?: number): string {
		if (!bytes) return '';
		if (bytes >= 1_048_576) return `${(bytes / 1_048_576).toFixed(1)} MB`;
		if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
		return `${bytes} B`;
	}

	async function load() {
		const req = ++request;
		loading = true;
		error = '';

		try {
			const res = await client.http.get<{ items: MediaItem[]; total: number }>('/files', {
				params: { page: String(page), perPage: String(PER_PAGE), mime: 'image/' }
			});

			if (req !== request) return;
			items = res?.items ?? [];
			total = res?.total ?? items.length;
			loadedImages = {};
		} catch (e) {
			if (req !== request) return;
			error = (e as Error).message || 'Could not load the media library.';
			items = [];
			total = 0;
		} finally {
			if (req === request) loading = false;
		}
	}

	async function goToPage(target: number) {
		if (target === page || loading) return;
		page = Math.min(Math.max(1, target), totalPages);
		await load();
	}

	function choose(item: MediaItem) {
		onSelect?.(item);
		close();
	}

	function close() {
		open = false;
		onClose();
	}

	// Load the first page once per open; reset when dismissed.
	$effect(() => {
		if (open && !fetched) {
			fetched = true;
			page = 1;
			void load();
		}

		if (!open && fetched) {
			fetched = false;
			items = [];
			total = 0;
			error = '';
			page = 1;
		}
	});

	// Escape closes; lock page scroll while the modal is up.
	$effect(() => {
		if (!open) return;

		const onKeydown = (event: KeyboardEvent) => {
			if (event.key === 'Escape') close();
		};

		document.addEventListener('keydown', onKeydown);
		const previousOverflow = document.body.style.overflow;
		document.body.style.overflow = 'hidden';

		return () => {
			document.removeEventListener('keydown', onKeydown);
			document.body.style.overflow = previousOverflow;
		};
	});
</script>

{#if open}
	<div
		class="fixed inset-0 z-[1000] flex items-center justify-center bg-black/50 p-4"
		role="dialog"
		aria-modal="true"
		aria-label={title}
	>
		<button type="button" class="absolute inset-0 cursor-default" aria-label="Close" onclick={close}
		></button>

		<div
			class="relative flex max-h-[85vh] w-full max-w-3xl flex-col overflow-hidden rounded-2xl border border-base-300 bg-base-100 shadow-2xl"
		>
			<div class="flex items-center justify-between gap-4 border-b border-base-300 px-5 py-4">
				<div>
					<h2 class="text-sm font-bold text-base-content">{title}</h2>
					<p class="mt-0.5 text-xs text-base-content/50">
						{loading ? 'Loading…' : `${total} image${total === 1 ? '' : 's'} uploaded.`}
					</p>
				</div>
				<button
					type="button"
					onclick={close}
					aria-label="Close"
					class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg text-base-content/60 transition hover:bg-base-200 hover:text-base-content"
				>
					<X size={18} />
				</button>
			</div>

			<div class="min-h-[18rem] flex-1 overflow-y-auto p-5">
				{#if error}
					<div
						class="flex items-start gap-2 rounded-lg border border-error/40 bg-error/10 p-3 text-sm text-error"
						role="alert"
					>
						<CircleAlert size={16} class="mt-0.5 shrink-0" />
						<span>{error}</span>
					</div>
				{:else if loading}
					<div class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4">
						{#each [1, 2, 3, 4, 5, 6, 7, 8] as cell (cell)}
							<div class="space-y-2">
								<div class="aspect-video w-full animate-pulse rounded-xl bg-base-300/50"></div>
								<div class="h-3 w-3/4 animate-pulse rounded bg-base-300/50"></div>
							</div>
						{/each}
					</div>
				{:else if items.length === 0}
					<div class="py-16 text-center">
						<Images size={30} class="mx-auto mb-3 text-base-content/20" />
						<p class="text-sm text-base-content/50">No images in the library yet.</p>
					</div>
				{:else}
					<div class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4">
						{#each items as item (item.id)}
							{@const thumb = thumbOf(item)}
							<button
								type="button"
								onclick={() => choose(item)}
								title={item.filename}
								class="group overflow-hidden rounded-xl border border-base-300 bg-base-200 text-left transition hover:border-primary focus:border-primary focus:ring-2 focus:ring-primary/30 focus:outline-none"
							>
								<span class="relative block aspect-video overflow-hidden bg-base-300/50">
									{#if thumb}
										{#if !loadedImages[item.id]}
											<span class="absolute inset-0 animate-pulse bg-base-300/60"></span>
										{/if}
										<img
											src={thumb}
											alt={item.filename}
											loading="lazy"
											decoding="async"
											class="h-full w-full object-cover transition group-hover:scale-105"
											class:opacity-0={!loadedImages[item.id]}
											onload={() => (loadedImages[item.id] = true)}
											onerror={(e) => {
												const img = e.currentTarget as HTMLImageElement;
												const fallback = item.variants?.thumb ?? item.url ?? '';

												if (fallback && img.src !== fallback) img.src = fallback;
												else loadedImages[item.id] = true;
											}}
										/>
									{:else}
										<span class="flex h-full items-center justify-center text-base-content/30">
											<ImageOff size={20} />
										</span>
									{/if}
								</span>
								<span class="block px-2.5 py-2">
									<span class="block truncate text-xs font-medium text-base-content"
										>{item.filename}</span
									>
									<span class="mt-0.5 block text-[11px] text-base-content/40">
										{fileTypeLabel(item)}{item.size ? ` · ${formatSize(item.size)}` : ''}
									</span>
								</span>
							</button>
						{/each}
					</div>
				{/if}
			</div>

			{#if !loading && !error && totalPages > 1}
				<div class="border-t border-base-300 px-5 py-3">
					<div class="flex items-center justify-between gap-2">
						<button
							type="button"
							class="rounded-lg px-3 py-1.5 text-xs font-medium text-base-content/70 transition hover:bg-base-200 disabled:opacity-40"
							disabled={page <= 1 || loading}
							onclick={() => void goToPage(page - 1)}>Previous</button
						>

						<div class="flex items-center gap-1">
							{#each pageWindow as entry (entry)}
								{#if entry === '…'}
									<span class="px-1 text-xs text-base-content/40">…</span>
								{:else}
									<button
										type="button"
										class="min-w-7 rounded-lg px-2 py-1.5 text-xs transition {entry === page
											? 'bg-primary font-semibold text-primary-content'
											: 'text-base-content/70 hover:bg-base-200'}"
										disabled={loading}
										onclick={() => void goToPage(Number(entry))}>{entry}</button
									>
								{/if}
							{/each}
						</div>

						<button
							type="button"
							class="rounded-lg px-3 py-1.5 text-xs font-medium text-base-content/70 transition hover:bg-base-200 disabled:opacity-40"
							disabled={page >= totalPages || loading}
							onclick={() => void goToPage(page + 1)}>Next</button
						>
					</div>
				</div>
			{/if}
		</div>
	</div>
{/if}
