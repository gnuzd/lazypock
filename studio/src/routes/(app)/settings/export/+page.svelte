<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import Button from '$lib/components/Button.svelte';
	import { Check, Clipboard, Download } from '@lucide/svelte';
	import '../settings.css';

	let collectionsList = $state<{ id: string; name: string; type: string }[]>([]);
	let loadingCollections = $state(false);
	let selectedExports = $state<Record<string, { id: string; name: string; type: string }>>({});
	let exportCopied = $state(false);

	let schemaJson = $derived(JSON.stringify(Object.values(selectedExports), null, 4));
	let totalSelected = $derived(Object.keys(selectedExports).length);
	let areAllSelected = $derived(
		collectionsList.length > 0 && totalSelected === collectionsList.length
	);

	async function loadCollections() {
		// if already loaded, skip
		if (collectionsList.length > 0) {
			// rebuild selected
			selectedExports = {};
			for (const c of collectionsList) {
				selectedExports[c.id] = c;
			}
			return;
		}
		loadingCollections = true;
		try {
			const res = (await client.http.get('/collections')) as {
				items?: { id: string; name: string; type: string }[];
			};
			collectionsList = res?.items ?? [];
			selectedExports = {};
			for (const c of collectionsList) {
				selectedExports[c.id] = c;
			}
		} catch {
			// ignore
		} finally {
			loadingCollections = false;
		}
	}

	onMount(() => {
		loadCollections();
	});

	function toggleSelectAll() {
		if (areAllSelected) {
			selectedExports = {};
		} else {
			selectedExports = {};
			for (const c of collectionsList) {
				selectedExports[c.id] = c;
			}
		}
	}

	function toggleSelectCollection(c: { id: string; name: string; type: string }) {
		if (selectedExports[c.id]) {
			const next = { ...selectedExports };
			delete next[c.id];
			selectedExports = next;
		} else {
			selectedExports = { ...selectedExports, [c.id]: c };
		}
	}

	function downloadExport() {
		const data = Object.values(selectedExports);
		const blob = new Blob([JSON.stringify(data, null, 4)], { type: 'application/json' });
		const url = URL.createObjectURL(blob);
		const a = document.createElement('a');
		a.href = url;
		a.download = `lazypock-schema-${new Date().toISOString().slice(0, 10)}.json`;
		a.click();
		URL.revokeObjectURL(url);
	}

	function copyExport() {
		navigator.clipboard.writeText(schemaJson);
		exportCopied = true;
		setTimeout(() => (exportCopied = false), 2000);
	}
</script>

<h2 class="mb-4 text-lg font-semibold">Export Collections</h2>
<div class="mb-4 text-sm text-base-content/60">
	<p>
		Below you'll find your current collections configuration that you could import in another
		environment.
	</p>
</div>

{#if loadingCollections}
	<div class="flex justify-center py-8">
		<span class="text-sm text-base-content/50">Loading collections...</span>
	</div>
{:else if collectionsList.length === 0}
	<div class="rounded-box border border-base-300 bg-base-100 p-6 text-center">
		<p class="text-sm text-base-content/50">No collections yet.</p>
		<Button class="btn-primary mt-4" onclick={loadCollections}>Load Collections</Button>
	</div>
{:else}
	<div class="export-panel flex flex-col gap-4">
		<div class="flex flex-col gap-4 lg:flex-row">
			<div class="min-w-0 flex-1 rounded-box border border-base-300 bg-base-100">
				<div class="border-b border-base-300 px-3 py-2">
					<label class="flex cursor-pointer items-center gap-2 text-sm font-medium">
						<input
							type="checkbox"
							class="checkbox"
							checked={areAllSelected}
							onchange={toggleSelectAll}
						/>
						Select all
						<span class="text-xs text-base-content/50">({totalSelected} selected)</span>
					</label>
				</div>
				<div class="max-h-96 overflow-y-auto">
					{#each collectionsList as c (c.id)}
						<label
							class="flex cursor-pointer items-center gap-2 border-b border-base-200 px-3 py-1.5 text-sm hover:bg-base-200"
						>
							<input
								type="checkbox"
								class="checkbox"
								checked={selectedExports[c.id] !== undefined}
								onchange={() => toggleSelectCollection(c)}
							/>
							<span class="font-medium">{c.name}</span>
							<span class="text-xs text-base-content/40">{c.type}</span>
						</label>
					{/each}
				</div>
			</div>

			<div class="relative min-w-0 flex-1 rounded-box border border-base-300 bg-base-100">
				<button
					type="button"
					class="absolute top-2 right-2 z-10 cursor-pointer rounded-field border border-base-300 bg-base-100 px-2 py-1 text-xs text-base-content/60 hover:text-base-content"
					disabled={!totalSelected}
					onclick={copyExport}
				>
					{#if exportCopied}
						<span class="flex items-center gap-1 text-success"><Check class="h-3 w-3" />Copied</span
						>
					{:else}
						<span class="flex items-center gap-1"><Clipboard class="h-3 w-3" />Copy</span>
					{/if}
				</button>
				<pre class="max-h-96 overflow-auto p-3 font-mono text-xs">{schemaJson ||
						'Select collections to preview...'}</pre>
			</div>
		</div>

		<div class="flex justify-end">
			<Button class="btn-primary" disabled={!totalSelected} onclick={downloadExport}>
				<Download class="h-4 w-4" />
				Download as JSON
			</Button>
		</div>
	</div>
{/if}
