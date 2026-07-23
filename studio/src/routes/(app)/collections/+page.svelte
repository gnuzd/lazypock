<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import { slide } from 'svelte/transition';
	import Dropdown from '$lib/components/Dropdown.svelte';
	import { Folder, Settings } from '@lucide/svelte';
	import { setSidebar } from '$lib/sidebar.svelte';
	import DataTable from '$lib/components/DataTable.svelte';
	import SidePane from '$lib/components/SidePane.svelte';
	import FieldSettings from '$lib/components/FieldSettings.svelte';
	import NewFieldButton from '$lib/components/NewFieldButton.svelte';
	import RuleField from '$lib/components/RuleField.svelte';
	import { slugify } from '$lib/fieldTypes';
	import type { FieldDefinition } from '$lib/fieldTypes';
	import RecordForm from '$lib/components/RecordForm.svelte';

	let collections = $state<Record<string, unknown>[]>([]);
	let collection = $state<Record<string, unknown> | null>(null);
	let rows = $state<Record<string, unknown>[]>([]);
	let loading = $state(false);
	let activeName = $state('');
	let search = $state('');

	// ── Record CRUD state ──
	let showRecordPane = $state(false);
	let recordData = $state<Record<string, unknown>>({});
	let editingRecordId = $state<string | null>(null);
	let recordSaving = $state(false);
	let recordError = $state('');

	let filtered = $derived.by(() => {
		if (!search) return collections;
		const q = search.toLowerCase();
		return collections.filter((c) => (c.name as string)?.toLowerCase().includes(q));
	});

	onMount(async () => {
		try {
			const result = await client.listCollections('page=1&perPage=200');
			collections = result?.items || [];
			const params = new URLSearchParams(window.location.search);
			activeName = params.get('collection') ?? (collections?.[0]?.name as string | undefined) ?? '';
			if (activeName) loadCollection(activeName);
		} catch {
			// ignore
		}

		// Subscribe to admin:collections for collection CRUD events
		client.realtime.subscribe('admin:collections', (e) => {
			if (e.event === 'collection_change') {
				// Reload the collection list on any admin change
				client.listCollections('page=1&perPage=200').then((res) => {
					collections = res?.items || [];
				});
			}
		});
	});

	/** Track which collection topic we're subscribed to for record events */
	let subscribedRecordTopic = $state('');

	function selectCollection(name: string) {
		activeName = name;
		history.replaceState(null, '', '?collection=' + encodeURIComponent(name));
		loadCollection(name);
	}

	/** Subscribe to record events when active collection changes */
	$effect(() => {
		if (!activeName) return;

		// Unsubscribe from previous topic
		if (subscribedRecordTopic) {
			client.realtime.unsubscribe(subscribedRecordTopic);
		}

		const topic = 'collection:' + activeName;
		subscribedRecordTopic = topic;

		client.realtime.subscribe(topic, (e) => {
			if (e.event === 'record_change') {
				// Reload records for the active collection
				loadCollection(activeName);
			}
		});
	});

	async function loadCollection(name: string) {
		loading = true;
		try {
			const [coll, recs] = await Promise.all([
				client.getCollection(name),
				client.listRecords(name, { page: '1', perPage: '50' })
			]);
			collection = coll;
			rows = (recs?.items as Record<string, unknown>[]) || [];
		} catch (e) {
			console.error('load collection:', e);
		} finally {
			loading = false;
		}
	}

	function formatValue(field: Record<string, unknown>, value: unknown): string {
		if (value == null) return '—';
		if (field.type === 'bool') return value ? '✓' : '✗';
		if (typeof value === 'object') return JSON.stringify(value).slice(0, 50);
		return String(value);
	}

	let columns = $derived.by(() => {
		const cols: { key: string; label: string; render: (r: Record<string, unknown>) => string }[] = [
			{ key: 'id', label: 'ID', render: (r) => ((r.id as string)?.slice(0, 8) ?? '') + '...' }
		];
		for (const field of (collection?.schema as Record<string, unknown>[]) ?? []) {
			const f = field;
			cols.push({
				key: f.name as string,
				label: f.name as string,
				render: (r) => formatValue(f, r[f.name as string])
			});
		}
		return cols;
	});

	// ── New/Edit Collection form ──
	let showCollectionPane = $state(false);
	let editingCollectionId = $state<string | null>(null);
	let newName = $state('');
	let newType = $state('base');
	let typeOpen = $state(false);
	let newFields = $state<FieldDefinition[]>([]);
	let newIndexes = $state<string[]>([]);
	let activeTab = $state('Fields');
	let saving = $state(false);
	let error = $state('');
	let fieldsListEl = $state<HTMLDivElement | undefined>(undefined);
	let dragIndex = $state<number | null>(null);
	let dropIndex = $state<number | null>(null);
	let listRule = $state<string | null>(null);
	let viewRule = $state<string | null>(null);
	let createRule = $state<string | null>(null);
	let updateRule = $state<string | null>(null);
	let deleteRule = $state<string | null>(null);
	let showRulesInfo = $state(false);

	const collectionTypes = [
		{ value: 'base', label: 'Base collection' },
		{ value: 'view', label: 'View collection' },
		{ value: 'auth', label: 'Auth collection' },
	];

	function getTypeLabel(val: string) {
		return collectionTypes.find(t => t.value === val)?.label ?? val;
	}

	function handleNameInput(e: Event) {
		if ((e as InputEvent).isComposing) return;
		const raw = (e.target as HTMLInputElement).value;
		const slugged = slugify(raw);
		if (slugged && slugged !== newName) {
			newName = slugged;
		} else if (!slugged) {
			newName = raw;
		}
	}

	function handleNameCompositionEnd(e: Event) {
		newName = (e.target as HTMLInputElement).value;
	}

	function handleKeydown(e: KeyboardEvent) {
		if ((e.ctrlKey || e.metaKey) && e.key === 's') {
			e.preventDefault();
			handleSave();
		}
	}

	function closestChild(parent: HTMLElement, node: Node | null): HTMLElement | null {
		if (!node || !node.parentNode) return null;
		if (node.parentNode === parent) return node as HTMLElement;
		return closestChild(parent, node.parentNode);
	}

	$effect(() => {
		const el = fieldsListEl;
		if (!el) return;

		/** Get where to insert: index in the array (e.g. 0 = before first, length = after last). */
		function getInsertIndex(clientY: number): number {
			const _el = el as HTMLElement;
			const items = [..._el.children].filter((c) => c.hasAttribute('data-sortable-child'));
			for (let i = 0; i < items.length; i++) {
				const rect = items[i].getBoundingClientRect();
				const mid = rect.top + rect.height / 2;
				if (clientY < mid) return i;
			}
			return items.length;
		}

		function onDragStart(e: DragEvent) {
			const _el = el as HTMLElement;
			const items = [..._el.children].filter((c) => c.hasAttribute('data-sortable-child'));
			e.dataTransfer!.setData('text/plain', '');
			e.dataTransfer!.effectAllowed = 'move';
			const child = closestChild(_el, e.target as Node);
			dragIndex = child ? items.indexOf(child) : -1;
		}
		function onDragOver(e: DragEvent) {
			e.preventDefault();
			e.dataTransfer!.dropEffect = 'move';
			dropIndex = getInsertIndex(e.clientY);
		}
		function onDragLeave(e: DragEvent) {
			// Only clear when actually leaving the container, not between children
			if (!e.relatedTarget || !(el as HTMLElement).contains(e.relatedTarget as Node)) {
				dropIndex = null;
			}
		}
		function onDrop() {
			if (dragIndex == null || dropIndex == null || dragIndex === dropIndex) {
				reset();
				return;
			}
			const clone = [...newFields];
			const [moved] = clone.splice(dragIndex, 1);
			clone.splice(dropIndex, 0, moved);
			newFields = clone;
			reset();
		}
		function reset() {
			dragIndex = null;
			dropIndex = null;
		}

		const _el = el as HTMLElement;
		_el.addEventListener('dragstart', onDragStart);
		_el.addEventListener('dragover', onDragOver);
		_el.addEventListener('dragleave', onDragLeave);
		_el.addEventListener('drop', onDrop);
		_el.addEventListener('dragend', reset);
		return () => {
			_el.removeEventListener('dragstart', onDragStart);
			_el.removeEventListener('dragover', onDragOver);
			_el.removeEventListener('dragleave', onDragLeave);
			_el.removeEventListener('drop', onDrop);
			_el.removeEventListener('dragend', reset);
		};
	});

	function newCollection() {
		// Reset form state
		editingCollectionId = null;
		newName = '';
		newType = 'base';
		newFields = [];
		newIndexes = [];
		activeTab = 'Fields';
		error = '';
		listRule = null;
		viewRule = null;
		createRule = null;
		updateRule = null;
		deleteRule = null;
		showCollectionPane = true;
	}

	function editCollection(coll: Record<string, unknown>) {
		editingCollectionId = (coll.id as string) ?? null;
		newName = (coll.name as string) ?? '';
		newType = (coll.type as string) ?? 'base';
		newFields = (((coll.fields as Record<string, unknown>[]) ?? []).map((f) => ({
			...f,
			id: (f.id as string) ?? crypto.randomUUID()
		})) as unknown) as FieldDefinition[];
		newIndexes = [];
		activeTab = 'Fields';
		error = '';
		listRule = (coll.rules as Record<string, unknown>)?.['listRule'] as string | null ?? null;
		viewRule = (coll.rules as Record<string, unknown>)?.['viewRule'] as string | null ?? null;
		createRule = (coll.rules as Record<string, unknown>)?.['createRule'] as string | null ?? null;
		updateRule = (coll.rules as Record<string, unknown>)?.['updateRule'] as string | null ?? null;
		deleteRule = (coll.rules as Record<string, unknown>)?.['deleteRule'] as string | null ?? null;
		showCollectionPane = true;
	}

	async function handleSave() {
		if (!newName.trim() || saving) return;
		saving = true;
		error = '';
		try {
			const payload: Record<string, unknown> = {
				name: newName.trim(),
				type: newType,
				fields: newFields.filter(f => !f['@toDelete']).map(f => {
					const clean = { ...f };
					delete clean.__focus;
					delete clean['@toDelete'];
					delete clean._showChoices;
					delete clean._choicesInput;
					return clean;
				}),
				indexes: newIndexes.filter(Boolean),
				listRule,
				viewRule,
				createRule,
				updateRule,
				deleteRule,
			};
			if (editingCollectionId) {
				await client.updateCollection(editingCollectionId, payload);
				// Navigate to the (possibly renamed) collection
				if (newName.trim() !== activeName) {
					selectCollection(newName.trim());
				} else {
					loadCollection(newName.trim());
				}
			} else {
				const created = await client.createCollection(payload);
				if (created?.name) {
					selectCollection(created.name as string);
				}
			}
			showCollectionPane = false;
		} catch (e) {
			error = (e as Error).message || (editingCollectionId ? 'Failed to save collection' : 'Failed to create collection');
		} finally {
			saving = false;
		}
	}

	// ── Record CRUD ──

	function newRecord() {
		editingRecordId = null;
		recordData = {};
		recordError = '';
		showRecordPane = true;
	}

	function editRecord(row: Record<string, unknown>) {
		editingRecordId = (row.id as string) ?? null;
		recordData = { ...row };
		recordError = '';
		showRecordPane = true;
	}

	async function saveRecord() {
		if (recordSaving || !collection) return;
		recordSaving = true;
		recordError = '';
		const collName = collection.name as string;
		try {
			if (editingRecordId) {
				await client.updateRecord(collName, editingRecordId, recordData);
			} else {
				await client.createRecord(collName, recordData);
			}
			showRecordPane = false;
			loadCollection(collName);
		} catch (e) {
			recordError = (e as Error).message || 'Failed to save record';
		} finally {
			recordSaving = false;
		}
	}

	async function deleteRecord() {
		if (!collection || !editingRecordId || recordSaving) return;
		if (!confirm('Delete this record? This action cannot be undone.')) return;
		recordSaving = true;
		recordError = '';
		const collName = collection.name as string;
		try {
			await client.deleteRecord(collName, editingRecordId);
			showRecordPane = false;
			loadCollection(collName);
		} catch (e) {
			recordError = (e as Error).message || 'Failed to delete record';
		} finally {
			recordSaving = false;
		}
	}

	setSidebar(headerContent, bodyContent, footerContent);
