<script lang="ts">
	import { client } from '$lib/client';
	import Button from '$lib/components/Button.svelte';
	import { toast } from 'svelte-sonner';

	let backingUp = $state(false);
	let restoring = $state(false);
	let restoreFileName = $state('');
	let parsedCollections = $state<{ name: string; type: string; recordCount: number }[]>([]);
	let restorePayload = $state<unknown[]>([]);
	let parseError = $state<string | null>(null);
	let restoreFileInput: HTMLInputElement | undefined = $state();
	let deleteMissing = $state(false);
	let restoreResult = $state<{
		imported: { name: string; records_imported: number }[];
		errors: { name: string; error: string }[];
	} | null>(null);

	async function doBackup() {
		backingUp = true;
		try {
			const res = (await client.http.get('/export')) as Record<string, unknown> | null;
			if (res) {
				const blob = new Blob([JSON.stringify(res, null, 2)], { type: 'application/json' });
				const url = URL.createObjectURL(blob);
				const a = document.createElement('a');
				a.href = url;
				a.download = `lazypock-backup-${new Date().toISOString().slice(0, 10)}.json`;
				a.click();
				URL.revokeObjectURL(url);
			}
		} catch (e) {
			toast.error(`Backup failed: ${(e as Error).message}`);
		} finally {
			backingUp = false;
		}
	}

	function loadBackupFile(file: File) {
		restoreFileName = file.name;
		parseError = null;
		restoreResult = null;
		parsedCollections = [];
		restorePayload = [];
		const reader = new FileReader();
		reader.onload = async (event) => {
			const text = (event.target?.result as string) ?? '';
			try {
				const data = JSON.parse(text);
				// Accept both the backup envelope ({collections: [...]}) and a
				// bare array of collections (import page format).
				const collections = Array.isArray(data) ? data : data?.collections;
				if (!Array.isArray(collections)) {
					parseError =
						'Invalid backup file. Expected { "collections": [...] } or an array of collections.';
					return;
				}
				restorePayload = collections;
				for (const c of collections) {
					if (c && typeof c.name === 'string') {
						parsedCollections.push({
							name: c.name,
							type: c.type || 'base',
							recordCount: Array.isArray(c.records) ? c.records.length : 0
						});
					}
				}
				if (parsedCollections.length === 0) {
					parseError = 'No collections found in the backup file.';
				}
			} catch {
				parseError = 'Invalid JSON in backup file.';
			} finally {
				if (restoreFileInput) restoreFileInput.value = '';
			}
		};
		reader.onerror = () => {
			parseError = 'Failed to read the backup file.';
		};
		reader.readAsText(file);
	}

	function clearRestore() {
		restoreFileName = '';
		parsedCollections = [];
		restorePayload = [];
		parseError = null;
		restoreResult = null;
		deleteMissing = false;
		if (restoreFileInput) restoreFileInput.value = '';
	}

	async function doRestore() {
		if (parsedCollections.length === 0) return;
		restoring = true;
		restoreResult = null;
		try {
			const res = (await client.http.post('/import', {
				collections: restorePayload,
				deleteMissing
			})) as { imported?: unknown[]; errors?: unknown[] } | null;
			const imported = (res?.imported as { name: string; records_imported: number }[]) ?? [];
			const errors = (res?.errors as { name: string; error: string }[]) ?? [];
			restoreResult = { imported, errors };
			if (errors.length > 0) {
				toast.error(`Restored ${imported.length} collections with ${errors.length} errors`);
			} else {
				toast.success(`Restored ${imported.length} collections successfully`);
			}
		} catch (e) {
			toast.error(`Restore failed: ${(e as Error).message}`);
		} finally {
			restoring = false;
		}
	}
</script>

<h2 class="mb-4 text-lg font-semibold">Backups</h2>

