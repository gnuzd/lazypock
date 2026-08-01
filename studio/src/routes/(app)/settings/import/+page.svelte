<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import Button from '$lib/components/Button.svelte';
	import '../settings.css';

	let importSchemas = $state('');
	let importFileInput: HTMLInputElement | undefined = $state();
	const importPlaceholder = '[{ "id": "...", "name": "...", "type": "base", "fields": [] }]';
	let importLoadingFile = $state(false);
	let parsedCollections: { id: string; name: string; type: string }[] = [];
	let oldCollections: { id: string; name: string; type: string }[] = [];
	let loadingOldCollections = $state(false);
	let deleteMissing = $state(true);
	let importing = $state(false);
	let importResult = $state<string | null>(null);

	onMount(async () => {
		loadingOldCollections = true;
		try {
			const res = (await client.http.get('/collections')) as {
				items?: { id: string; name: string; type: string }[];
			};
			oldCollections = res?.items ?? [];
		} catch {
			// ignore
		} finally {
			loadingOldCollections = false;
		}
	});

	function loadFile(file: File) {
		importLoadingFile = true;
		const reader = new FileReader();
		reader.onload = async (event) => {
			importLoadingFile = false;
			importSchemas = (event.target?.result as string) ?? '';
			if (importFileInput) importFileInput.value = '';
			parseImport();
		};
		reader.onerror = () => {
			importLoadingFile = false;
			importResult = 'Failed to load the imported JSON.';
			if (importFileInput) importFileInput.value = '';
		};
		reader.readAsText(file);
	}

	function parseImport() {
		parsedCollections = [];
		importResult = null;
		try {
			const data = JSON.parse(importSchemas);
			if (!Array.isArray(data)) {
				importResult = 'Invalid format. Expected an array of collections.';
				return;
			}
			// Deduplicate by id
			const seenIds: Record<string, true> = {};
			for (const c of data) {
				if (c.id && c.name && !seenIds[c.id]) {
					seenIds[c.id] = true;
					parsedCollections.push({ id: c.id, name: c.name, type: c.type || 'base' });
				}
			}
		} catch {
			importResult = 'Invalid JSON format.';
		}
	}

	function clearImport() {
		importSchemas = '';
		parsedCollections = [];
		importResult = null;
		if (importFileInput) importFileInput.value = '';
	}

	let isValidImport = $derived(!!importSchemas && parsedCollections.length > 0 && !importResult);

	// Detect changes
	let importChanges = $derived.by(() => {
		if (!isValidImport) return { added: [], removed: [], changed: [] };
		const added: string[] = [];
		const removed: string[] = [];
		const changed: string[] = [];
		const oldMap = new Map(oldCollections.map((c) => [c.id, c]));
		const newIds = new Set(parsedCollections.map((c) => c.id));

		for (const c of oldCollections) {
			if (!newIds.has(c.id)) {
				if (deleteMissing) removed.push(c.name);
			}
		}

		for (const c of parsedCollections) {
			const old = oldMap.get(c.id);
			if (!old) {
				added.push(c.name);
			} else if (old.name !== c.name || old.type !== c.type) {
				changed.push(c.name);
			}
		}
		return { added, removed, changed };
	});

	let hasChanges = $derived(
		importChanges.added.length > 0 ||
			importChanges.removed.length > 0 ||
			importChanges.changed.length > 0
	);

	async function doImport() {
		if (!isValidImport) return;
		importing = true;
		importResult = null;
		try {
			const data = JSON.parse(importSchemas);
			const res = (await client.http.post('/import', {
				collections: data,
				deleteMissing: deleteMissing
			})) as { imported?: unknown[]; errors?: unknown[] } | null;
			if (res?.errors && (res.errors as unknown[]).length > 0) {
				importResult = `Imported ${(res.imported as unknown[]).length} collections with ${(res.errors as unknown[]).length} errors.`;
			} else {
				const count = (res?.imported as unknown[])?.length ?? 0;
				importResult = `Successfully imported ${count} collections.`;
			}
		} catch (e) {
			importResult = `Import failed: ${(e as Error).message}`;
		} finally {
			importing = false;
		}
	}
