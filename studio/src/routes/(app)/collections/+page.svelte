<script lang="ts">
	import { client } from '$lib/client';

	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { Settings, Plus, Trash } from '@lucide/svelte';
	import { getThumbUrl } from 'lazypock';
	import Button from '$lib/components/Button.svelte';
	import DataTable from '$lib/components/DataTable.svelte';
	import Select from '$lib/components/Select.svelte';
	import RecordSidePane from '$lib/components/RecordSidePane.svelte';
	import CollectionEditor from '$lib/components/CollectionEditor.svelte';
	import SidePane from '$lib/components/SidePane.svelte';
	import { activeName, collections } from '$lib/collectionsStore';

	let collection = $state<Record<string, unknown> | null>(null);
	let rows = $state<Record<string, unknown>[]>([]);
	let loading = $state(false);
	// ── Pagination state ──
	let currentPage = $state(1);
	let perPage = $state(50);
	let totalItems = $state(0);
	let totalPages = $state(1);
	// ── Collection edit pane state ──
	let showCollectionPane = $state(false);
	let collectionEditName = $state('');
	let deleteConfirmOpen = $state(false);
	// ── Record CRUD state ──
	let showRecordPane = $state(false);
	let recordData = $state<Record<string, unknown>>({});
	let editingRecordId = $state<string | null>(null);
	// ── Bulk selection ──
	let selectedIds = $state<string[]>([]);
	let deletingSelected = $state(false);

	/** Unsubscribe fn returned by the SDK for the active collection's record events */
	let unsubRecordEvents: (() => void) | null = null;
	/** Last collection name the effect processed (avoids duplicate loads/subscribes). */
	let lastHandledName = $state('');

	/**
	 * Human-readable label for an expanded related record. Picks the target
	 * collection's presentable field (or name/title/email), falling back to
	 * the raw id — mirrors RecordForm's relation label logic.
	 */
	function relationLabel(field: Record<string, unknown>, rec: Record<string, unknown>): string {
		const opts = (field.options ?? {}) as Record<string, unknown>;
		const target = (opts['collection'] as string | undefined) ?? '';
		const targetColl = $collections.find((c) => c.name === target);
		const targetFields = ((targetColl?.fields as Record<string, unknown>[]) ?? []) as Record<
			string,
			unknown
		>[];
		const presentable =
			targetFields.find((f) => f.presentable && f.name !== 'id') ??
			targetFields.find((f) => f.name === 'name' || f.name === 'title' || f.name === 'email');
		const id = String(rec['id'] ?? '');
		if (presentable) {
			const val = rec[presentable.name as string];
			if (val != null && val !== '') return `${String(val)} (${id.slice(0, 8)}...)`;
		}
		return id;
	}

	function formatValue(
		field: Record<string, unknown>,
		value: unknown,
		record?: Record<string, unknown>
	): string {
		if (value == null) return '—';
		if (field.type === 'bool') return value ? '✓' : '✗';
		if (field.type === 'relation') {
			// Prefer the expanded related record (the API returns it when the
			// list request carries `expand`), so the table shows the related
			// record's label instead of a raw id.
			const expanded = (record?.expand as Record<string, unknown> | undefined)?.[
				field.name as string
			] as Record<string, unknown> | undefined;
			if (expanded) return relationLabel(field, expanded);
		}
		if (typeof value === 'object') return JSON.stringify(value).slice(0, 50);
		return String(value);
	}

	let columns = $derived.by(() => {
		const cols: { key: string; label: string; render: (r: Record<string, unknown>) => string }[] = [
			{ key: 'id', label: 'ID', render: (r) => (r.id as string) ?? '' }
		];
		const fields = ((collection?.fields as Record<string, unknown>[]) ?? [])
			.filter((f) => !f.hidden && f.type !== 'password')
			.toSorted((a, b) => ((a.sort_order as number) ?? 0) - ((b.sort_order as number) ?? 0));
		for (const f of fields) {
			const isFile = f.type === 'file' || f.type === 'multi_file';
			const opts = (f.options ?? {}) as Record<string, unknown>;
			const thumbs = Array.isArray(opts.thumbs) ? (opts.thumbs as string[]) : [];
			// Smallest thumb size (shortest string) — e.g. "50x50"
			const thumbSize = thumbs.length ? [...thumbs].sort((a, b) => a.length - b.length)[0] : null;
			cols.push({
				key: f.name as string,
				label: f.name as string,
				render: (r) => formatValue(f, r[f.name as string], r),
				...(isFile && thumbSize
					? {
							thumbs: (r: Record<string, unknown>) => {
								const v = r[f.name as string];
								const ids = Array.isArray(v) ? (v as string[]) : v ? [v as string] : [];
								return ids.slice(0, 3).map((fid) => getThumbUrl('/api', fid, thumbSize));
							}
						}
					: {})
			});
		}
		return cols;
	});

	async function loadCollection(name: string, targetPage: number = currentPage) {
		loading = true;
		try {
			// Fetch the collection first so we know which relation fields to
			// expand in the record list request.
			const coll = await client.collections.getOne(name);
			if (!coll) return;
			collection = coll;
			const recs = await client.collection(name).getList(targetPage, perPage, {
				// Always request every field explicitly: the SDK otherwise projects
				// responses against the static codegen schema snapshot
				// (lazypock.types.ts), which goes stale the moment a field is added
				// or removed via the Studio. The server applies strict projection,
				// so new fields (e.g. a select field added to `users`) would be
				// silently dropped from the table and the edit pane. `*` = all
				// non-password fields (password stripping happens server-side).
				// Relation fields are expanded so the table shows the related
				// record's label instead of a raw id.
				fields: '*',
				expand: relationFieldNames(coll)
			});
			rows = (recs?.items as Record<string, unknown>[]) || [];
			totalItems = recs?.totalItems ?? 0;
			totalPages = Math.max(1, recs?.totalPages ?? 1);
			currentPage = Math.min(targetPage, totalPages);
		} catch (e) {
			console.error('load collection:', e);
		} finally {
			loading = false;
		}
	}

	/** Comma-separated names of the collection's relation fields (for `expand`). */
	function relationFieldNames(coll: Record<string, unknown>): string {
		return ((coll.fields as Record<string, unknown>[]) ?? [])
			.filter((f) => f.type === 'relation')
			.map((f) => f.name as string)
			.join(',');
	}

	// Sync activeName from URL param (or first collection)
	onMount(async () => {
		const params = new URLSearchParams(window.location.search);
		const fromParam = params.get('collection');
		const list = $collections.length > 0 ? $collections : [];
		$activeName = fromParam ?? (list[0]?.name as string | undefined) ?? '';
	});

	// Re-load when the URL param or active collection changes
	$effect(() => {
		const name = page.url.searchParams.get('collection') ?? $activeName;
		if (!name) return;

		if (name !== $activeName) {
			$activeName = name;
		}

		// Only reload + resubscribe when the collection actually changed
		if (name === lastHandledName) return;
		lastHandledName = name;
		currentPage = 1;

		loadCollection(name);

		// Subscribe to record events for this collection using the SDK's
		// higher-level collection subscription (auto-connects + normalises
		// events to {action, record}). Returns an unsubscriber we call when
		// the active collection changes.
		if (unsubRecordEvents) {
			unsubRecordEvents();
		}
		unsubRecordEvents = client.collection(name).subscribe((e) => {
			if (e.action === 'create' || e.action === 'update' || e.action === 'delete') {
				// Reload records for the active collection
				loadCollection(name);
			}
		});
	});

	// View collections are read-only: no record create/update/delete.
	const isViewCollection = $derived((collection?.type as string) === 'view');

	// ── Record CRUD ──
	function newRecord() {
		if (isViewCollection) return;
		editingRecordId = null;
		recordData = {};
		showRecordPane = true;
	}

	function editRecord(row: Record<string, unknown>) {
		if (isViewCollection) return;
		editingRecordId = (row.id as string) ?? null;
		// Copy from live fields — skip password fields
		const schemaFields = (collection?.fields as Record<string, unknown>[]) ?? [];
		recordData = {};
		for (const f of schemaFields) {
			if (f.type === 'password') continue;
			const name = f.name as string;
			if (name in row) {
				recordData[name] = row[name];
			}
		}
		showRecordPane = true;
	}

	function reload() {
		if ($activeName) loadCollection($activeName, currentPage);
	}

	// ── Pagination ──
	function goPage(target: number) {
		if (target < 1 || target > totalPages || target === currentPage || !$activeName) return;
		loadCollection($activeName, target);
	}

	function changePerPage() {
		if ($activeName) loadCollection($activeName, 1);
	}

	function editCollection() {
		// Open the collection editor in a right-side pane (not a full page).
		collectionEditName = $activeName;
		showCollectionPane = true;
	}

	const collName = $derived((collection?.name as string) ?? '');

	function downloadSelected() {
		const selected = rows.filter((r) => selectedIds.includes(r.id as string));
		const blob = new Blob([JSON.stringify(selected, null, 2)], { type: 'application/json' });
		const url = URL.createObjectURL(blob);
		const a = document.createElement('a');
		a.href = url;
		a.download = `${collName || 'records'}-selected.json`;
		a.click();
		URL.revokeObjectURL(url);
	}

	async function deleteSelected() {
		if (!collection || selectedIds.length === 0 || deletingSelected) return;
		if (
			!confirm(
				`Delete ${selectedIds.length} selected record${selectedIds.length > 1 ? 's' : ''}? This action cannot be undone.`
			)
		)
			return;
		deletingSelected = true;
		try {
			const collName = collection.name as string;
			for (const id of selectedIds) {
				await client.collection(collName).delete(id);
			}
			selectedIds = [];
			reload();
		} catch (e) {
			console.error('delete selected:', e);
			alert('Failed to delete some records: ' + ((e as Error).message ?? String(e)));
		} finally {
			deletingSelected = false;
		}
	}