</script>

{#snippet headerContent()}
	<input
		type="text"
		class="input input-sm w-full"
		placeholder="Search..."
		bind:value={search}
	/>
{/snippet}

{#snippet bodyContent()}
	{#if filtered.length === 0}
		<div class="p-4 text-center opacity-40 text-sm">No collections</div>
	{:else}
		{#each filtered as coll (coll.id)}
			<div
				class="flex items-center gap-2 w-[calc(100%-12px)] mx-1.5 px-3 py-1.5 border-none rounded-field text-sm text-base-content cursor-pointer text-left transition-[background] duration-(--animation-speed-fast) hover:bg-base-200"
				class:bg-base-200={coll.name === activeName}
				class:font-medium={coll.name === activeName}
				role="button"
				tabindex="0"
				onclick={() => selectCollection(coll.name as string)}
				onkeydown={(e) => { if (e.key === 'Enter') selectCollection(coll.name as string); }}
			>
				<Folder class="w-4 h-4 opacity-60 shrink-0" />
				<span class="truncate flex-1">{coll.name as string}</span>
				<span class="text-xs opacity-40 mr-1">{(coll.schema as unknown[])?.length ?? 0}</span>
				<button
					type="button"
					class="p-0.5 rounded cursor-pointer opacity-30 hover:opacity-100 transition-opacity border-none bg-transparent text-base-content"
					onclick={(e) => { e.stopPropagation(); editCollection(coll); }}
					title="Edit collection"
				>
					<Settings class="w-3.5 h-3.5" />
				</button>
			</div>
		{/each}
	{/if}
{/snippet}

{#snippet footerContent()}
	<button class="btn btn-primary btn-full p-3" onclick={newCollection}>+ New Collection</button>
{/snippet}

<!-- Main content -->
{#if loading}
	<div class="flex flex-col items-center justify-center flex-1 min-h-[200px] text-base-content/40 gap-2">
		<p class="text-sm">Loading...</p>
	</div>
{:else if collection}
	<!-- Breadcrumb -->
	<div class="flex items-center justify-between pb-3 mb-3 border-b border-base-300">
		<nav class="flex items-center gap-1 text-sm">
			<span class="text-base-content/50">Collections</span>
			<span class="text-xs opacity-30">/</span>
			<span class="font-medium">{(collection.name as string) ?? '...'}</span>
		</nav>
		<button class="btn btn-primary btn-sm" onclick={newRecord}>+ New Record</button>
	</div>

	<!-- Table -->
	<DataTable
		{columns}
		{rows}
		emptyLabel="No records yet. Create your first record to get started."
		emptyActionLabel="+ New Record"
		onemptyaction={newRecord}
		onrowclick={(row) => editRecord(row)}
	/>

	<!-- Record create/edit SidePane -->
	<SidePane bind:show={showRecordPane} title={editingRecordId ? 'Edit Record' : 'New Record'}>
		<div class="flex flex-col min-h-0 h-full">
			<div class="flex-1 overflow-y-auto p-4">
				<RecordForm
					fields={((collection?.schema ?? []) as Record<string, unknown>[])}
					bind:data={recordData}
					disabled={recordSaving}
				/>
			</div>

			{#if recordError}
				<div class="shrink-0 px-4 py-2 text-xs text-error bg-error/10 border-t border-base-300">{recordError}</div>
			{/if}

			<div class="shrink-0 flex items-center gap-2 px-4 py-3 border-t border-base-300">
				<button type="button" class="btn btn-ghost mr-auto" onclick={() => showRecordPane = false}>Close</button>
				{#if editingRecordId}
					<button
						type="button"
						class="btn btn-error btn-sm"
						disabled={recordSaving}
						onclick={deleteRecord}
					>Delete</button>
				{/if}
				<button
					type="button"
					class="btn btn-primary"
					class:loading={recordSaving}
					disabled={!Object.keys(recordData).length || recordSaving}
					onclick={saveRecord}
				>
					{editingRecordId ? 'Update' : 'Create'}
				</button>
			</div>
		</div>
	</SidePane>
{/if}

<!-- New/Edit Collection SidePane -->
<SidePane bind:show={showCollectionPane} title={editingCollectionId ? 'Edit Collection' : 'New Collection'}>
	<div class="flex flex-col min-h-0 h-full">
		<!-- Header: Name + Type row -->
		<div class="shrink-0 p-4 pb-3">
			<div class="flex gap-0">
				<div class="field flex-1 min-w-0">
					<label for="coll-name" class="block text-xs font-medium text-base-content/70 mb-1">Name</label>
					<input
						id="coll-name"
						type="text"
						name="name"
						required
						spellcheck="false"
						class="input input-sm w-full"
						placeholder="e.g. posts"
						value={newName}
						oninput={handleNameInput}
						oncompositionend={handleNameCompositionEnd}
						onkeydown={handleKeydown}
						autofocus
					/>
				</div>
				<Dropdown bind:show={typeOpen} class="shrink-0">
					{#snippet trigger()}
						<label class="block text-xs font-medium text-base-content/70 mb-1">&nbsp;</label>
						<button
							type="button"
							class="btn btn-sm border-2 border-current bg-transparent text-base-content rounded-field flex items-center gap-2 whitespace-nowrap px-3 h-[34px]"
						>
							<span>Type: {getTypeLabel(newType)}</span>
							<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="ml-auto"><polyline points="6 9 12 15 18 9"/></svg>
						</button>
					{/snippet}
					<div class="min-w-[200px] py-1">
						{#each collectionTypes as ct (ct.value)}
							<button
								type="button"
								class="flex items-center gap-2 w-full px-3 py-2 text-sm text-left cursor-pointer border-none bg-transparent text-base-content rounded-field hover:bg-base-200"
								class:font-medium={ct.value === newType}
								onmousedown={() => { newType = ct.value; typeOpen = false; }}
							>
								<span>{ct.label}</span>
							</button>
						{/each}
					</div>
				</Dropdown>
			</div>
		</div>

		<!-- Header: Tabs (separate section with bottom border) -->
		<nav class="shrink-0 flex gap-0 border-b border-base-300 px-4">
			<button
				type="button"
				class="px-3 py-2 text-sm border-b-2 transition-colors cursor-pointer {activeTab === 'Fields' ? 'border-primary text-primary' : 'border-transparent text-base-content/60'}"
				onclick={() => activeTab = 'Fields'}
			>Fields</button>
			<button
				type="button"
				class="px-3 py-2 text-sm border-b-2 transition-colors cursor-pointer {activeTab === 'API rules' ? 'border-primary text-primary' : 'border-transparent text-base-content/60'}"
				onclick={() => activeTab = 'API rules'}
			>API rules</button>
		</nav>

		<!-- Tab content -->
			<div class="flex-1 overflow-y-auto p-4">
			{#if activeTab === 'Fields'}
				<!-- Fields list with drag-reorder -->
				<div class="space-y-1" bind:this={fieldsListEl}>
					{#each newFields as field, i (field)}
						{#if dropIndex === i}
							<div class="h-1 bg-primary rounded-field" transition:slide={{ duration: 100 }}></div>
						{/if}
						<div data-sortable-child class:opacity-40={dragIndex === i}>
							<FieldSettings field={newFields[i]} fieldIndex={i} collections={collections} />
						</div>
					{/each}
					{#if dropIndex === newFields.length}
						<div class="h-1 bg-primary rounded-field" transition:slide={{ duration: 100 }}></div>
					{/if}
				</div>

				{#if newFields.length === 0}
					<div class="text-center py-6 opacity-40 text-sm">No fields configured. Click "Add Field" to start.</div>
				{/if}

				<NewFieldButton bind:fields={newFields} />

				<hr class="border-t border-base-300 my-3" />

				<p class="text-xs font-semibold mb-2">Unique constraints and indexes ({newIndexes.length})</p>
				<div class="flex flex-wrap gap-1 mb-2">
					{#each newIndexes as idx, i (idx + '-' + i)}
						<button type="button" class="inline-flex items-center gap-1 px-2 py-1 text-xs rounded bg-base-200 border border-base-300 cursor-pointer"
							onclick={() => { newIndexes.splice(i, 1); newIndexes = [...newIndexes]; }}
						>
							{#if idx.startsWith('UNIQUE')}
								<strong>Unique:</strong>
							{/if}
							<span>{idx.replace(/^UNIQUE\s+/, '')}</span>
							<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="opacity-60"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
						</button>
					{/each}
					<button type="button" class="inline-flex items-center gap-1 px-2 py-1 text-xs rounded bg-base-200 border border-base-300 cursor-pointer" onclick={() => newIndexes = [...newIndexes, '']}>
						<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
						<span>New index</span>
					</button>
				</div>
			{:else}
				<!-- API Rules tab -->
				<div class="flex flex-col gap-2">
					<div class="flex items-center gap-1 text-xs text-base-content/60">
						<span>All rules follow the <a target="_blank" rel="noopener noreferrer" href="https://pocketbase.io/docs/api-rules-and-filters/" class="underline">PocketBase filter syntax and operators</a>.</span>
						<strong tabindex="-1" class="ml-auto cursor-pointer hover:underline" onclick={() => showRulesInfo = !showRulesInfo}>
							{showRulesInfo ? 'Hide available fields' : 'Show available fields'}
						</strong>
					</div>

					{#if showRulesInfo}
						<div class="p-3 text-xs bg-warning/10 border border-warning/30 rounded-field">
							<p class="mb-1">The following record fields are available:</p>
							<div class="flex flex-wrap gap-1 mb-2">
								{#each newFields.filter(f => !f['@toDelete']) as field (field.name)}
									<code class="px-1 py-0.5 bg-base-300 rounded text-xs">{field.name}</code>
								{/each}
							</div>
							<hr class="my-2 border-base-300" />
							<p class="mb-1">The request fields could be accessed with the special <strong>@request</strong> fields:</p>
							<div class="flex flex-wrap gap-1 mb-2">
								<code class="px-1 py-0.5 bg-base-300 rounded text-xs">@request.headers.*</code>
								<code class="px-1 py-0.5 bg-base-300 rounded text-xs">@request.query.*</code>
								<code class="px-1 py-0.5 bg-base-300 rounded text-xs">@request.body.*</code>
								<code class="px-1 py-0.5 bg-base-300 rounded text-xs">@request.auth.*</code>
							</div>
							<hr class="my-2 border-base-300" />
							<p class="mb-1">You could also add constraints using <strong>@collection</strong>:</p>
							<div class="flex flex-wrap gap-1">
								<code class="px-1 py-0.5 bg-base-300 rounded text-xs">@collection.ANY_COLLECTION_NAME.*</code>
							</div>
							<hr class="my-2 border-base-300" />
							<p class="mb-1">Example rule:</p>
							<code class="px-1 py-0.5 bg-base-300 rounded text-xs">@request.auth.id != ""</code>
						</div>
					{/if}

					<RuleField label="List/Search rule" name="listRule" bind:value={listRule} />
					<RuleField label="View rule" name="viewRule" bind:value={viewRule} />
					<RuleField label="Create rule" name="createRule" bind:value={createRule} />
					<RuleField label="Update rule" name="updateRule" bind:value={updateRule} />
					<RuleField label="Delete rule" name="deleteRule" bind:value={deleteRule} />
				</div>
			{/if}
		</div>

		{#if error}
			<div class="shrink-0 px-4 py-2 text-xs text-error bg-error/10 border-t border-base-300">{error}</div>
		{/if}

		<!-- Footer -->
		<div class="shrink-0 flex items-center gap-2 px-4 py-3 border-t border-base-300">
			<button type="button" class="btn btn-ghost mr-auto" onclick={() => showCollectionPane = false}>Close</button>
			{#if error}
				<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-error"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>
			{/if}
			<button
				type="button"
				class="btn btn-primary expanded-lg"
				class:loading={saving}
				disabled={!newName.trim() || saving}
				onclick={handleSave}
			>
				{editingCollectionId ? 'Save' : 'Create'}
			</button>
		</div>
	</div>
</SidePane>

<style>
	/* sortable children need pointer-events to catch drag enter/leave */
	[data-sortable-child] {
		pointer-events: auto;
	}
</style>