</script>

<h2 class="mb-4 text-lg font-semibold">Import Collections</h2>

{#if loadingOldCollections}
	<div class="flex justify-center py-8">
		<span class="text-sm text-base-content/50">Loading existing collections...</span>
	</div>
{:else}
	<div class="rounded-box border border-base-300 bg-base-100 p-6">
		<div class="mb-4 text-sm text-base-content/60">
			<p>
				Paste below the collections configuration you want to import or
				<button
					type="button"
					class="btn btn-outline btn-sm ml-2"
					class:btn-loading={importLoadingFile}
					onclick={() => importFileInput?.click()}
				>
					Load from JSON file
				</button>
			</p>
			<input
				bind:this={importFileInput}
				type="file"
				accept=".json"
				class="hidden"
				onchange={() => {
					if (importFileInput?.files?.length) loadFile(importFileInput.files[0]);
				}}
			/>
		</div>

		<div class="field mb-4">
			<label for="import-schemas" class="field-label">Collections</label>
			<textarea
				id="import-schemas"
				class="field-input font-mono text-xs"
				class:border-error={importSchemas && !isValidImport}
				spellcheck="false"
				rows="16"
				placeholder={importPlaceholder}
				bind:value={importSchemas}
				oninput={parseImport}></textarea>
			{#if importSchemas && !isValidImport}
				<p class="mt-1 text-xs text-error">
					{importResult || 'Invalid collections configuration.'}
				</p>
			{/if}
		</div>

		<div class="switch-field mb-4">
			<label class="switch-label" for="delete-missing">
				<span class="txt">Delete missing collections and schema fields</span>
			</label>
			<label class="switch">
				<input
					id="delete-missing"
					type="checkbox"
					bind:checked={deleteMissing}
					disabled={!isValidImport}
				/>
				<span class="switch-slider"></span>
			</label>
		</div>

		{#if isValidImport && parsedCollections.length > 0 && !hasChanges}
			<div class="mb-4 rounded-box border border-info/30 bg-info/20 p-3 text-sm text-info">
				Your collections configuration is already up-to-date!
			</div>
		{/if}

		{#if isValidImport && hasChanges}
			<h5 class="mb-2 text-sm font-semibold">Detected changes</h5>
			<div class="mb-4 space-y-1">
				{#each importChanges.removed as name (name)}
					<label class="flex items-center gap-2 rounded-field bg-error/20 px-3 py-1.5 text-sm">
						<span class="text-white rounded bg-error px-1.5 py-0.5 text-[10px] font-semibold"
							>Deleted</span
						>
						<span>{name}</span>
					</label>
				{/each}
				{#each importChanges.changed as name (name)}
					<label class="flex items-center gap-2 rounded-field bg-warning/20 px-3 py-1.5 text-sm">
						<span class="text-white rounded bg-warning px-1.5 py-0.5 text-[10px] font-semibold"
							>Changed</span
						>
						<span>{name}</span>
					</label>
				{/each}
				{#each importChanges.added as name (name)}
					<label class="flex items-center gap-2 rounded-field bg-success/20 px-3 py-1.5 text-sm">
						<span class="text-white rounded bg-success px-1.5 py-0.5 text-[10px] font-semibold"
							>Added</span
						>
						<span>{name}</span>
					</label>
				{/each}
			</div>
		{/if}

		<div class="flex items-center justify-between">
			{#if importSchemas}
				<button
					type="button"
					class="cursor-pointer border-none bg-transparent text-sm text-base-content/50 hover:text-base-content"
					onclick={clearImport}
				>
					Clear
				</button>
			{:else}
				<div></div>
			{/if}
			<Button
				class="btn-warning"
				disabled={!isValidImport || !hasChanges}
				loading={importing}
				onclick={doImport}
			>
				Import
			</Button>
		</div>

		{#if importResult && !importResult.startsWith('Invalid')}
			<p class="mt-3 text-xs text-base-content/60">{importResult}</p>
		{/if}
	</div>
{/if}
