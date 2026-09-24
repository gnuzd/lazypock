<script lang="ts">
	import Modal from './Modal.svelte';
	import Button from './Button.svelte';

	let {
		show = $bindable(false),
		indexes = $bindable<string[]>([]),
		fieldNames = [] as string[]
	}: {
		show: boolean;
		indexes: string[];
		fieldNames?: string[];
	} = $props();

	// -1 = adding a new index, >= 0 = editing that entry.
	let editingIndex = $state<number>(-1);
	/** Whether the add/edit form is open (vs. the saved-indexes list). */
	let formOpen = $state(false);

	// Form fields
	let isUnique = $state(false);
	let selectedFields = $state<string[]>([]);

	function resetForm() {
		isUnique = false;
		selectedFields = [];
		editingIndex = -1;
		formOpen = false;
	}

	// Always reopen on the list view so a half-finished form (or a stale edit)
	// from a previous visit never leaks into the next one.
	$effect(() => {
		if (show) resetForm();
	});

	function openNew() {
		resetForm();
		formOpen = true;
	}

	function openEdit(i: number) {
		resetForm();
		const raw = indexes[i];
		const unique = raw.startsWith('UNIQUE ');
		const expr = unique ? raw.slice(7) : raw;
		isUnique = unique;
		selectedFields = expr
			.split(',')
			.map((s) => s.trim())
			.filter(Boolean);
		editingIndex = i;
		formOpen = true;
	}

	function handleSave() {
		if (selectedFields.length === 0) return;
		const expr = selectedFields.join(', ');
		const raw = isUnique ? `UNIQUE ${expr}` : expr;

		if (editingIndex >= 0) {
			indexes = indexes.map((idx, i) => (i === editingIndex ? raw : idx));
		} else {
			indexes = [...indexes, raw];
		}
		formOpen = false;
		show = false;
	}

	function handleDelete() {
		if (editingIndex >= 0) {
			indexes = indexes.filter((_, i) => i !== editingIndex);
		}
		formOpen = false;
		show = false;
	}

	function toggleField(name: string) {
		if (selectedFields.includes(name)) {
			selectedFields = selectedFields.filter((f) => f !== name);
		} else {
			selectedFields = [...selectedFields, name];
		}
	}
</script>

<Modal bind:show title="Collection indexes">
	<div class="flex flex-col gap-3 text-sm">
		{#if indexes.length === 0 && !formOpen}
			<p class="text-xs text-base-content/60">
				No indexes configured. Click "Add index" to create one.
			</p>
		{/if}

		<!-- Existing indexes list -->
		{#if !formOpen}
			{#each indexes as idx, i (i)}
				<div class="flex items-center gap-2 rounded-field bg-base-200/40 p-2">
					<code class="flex-1 text-xs">
						{#if idx.startsWith('UNIQUE ')}
							<span class="font-semibold text-info">UNIQUE </span>
							{idx.slice(7)}
						{:else}
							{idx}
						{/if}
					</code>
					<button type="button" class="btn btn-ghost btn-xs w-6 px-0" onclick={() => openEdit(i)}>
						<svg
							width="12"
							height="12"
							viewBox="0 0 24 24"
							fill="none"
							stroke="currentColor"
							stroke-width="2"
							><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" /><path
								d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"
							/></svg
						>
					</button>
					<button
						type="button"
						class="btn btn-ghost btn-xs w-6 px-0 text-error"
						onclick={() => {
							indexes.splice(i, 1);
							indexes = [...indexes];
						}}
					>
						<svg
							width="12"
							height="12"
							viewBox="0 0 24 24"
							fill="none"
							stroke="currentColor"
							stroke-width="2"
							><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg
						>
					</button>
				</div>
			{/each}
			<Button
				type="button"
				class="btn-sm w-fit"
				disabled={fieldNames.length === 0}
				onclick={openNew}>+ Add index</Button
			>
			{#if fieldNames.length === 0}
				<p class="text-xs text-base-content/50">Add at least one field before creating an index.</p>
			{/if}
		{:else}
			<!-- Edit index form -->
			<div class="rounded-field bg-base-200/40 p-2">
				<label class="mb-3 flex cursor-pointer items-center gap-2">
					<input type="checkbox" class="checkbox checkbox-sm" bind:checked={isUnique} />
					<span class="text-xs font-medium">Unique constraint</span>
				</label>
			</div>

			{#if fieldNames.length > 0}
				<div class="rounded-field bg-base-200/40 p-2">
					<label class="mb-1 block text-xs font-medium text-base-content/70">Fields</label>
					<div class="flex flex-wrap gap-1">
						{#each fieldNames as name (name)}
							<button
								type="button"
								class="cursor-pointer rounded border px-2 py-1 text-xs transition-colors {selectedFields.includes(
									name
								)
									? 'border-primary bg-primary text-primary-content'
									: 'border-base-300 bg-base-200 hover:border-base-content/30'}"
								onclick={() => toggleField(name)}>{name}</button
							>
						{/each}
					</div>
				</div>
			{/if}

			<div class="mt-2 flex items-center gap-2">
				<Button type="button" class="btn-ghost btn-sm" onclick={() => (formOpen = false)}
					>Cancel</Button
				>
				{#if editingIndex >= 0}
					<Button type="button" class="btn-error btn-sm" onclick={handleDelete}>Delete</Button>
				{/if}
				<Button
					type="button"
					class="btn-primary btn-sm ml-auto"
					disabled={selectedFields.length === 0}
					onclick={handleSave}
				>
					{editingIndex >= 0 ? 'Update' : 'Add'}
				</Button>
			</div>
		{/if}
	</div>
</Modal>