</script>

{#if loading}
	<div
		class="flex min-h-[200px] flex-1 flex-col items-center justify-center gap-2 text-base-content/40"
	>
		<p class="text-sm">Loading...</p>
	</div>
{:else if collection}
	<div class="flex h-full flex-col">
		<div class="mb-3 flex items-center justify-between pb-3">
			<nav class="flex items-center gap-2">
				<span class="text-base-content/50">Collections</span>
				<span class="text-xs opacity-30">/</span>
				<span class="font-medium">{(collection.name as string) ?? '...'}</span>
				{#if isViewCollection}
					<span
						class="rounded bg-info/20 px-1.5 py-0.5 text-[10px] font-medium tracking-wide text-info uppercase"
						>view</span
					>
				{/if}
				<button
					type="button"
					class="ml-2 cursor-pointer rounded border-none bg-transparent p-0.5 text-base-content opacity-40 transition-opacity hover:opacity-100"
					onclick={editCollection}
					title="Edit collection"
				>
					<Settings class="h-4 w-4" />
				</button>
			</nav>
			{#if !isViewCollection}
				<Button class="btn-primary w-fit" onclick={newRecord}><Plus size={18} /> New Record</Button>
			{:else}
				<span class="text-sm text-base-content/50">Read-only view</span>
			{/if}
		</div>

		<div class="min-h-0">
			<DataTable
				{columns}
				{rows}
				fillHeight
				selectable={!isViewCollection}
				bind:selectedIds
				emptyLabel="No records yet. Create your first record to get started."
				emptyActionLabel={isViewCollection ? '' : '+ New Record'}
				onemptyaction={isViewCollection ? undefined : newRecord}
				onrowclick={(row) => editRecord(row)}
			>
				{#snippet selectionActions()}
					<Button class="btn-ghost btn-sm" onclick={downloadSelected}>Download JSON</Button>
					<Button class="btn-error btn-sm" loading={deletingSelected} onclick={deleteSelected}>
						Delete
					</Button>
				{/snippet}
			</DataTable>
		</div>

		{#if totalPages > 1}
			<div class="mt-3 flex items-center justify-between gap-3">
				<div class="text-sm text-base-content/60">
					{totalItems} record{totalItems === 1 ? '' : 's'}
				</div>
				<div class="flex items-center gap-2">
					<Select
						options={[
							{ value: 10, label: '10 / page' },
							{ value: 25, label: '25 / page' },
							{ value: 50, label: '50 / page' },
							{ value: 100, label: '100 / page' }
						]}
						bind:value={perPage}
						onchange={changePerPage}
					/>
					<Button
						class="btn-ghost btn-sm"
						disabled={currentPage <= 1}
						onclick={() => goPage(currentPage - 1)}>Previous</Button
					>
					<span class="text-sm text-base-content/60">
						Page {currentPage} of {totalPages}
					</span>
					<Button
						class="btn-ghost btn-sm"
						disabled={currentPage >= totalPages}
						onclick={() => goPage(currentPage + 1)}>Next</Button
					>
				</div>
			</div>
		{/if}
	</div>
{:else}
	<p class="text-sm text-base-content/50">Select a collection from the sidebar.</p>
{/if}

<!-- Record create/edit SidePane -->
<RecordSidePane
	bind:show={showRecordPane}
	{collection}
	collections={$collections}
	bind:recordData
	bind:editingRecordId
	onSaved={reload}
	onDeleted={reload}
/>

<!-- Collection edit SidePane (right-side pane instead of a full page) -->
<SidePane
	bind:show={showCollectionPane}
	title="Edit Collection"
	closable={false}
	onCloseRequest={() => {
		// Block external closes (Esc/backdrop) — the editor decides via its own
		// dirty-check requestClose. It calls onClose() when it's safe to close.
		return false;
	}}
>
	{#snippet headerExtra()}
		<button
			type="button"
			class="btn btn-ghost btn-sm px-2"
			title="Delete collection"
			onclick={() => (deleteConfirmOpen = true)}
		>
			<Trash size={16} class="text-error" />
		</button>
	{/snippet}
	<CollectionEditor
		editingCollectionId={collectionEditName || null}
		existingName={collectionEditName}
		bind:showDeleteConfirm={deleteConfirmOpen}
		onClose={() => {
			showCollectionPane = false;
			reload();
		}}
	/>
</SidePane>
