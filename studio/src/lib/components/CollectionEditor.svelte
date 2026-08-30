<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import { slide } from 'svelte/transition';
	import Dropdown from '$lib/components/Dropdown.svelte';
	import Button from '$lib/components/Button.svelte';
	import FieldSettings from '$lib/components/FieldSettings.svelte';
	import IndexesModal from '$lib/components/IndexesModal.svelte';
	import Input from '$lib/components/Input.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import NewFieldButton from '$lib/components/NewFieldButton.svelte';
	import RuleField from '$lib/components/RuleField.svelte';
	import { slugify } from '$lib/fieldTypes';
	import type { FieldDefinition } from '$lib/fieldTypes';
	import { collectionSchema } from '$lib/validation';
	import { goto } from '$app/navigation';
	import { base } from '$app/paths';
	import { collections, loadCollections } from '$lib/collectionsStore';
	import { Check } from '@lucide/svelte';

	/**
	 * New/Edit collection editor. Shared by /collections/[name] (edit) and
	 * /collections/new (create). Owns all form state and the save action.
	 */
	let {
		editingCollectionId = null as string | null,
		existingName = '',
		showDeleteConfirm = $bindable(false),
		onClose
	}: {
		/** The collection id when editing an existing collection, null when creating. */
		editingCollectionId?: string | null;
		/** The current name (used to detect renames). */
		existingName?: string;
		/** Bindable — set to true (e.g. from a pane header) to open the delete confirm. */
		showDeleteConfirm?: boolean;
		/**
		 * When set (embedded in a side pane), Close button and a successful save
		 * call this instead of navigating away. When unset (page route), the
		 * editor keeps its current navigate-on-save behavior.
		 */
		onClose?: () => void;
	} = $props();

	// ── Form state ──
	let newName = $state(existingName);
	let newType = $state('base');
	let typeOpen = $state(false);
	let newFields = $state<FieldDefinition[]>([]);
	let newIndexes = $state<string[]>([]);
	// ── View collections: query + live dry-run state (PocketBase parity) ──
	let viewQuery = $state('');
	let dryRunState = $state<'idle' | 'testing' | 'ok' | 'error'>('idle');
	let dryRunSample = $state<Record<string, unknown>[]>([]);
	let dryRunFields = $state<FieldDefinition[]>([]);
	let dryRunError = $state('');
	let dryRunTimer: ReturnType<typeof setTimeout> | undefined = undefined;
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
	let manageRule = $state<string | null>(null);
	let showRulesInfo = $state(false);
	let showIndexesModal = $state(false);
	let showCloseConfirm = $state(false);
	let deleting = $state(false);
	const collectionTypes = [
		{ value: 'base', label: 'Base collection' },
		{ value: 'view', label: 'View collection' },
		{ value: 'auth', label: 'Auth collection' }
	];

	function getTypeLabel(val: string) {
		return collectionTypes.find((t) => t.value === val)?.label ?? val;
	}

	function _goto(path: string) {
		// eslint-disable-next-line svelte/no-navigation-without-resolve
		goto(base + path);
	}

	// Load existing collection into the form
	onMount(async () => {
		if (!editingCollectionId) return;
		try {
			const coll = await client.collections.getOne(editingCollectionId);
			if (!coll) return;
			newName = (coll.name as string) ?? '';
			newType = (coll.type as string) ?? 'base';
			newFields = ((coll.fields as Record<string, unknown>[]) ?? [])
				.map((f): Record<string, unknown> => ({
					...f,
					id: (f.id as string) ?? crypto.randomUUID()
				}))
				.sort(
					(a, b) => ((a.sort_order as number) ?? 0) - ((b.sort_order as number) ?? 0)
				) as unknown as FieldDefinition[];
			newIndexes = ((coll.indexes as string[]) ?? []).filter(Boolean);
			viewQuery = (coll.viewQuery as string) ?? '';
			const rules = (coll.rules as Record<string, unknown>) ?? {};
			listRule = (rules['listRule'] as string | null) ?? null;
			viewRule = (rules['viewRule'] as string | null) ?? null;
			createRule = (rules['createRule'] as string | null) ?? null;
			updateRule = (rules['updateRule'] as string | null) ?? null;
			deleteRule = (rules['deleteRule'] as string | null) ?? null;
			manageRule = (rules['manageRule'] as string | null) ?? null;

			// After loading, capture the pristine snapshot for dirty detection.
			queueMicrotask(() => {
				loadedSnapshot = snapshot();
			});
		} catch {
			// failed to load — leave form at defaults
		}
	});

	// ── Name input (slugify, IME-safe) ──
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

	// ── Dirty tracking + close confirm ──
	// A snapshot of the loaded state; any divergence means unsaved changes.
	let loadedSnapshot = $state('');

	function snapshot(): string {
		return JSON.stringify({
			name: newName,
			type: newType,
			viewQuery: newType === 'view' ? viewQuery : '',
			fields: newFields
				.filter((f) => !f['@toDelete'])
				.map((f) => {
					const { id: _id, ...rest } = f as Record<string, unknown>;
					void _id;
					return rest;
				}),
			indexes: newIndexes,
			listRule,
			viewRule,
			createRule,
			updateRule,
			deleteRule,
			manageRule
		});
	}

	const isDirty = $derived(loadedSnapshot !== '' && loadedSnapshot !== snapshot());

	function requestClose() {
		if (isDirty) {
			showCloseConfirm = true;
			return false;
		}
		doClose();
		return true;
	}

	function doClose() {
		showCloseConfirm = false;
		if (onClose) onClose();
		else _goto('/collections');
	}

	async function deleteCollection() {
		if (!editingCollectionId || deleting) return;
		deleting = true;
		error = '';
		try {
			await client.collections.delete(editingCollectionId);
			await loadCollections();
			showDeleteConfirm = false;
			if (onClose) onClose();
			else _goto('/collections');
		} catch (e) {
			error = (e as Error).message || 'Failed to delete collection';
		} finally {
			deleting = false;
		}
	}

	// ── Drag-reorder fields ──
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

	// ── View query live dry-run (debounced 200ms, PocketBase parity) ──
	$effect(() => {
		if (newType !== 'view') return;
		const q = viewQuery;
		clearTimeout(dryRunTimer);
		if (!q.trim()) {
			dryRunState = 'idle';
			dryRunSample = [];
			dryRunFields = [];
			dryRunError = '';
			return;
		}
		dryRunState = 'testing';
		dryRunTimer = setTimeout(async () => {
			try {
				const res = (await client.http.post('/collections/meta/dry-run-view', {
					query: q
				})) as { fields: FieldDefinition[]; sample: Record<string, unknown>[] };
				dryRunFields = (res?.fields as FieldDefinition[]) ?? [];
				dryRunSample = (res?.sample as Record<string, unknown>[]) ?? [];
				dryRunError = '';
				dryRunState = 'ok';
			} catch (e) {
				dryRunError =
					((e as { message?: string })?.message ?? '').replace(/^Invalid view query\. Raw error: \s*/, '') ||
					'Invalid query';
				dryRunSample = [];
				dryRunFields = [];
				dryRunState = 'error';
			}
		}, 200);
	});

	async function handleSave() {
		if (saving) return;

		// Build payload
		const payload: Record<string, unknown> = {
			name: newName.trim(),
			type: newType,
			...(newType === 'view' ? { viewQuery: viewQuery.trim() } : {}),
			indexes: newIndexes,
			fields: newFields
				.filter((f) => !f['@toDelete'])
				.map((f) => {
					const clean = { ...f };
					delete clean.__focus;
					delete clean['@toDelete'];
					delete clean._showChoices;
					delete clean._choicesInput;
					return clean;
				}),
			listRule,
			viewRule,
			createRule,
			updateRule,
			deleteRule,
			manageRule
		};

		// Validate with zod
		const result = collectionSchema.safeParse(payload);
		if (!result.success) {
			error = result.error.issues[0]?.message || 'Invalid form data';
			return;
		}

		// View collections must have a query (server-side introspection will
		// surface any SQL errors with a clear message).
		if (newType === 'view' && !viewQuery.trim()) {
			error = 'View collections require a SELECT query';
			return;
		}

		saving = true;
		error = '';
		try {
			if (editingCollectionId) {
				await client.collections.update(
					editingCollectionId,
					result.data as unknown as Record<string, unknown>
				);
			} else {
				await client.collections.create(result.data as unknown as Record<string, unknown>);
			}
			await loadCollections();
			// When embedded in a pane, close it on success instead of navigating away.
			if (onClose) {
				onClose();
			} else {
				// Navigate to the collection list (or the renamed collection)
				// eslint-disable-next-line svelte/no-navigation-without-resolve
				goto(base + '/collections?collection=' + encodeURIComponent(result.data.name));
			}
		} catch (e) {
			error =
				(e as Error).message ||
				(editingCollectionId ? 'Failed to save collection' : 'Failed to create collection');
		} finally {
			saving = false;
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<!-- Header: Name + Type row -->
	<div class="shrink-0 p-4 pb-3">
		<div class="flex items-end gap-0">
			<Input
				id="coll-name"
				label="Name"
				class="bg-transparent!"
				placeholder="e.g. posts"
				name="name"
				required
				spellcheck={false}
				bind:value={newName}
				oninput={handleNameInput}
				oncompositionend={handleNameCompositionEnd}
				onkeydown={handleKeydown}
				autofocus
			/>
			<Dropdown bind:show={typeOpen} class="shrink-0">
				{#snippet trigger()}
					<Button class="btn-outline">
						<span>Type: {getTypeLabel(newType)}</span>
						<svg
							width="16"
							height="16"
							viewBox="0 0 24 24"
							fill="none"
							stroke="currentColor"
							stroke-width="2"
							class="ml-auto"><polyline points="6 9 12 15 18 9" /></svg
						>
					</Button>
				{/snippet}

				<div class="flex flex-col py-1">
					{#each collectionTypes as ct (ct.value)}
						<Button
							class="justify-between font-normal hover:bg-base-200"
							onclick={() => {
								newType = ct.value;
								typeOpen = false;
							}}
						>
							<span>{ct.label}</span>
							{#if ct.value === newType}
								<Check size={16} />
							{/if}
						</Button>
					{/each}
				</div>
			</Dropdown>
		</div>
	</div>

	<!-- Header: Tabs (separate section with bottom border) -->
	<nav class="flex shrink-0 gap-0 border-b border-base-300 px-4">
		<button
			type="button"
			class="cursor-pointer border-b-2 px-3 py-2 text-sm transition-colors {activeTab === 'Fields'
				? 'border-primary text-primary'
				: 'border-transparent text-base-content/60'}"
			onclick={() => (activeTab = 'Fields')}>Fields</button
		>
		<button
			type="button"
			class="cursor-pointer border-b-2 px-3 py-2 text-sm transition-colors {activeTab ===
			'API rules'
				? 'border-primary text-primary'
				: 'border-transparent text-base-content/60'}"
			onclick={() => (activeTab = 'API rules')}>API rules</button
		>
	</nav>

	<!-- Tab content -->
	<div class="flex-1 overflow-y-auto p-4">
		{#if activeTab === 'Fields' && newType === 'view'}
			<!-- View collections: query-driven schema (PocketBase parity) -->
			<div class="flex flex-col gap-3">
				<div class="rounded-field border border-base-300 bg-base-200/40 p-3 text-xs text-base-content/70">
					<p class="mb-1 font-medium text-base-content">Query caveats</p>
					<ul class="list-disc pl-4">
						<li>Wildcard columns (<code>*</code>) are not supported.</li>
						<li>
							The query must have a unique <code>id</code> column. If your query doesn't have a
							suitable one, use
							<code>(ROW_NUMBER() OVER()) as id</code>.
						</li>
						<li>Expressions must be aliased, e.g. <code>MAX(balance) as maxBalance</code>.</li>
						<li>Only a single SELECT statement is allowed.</li>
					</ul>
				</div>

				<div class="flex flex-col gap-1">
					<label for="coll-view-query" class="flex items-center gap-2 text-sm font-medium">
						<span>Select query</span>
						{#if dryRunState === 'testing'}
							<span class="text-xs text-base-content/50">Testing…</span>
						{:else if dryRunState === 'ok'}
							<span class="text-xs text-success">✓ Valid query</span>
						{:else if dryRunState === 'error'}
							<span class="text-xs text-error">✗ Invalid query</span>
						{/if}
					</label>
					<textarea
						id="coll-view-query"
						class="input input-sm h-40 w-full resize-y font-mono text-xs"
						spellcheck={false}
						placeholder="SELECT posts.id, posts.title, count(comments.id) as totalComments FROM posts LEFT JOIN comments ON comments.postId = posts.id GROUP BY posts.id"
						bind:value={viewQuery}
					></textarea>
				</div>

				{#if dryRunError}
					<div class="rounded-field border border-error/30 bg-error/10 p-3 font-mono text-xs text-error">
						{dryRunError}
					</div>
				{/if}

				{#if dryRunState === 'ok' || newFields.length > 0}
					<div class="flex flex-col gap-1">
						<p class="text-sm font-medium">Generated fields</p>
						{#if dryRunState === 'ok'}
							<div class="flex flex-wrap gap-1">
								{#each dryRunFields as f (f.name)}
									<span class="rounded bg-base-300 px-1.5 py-0.5 font-mono text-xs">{f.name}</span>
									<span class="text-xs text-base-content/50">{f.type}</span>
								{/each}
							</div>
						{:else}
							<div class="flex flex-wrap gap-1">
								{#each newFields as f (f.name)}
									<span class="rounded bg-base-300 px-1.5 py-0.5 font-mono text-xs">{f.name}</span>
									<span class="text-xs text-base-content/50">{f.type}</span>
								{/each}
							</div>
						{/if}
						<p class="text-xs text-base-content/50">
							Fields are auto-generated from the query on save — they can't be edited directly.
						</p>
					</div>
				{/if}

				{#if dryRunState === 'ok'}
					<div class="flex flex-col gap-1">
						<p class="text-sm font-medium">Sample output ({dryRunSample.length})</p>
						{#if dryRunSample.length > 0}
							<pre
								class="max-h-56 overflow-auto rounded-field border border-base-300 bg-base-200/60 p-3 font-mono text-xs"
							><code>{JSON.stringify(dryRunSample.slice(0, 3), null, 2)}</code></pre
							>
						{:else}
							<p class="text-xs text-base-content/50">No records match the query.</p>
						{/if}
					</div>
				{/if}
			</div>
		{:else if activeTab === 'Fields'}
			<!-- Fields list with drag-reorder -->
			<div class="space-y-1" bind:this={fieldsListEl}>
				{#each newFields as field, i (field)}
					{#if dropIndex === i}
						<div class="h-1 rounded-field bg-primary" transition:slide={{ duration: 100 }}></div>
					{/if}
					<div data-sortable-child class:opacity-40={dragIndex === i}>
						<FieldSettings field={newFields[i]} fieldIndex={i} collections={$collections} />
					</div>
				{/each}
				{#if dropIndex === newFields.length}
					<div class="h-1 rounded-field bg-primary" transition:slide={{ duration: 100 }}></div>
				{/if}
			</div>

			{#if newFields.length === 0}
				<div class="py-6 text-center text-sm opacity-40">
					No fields configured. Click "Add Field" to start.
				</div>
			{/if}

			<NewFieldButton bind:fields={newFields} />

			<hr class="my-3 border-t border-base-300" />

			<button
				type="button"
				class="btn btn-ghost btn-xs flex w-full items-center justify-start gap-2 text-left"
				onclick={() => (showIndexesModal = true)}
			>
				<svg
					width="14"
					height="14"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2"
					><polyline points="1 4 1 10 7 10" /><path d="M3.51 15a9 9 0 1 0 2.13-9.36L1 10" /></svg
				>
				<span>Unique constraints and indexes ({newIndexes.length})</span>
			</button>
		{:else}
			<!-- API Rules tab -->
			<div class="flex flex-col gap-2">
				<div class="flex items-center gap-1 text-xs text-base-content/60">
					<span
						>All rules follow the <a
							target="_blank"
							rel="noopener noreferrer"
							href="https://pocketbase.io/docs/api-rules-and-filters/"
							class="underline">PocketBase filter syntax and operators</a
						>.</span
					>
					<strong
						tabindex="-1"
						class="ml-auto cursor-pointer hover:underline"
						onclick={() => (showRulesInfo = !showRulesInfo)}
					>
						{showRulesInfo ? 'Hide available fields' : 'Show available fields'}
					</strong>
				</div>

				{#if showRulesInfo}
					<div class="rounded-field border border-warning/30 bg-warning/10 p-3 text-xs">
						<p class="mb-1">The following record fields are available:</p>
						<div class="mb-2 flex flex-wrap gap-1">
							{#each newFields.filter((f) => !f['@toDelete']) as field (field.name)}
								<code class="rounded bg-base-300 px-1 py-0.5 text-xs">{field.name}</code>
							{/each}
						</div>
						<hr class="my-2 border-base-300" />
						<p class="mb-1">
							The request fields could be accessed with the special <strong>@request</strong> fields:
						</p>
						<div class="mb-2 flex flex-wrap gap-1">
							<code class="rounded bg-base-300 px-1 py-0.5 text-xs">@request.headers.*</code>
							<code class="rounded bg-base-300 px-1 py-0.5 text-xs">@request.query.*</code>
							<code class="rounded bg-base-300 px-1 py-0.5 text-xs">@request.body.*</code>
							<code class="rounded bg-base-300 px-1 py-0.5 text-xs">@request.auth.*</code>
						</div>
						<hr class="my-2 border-base-300" />
						<p class="mb-1">You could also add constraints using <strong>@collection</strong>:</p>
						<div class="flex flex-wrap gap-1">
							<code class="rounded bg-base-300 px-1 py-0.5 text-xs"
								>@collection.ANY_COLLECTION_NAME.*</code
							>
						</div>
						<hr class="my-2 border-base-300" />
						<p class="mb-1">Example rule:</p>
						<code class="rounded bg-base-300 px-1 py-0.5 text-xs">@request.auth.id != ""</code>
					</div>
				{/if}

				<RuleField label="List/Search rule" name="listRule" bind:value={listRule} />
				<RuleField label="View rule" name="viewRule" bind:value={viewRule} />
				{#if newType !== 'view'}
					<RuleField label="Create rule" name="createRule" bind:value={createRule} />
					<RuleField label="Update rule" name="updateRule" bind:value={updateRule} />
					<RuleField label="Delete rule" name="deleteRule" bind:value={deleteRule} />
					<RuleField label="Manage rule" name="manageRule" bind:value={manageRule} />
				{:else}
					<p class="text-xs text-base-content/50">
						View collections are read-only — only the List and View rules apply.
					</p>
				{/if}
			</div>
		{/if}
	</div>

	{#if error}
		<div class="shrink-0 border-t border-base-300 bg-error/10 px-4 py-2 text-xs text-error">
			{error}
		</div>
	{/if}

	<!-- Footer -->
	<div class="flex shrink-0 items-center gap-2 border-t border-base-300 px-4 py-3">
		<button type="button" class="btn btn-ghost" onclick={requestClose}>Close</button>
		<div class="ml-auto flex items-center gap-2">
			{#if error}
				<svg
					width="16"
					height="16"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2"
					class="text-error"
					><circle cx="12" cy="12" r="10" /><line x1="12" y1="8" x2="12" y2="12" /><line
						x1="12"
						y1="16"
						x2="12.01"
						y2="16"
					/></svg
				>
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
</div>

<IndexesModal
	bind:show={showIndexesModal}
	bind:indexes={newIndexes}
	fieldNames={newFields
		.filter((f) => !f['@toDelete'])
		.map((f) => f.name as string)
		.filter(Boolean)}
/>

<!-- Unsaved-changes confirm -->
<Modal bind:show={showCloseConfirm} title="Discard changes?">
	<p class="text-sm">
		You have unsaved changes to this collection. They will be lost if you close without saving.
	</p>
	<div class="mt-4 flex justify-end gap-2">
		<Button type="button" class="btn-ghost btn-sm" onclick={() => (showCloseConfirm = false)}
			>Keep editing</Button
		>
		<Button type="button" class="btn-error btn-sm" onclick={doClose}>Discard</Button>
	</div>
</Modal>

<!-- Delete collection confirm -->
<Modal bind:show={showDeleteConfirm} title="Delete collection?">
	<p class="text-sm">
		Delete <strong>{newName}</strong>? The table and all its records will be permanently removed.
		This cannot be undone.
	</p>
	<div class="mt-4 flex justify-end gap-2">
		<Button type="button" class="btn-ghost btn-sm" onclick={() => (showDeleteConfirm = false)}
			>Cancel</Button
		>
		<Button type="button" class="btn-error btn-sm" loading={deleting} onclick={deleteCollection}
			>Delete</Button
		>
	</div>
</Modal>

<style>
	/* sortable children need pointer-events to catch drag enter/leave */
	[data-sortable-child] {
		pointer-events: auto;
	}
</style>