<div class="rounded-box border border-base-300 bg-base-100 p-6">
	<p class="mb-4 text-sm text-base-content/70">
		Download a full JSON backup of all collections and their data.
	</p>
	<Button class="btn-primary" loading={backingUp} onclick={doBackup}>Download Backup</Button>
</div>

<div class="mt-6 rounded-box border border-base-300 bg-base-100 p-6">
	<p class="mb-1 text-sm text-base-content/70">
		Restore a backup file (the JSON you downloaded above). Collections are created or updated and
		records are upserted by id, so relations stay intact and re-restoring never duplicates data.
	</p>
	<p class="mb-4 text-xs text-base-content/50">
		Also available from the CLI: <code class="font-mono">lazypock restore &lt;backup.json&gt;</code
		>.
	</p>

	<div class="mb-4 flex items-center gap-2">
		<button type="button" class="btn btn-outline btn-sm" onclick={() => restoreFileInput?.click()}>
			Choose backup file
		</button>
		<input
			bind:this={restoreFileInput}
			type="file"
			accept=".json,application/json"
			class="hidden"
			onchange={() => {
				if (restoreFileInput?.files?.length) loadBackupFile(restoreFileInput.files[0]);
			}}
		/>
		{#if restoreFileName}
			<span class="text-sm text-base-content/70">{restoreFileName}</span>
		{/if}
	</div>

	{#if parseError}
		<div class="mb-4 rounded-box border border-error/30 bg-error/10 p-3 text-xs text-error">
			{parseError}
		</div>
	{/if}

	{#if parsedCollections.length > 0}
		<div class="mb-4 rounded-box border border-base-300 bg-base-200/40 p-3">
			<div class="mb-2 text-xs font-semibold tracking-wide text-base-content/50 uppercase">
				{parsedCollections.length} collections in backup
			</div>
			<ul class="max-h-40 space-y-1 overflow-y-auto">
				{#each parsedCollections as c (c.name)}
					<li class="flex items-center justify-between text-sm">
						<span class="font-mono text-xs">{c.name}</span>
						<span class="text-xs text-base-content/50">
							{c.type}{c.recordCount > 0 ? ` · ${c.recordCount} records` : ''}
						</span>
					</li>
				{/each}
			</ul>
		</div>

		<div class="mb-4 flex items-start gap-2">
			<label class="switch">
				<input id="restore-delete-missing" type="checkbox" bind:checked={deleteMissing} />
				<span class="switch-slider"></span>
			</label>
			<div>
				<label class="switch-label" for="restore-delete-missing">
					<span class="txt">Delete collections not present in the backup</span>
				</label>
				<p class="text-xs text-base-content/50">
					Makes the database match the backup exactly. System collections are always kept. Leave off
					for a safe merge.
				</p>
			</div>
		</div>
	{/if}

	{#if restoreResult}
		<div class="mb-4 rounded-box border border-base-300 bg-base-200/40 p-3">
			<div class="mb-2 text-xs font-semibold tracking-wide text-base-content/50 uppercase">
				Restore results
			</div>
			<ul class="space-y-1">
				{#each restoreResult.imported as item (item.name)}
					<li class="flex items-center justify-between text-sm">
						<span class="font-mono text-xs">{item.name}</span>
						<span class="text-xs text-base-content/50">
							{item.records_imported > 0 ? `${item.records_imported} records` : 'schema only'}
						</span>
					</li>
				{/each}
				{#each restoreResult.errors as item (item.name)}
					<li class="text-xs text-error">✗ {item.name}: {item.error}</li>
				{/each}
			</ul>
		</div>
	{/if}

	<div class="flex items-center gap-3">
		<Button
			class="btn-warning"
			disabled={parsedCollections.length === 0}
			loading={restoring}
			onclick={doRestore}
		>
			Restore
		</Button>
		{#if restoreFileName}
			<button
				type="button"
				class="cursor-pointer border-none bg-transparent text-sm text-base-content/50 hover:text-base-content"
				onclick={clearRestore}
			>
				Clear
			</button>
		{/if}
	</div>
</div>
