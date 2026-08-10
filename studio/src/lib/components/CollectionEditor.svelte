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

	/**
	 * New/Edit collection editor. Shared by /collections/[name] (edit) and
	 * /collections/new (create). Owns all form state and the save action.
	 */
	let {
		editingCollectionId = null as string | null,
		existingName = '',
		onClose
	}: {
		/** The collection id when editing an existing collection, null when creating. */
		editingCollectionId?: string | null;
		/** The current name (used to detect renames). */
		existingName?: string;
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
	let showDeleteConfirm = $state(false);
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

	async function handleSave() {
		if (saving) return;

		// Build payload
		const payload: Record<string, unknown> = {
			name: newName.trim(),
			type: newType,
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
		<div class="flex gap-0">
			<div class="field min-w-0 flex-1">
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
			</div>
			<Dropdown bind:show={typeOpen} class="shrink-0">
				{#snippet trigger()}
					<label class="mb-1 block text-xs font-medium text-base-content/70">&nbsp;</label>
					<button
						type="button"
						class="btn btn-sm flex h-[34px] items-center gap-2 rounded-field border-2 border-current bg-transparent px-3 whitespace-nowrap text-base-content"
					>
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
					</button>
				{/snippet}
				<div class="min-w-[200px] py-1">
					{#each collectionTypes as ct (ct.value)}
						<button
							type="button"
							class="flex w-full cursor-pointer items-center gap-2 rounded-field border-none bg-transparent px-3 py-2 text-left text-sm text-base-content hover:bg-base-200"
							class:font-medium={ct.value === newType}
							onmousedown={() => {
								newType = ct.value;
								typeOpen = false;
							}}
						>
							<span>{ct.label}</span>
						</button>
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
		{#if activeTab === 'Fields'}
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
				<RuleField label="Create rule" name="createRule" bind:value={createRule} />
				<RuleField label="Update rule" name="updateRule" bind:value={updateRule} />
				<RuleField label="Delete rule" name="deleteRule" bind:value={deleteRule} />
				<RuleField label="Manage rule" name="manageRule" bind:value={manageRule} />
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
		{#if editingCollectionId}
			<button
				type="button"
				class="btn btn-ghost btn-error px-2"
				title="Delete collection"
				onclick={() => (showDeleteConfirm = true)}
			>
				<svg
					width="16"
					height="16"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2"
					stroke-linecap="round"
					stroke-linejoin="round"
					><polyline points="3 6 5 6 21 6" /><path
						d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"
					/></svg
				>
			</button>
		{/if}
		<button type="button" class="btn btn-ghost mr-auto" onclick={requestClose}>Close</button>
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
