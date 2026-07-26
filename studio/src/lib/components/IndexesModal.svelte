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

	let editingIndex = $state<number>(-1);

	// Form fields
	let isUnique = $state(false);
	let selectedFields = $state<string[]>([]);
	let whereExpr = $state('');

	function resetForm() {
		isUnique = false;
		selectedFields = [];
		whereExpr = '';
		editingIndex = -1;
	}

	function openNew() {
		resetForm();
		editingIndex = -1;
	}

	function openEdit(i: number) {
		const raw = indexes[i];
		const unique = raw.startsWith('UNIQUE ');
		const expr = unique ? raw.slice(7) : raw;
		isUnique = unique;
		selectedFields = expr
			.split(',')
			.map((s) => s.trim())
			.filter(Boolean);
		whereExpr = '';
		editingIndex = i;
	}

	function handleSave() {
		if (selectedFields.length === 0) return;
		const expr = selectedFields.join(', ');
		const raw = isUnique ? `UNIQUE ${expr}` : expr;

		if (editingIndex >= 0) {
			indexes[editingIndex] = raw;
			indexes = [...indexes];
		} else {
			indexes = [...indexes, raw];
		}
		show = false;
	}

	function handleDelete() {
		if (editingIndex >= 0) {
			indexes.splice(editingIndex, 1);
			indexes = [...indexes];
		}
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
		{#if indexes.length === 0 && editingIndex < 0}
			<p class="text-xs text-base-content/60">
				No indexes configured. Click "Add index" to create one.
			</p>
		{/if}

		<!-- Existing indexes list -->
		{#if editingIndex < 0}
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
					<button type="button" class="btn btn-ghost btn-xs px-1" onclick={() => openEdit(i)}>
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
						class="btn btn-ghost btn-xs px-1 text-error"
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
			<Button type="button" class="btn-sm w-fit" onclick={openNew}>+ Add index</Button>
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

			<div class="rounded-field bg-base-200/40 p-2">
				<label class="mb-1 block text-xs font-medium text-base-content/70" for="index-where"
					>Where expression (optional)</label
				>
				<input
					type="text"
					id="index-where"
					bind:value={whereExpr}
					placeholder="e.g. status = 'active'"
					class="input input-sm w-full"
				/>
			</div>

			<div class="mt-2 flex items-center gap-2">
				<Button type="button" class="btn-ghost btn-sm" onclick={() => (editingIndex = -1)}
					>Cancel</Button
				>
				<Button type="button" class="btn-error btn-sm mr-auto" onclick={handleDelete}>Delete</Button
				>
				<Button
					type="button"
					class="btn-primary btn-sm"
					disabled={selectedFields.length === 0}
					onclick={handleSave}
				>
					{editingIndex >= 0 ? 'Update' : 'Add'}
				</Button>
			</div>
		{/if}
	</div>
</Modal>
