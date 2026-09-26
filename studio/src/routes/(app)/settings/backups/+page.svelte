<script lang="ts">
	import { client } from '$lib/client';
	import Button from '$lib/components/Button.svelte';
	import AiPromptButton from '$lib/components/AiPromptButton.svelte';
	import UndoImportButton from '$lib/components/UndoImportButton.svelte';
	import Switch from '$lib/components/Switch.svelte';
	import { toast } from 'svelte-sonner';
	import Modal from '$lib/components/Modal.svelte';
	import {
		ACCEPTED_FILES,
		confirmationRequired,
		describeError,
		downloadArchive,
		importJson,
		isArchive,
		parseCollections,
		summarize,
		uploadArchive
	} from '$lib/importRestore';

	let backingUp = $state(false);
	let restoring = $state(false);
	let restoreFileName = $state('');
	let parsedCollections = $state<{ name: string; type: string; recordCount: number }[]>([]);
	let restorePayload = $state<unknown[]>([]);
	let parseError = $state<string | null>(null);
	let restoreFileInput: HTMLInputElement | undefined = $state();
	let deleteMissing = $state(false);
	let atomic = $state(true);
	let password = $state('');
	let undoToken = $state(0);
	let restoreResult = $state<{
		imported: { name: string; type?: string; records_imported?: number }[];
		errors: { name: string; error: string }[];
	} | null>(null);
	// An NDJSON archive is uploaded as a file rather than parsed in the browser.
	let archiveFile = $state<File | null>(null);
	let uploadProgress = $state<number | null>(null);
	let exportProgress = $state<number | null>(null);
	// Set when the server refuses a large restore pending explicit confirmation.
	let confirmMessage = $state<string | null>(null);
	let confirmOpen = $state(false);

	async function downloadZipBackup() {
		backingUp = true;
		exportProgress = 0;
		try {
			const name = await downloadArchive((p) => (exportProgress = p));
			toast.success(`Downloaded ${name}`);
		} catch (e) {
			toast.error(`Backup failed: ${describeError(e)}`);
		} finally {
			backingUp = false;
			exportProgress = null;
		}
	}

	// Legacy single-document JSON export. Still supported (and still what
	// /backup.schema.json describes) but it builds the whole database as one JS
	// string, so it is no longer the default for large databases.
	async function downloadJsonBackup() {
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
			toast.error(`Backup failed: ${describeError(e)}`);
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

		// Archives are not parsed in the browser — reading a 10 GB zip into a JS
		// string is exactly the failure mode this path removes. The file is sent
		// as-is and the server reports what it imported.
		if (isArchive(file)) {
			archiveFile = file;
			if (restoreFileInput) restoreFileInput.value = '';
			return;
		}

		archiveFile = null;
		const reader = new FileReader();
		reader.onload = async (event) => {
			const text = (event.target?.result as string) ?? '';
			const { collections, error } = parseCollections(text);

			if (error || !collections) {
				parseError = error;
			} else {
				restorePayload = collections;

				for (const c of collections as {
					name?: string;
					type?: string;
					records?: unknown[];
				}[]) {
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
			}

			if (restoreFileInput) restoreFileInput.value = '';
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
		password = '';
		archiveFile = null;
		uploadProgress = null;
		if (restoreFileInput) restoreFileInput.value = '';
	}

	async function doRestore(confirm = false) {
		if (!archiveFile && parsedCollections.length === 0) return;
		restoring = true;
		restoreResult = null;
		uploadProgress = archiveFile ? 0 : null;

		const options = {
			deleteMissing,
			atomic: atomic ? ('batch' as const) : (false as const),
			password,
			confirm
		};

		try {
			const outcome = archiveFile
				? await uploadArchive(archiveFile, options, (p) => (uploadProgress = p))
				: await importJson({ ...options, collections: restorePayload });

			const imported = outcome.imported ?? [];
			const errors = (outcome.errors ?? []) as { name: string; error: string }[];
			restoreResult = { imported, errors };

			const { ok, message } = summarize(outcome, 'Restored');
			if (ok) toast.success(message);
			else toast.error(message);
			undoToken += 1;

			if (ok && archiveFile) clearRestore();
		} catch (e) {
			const gate = confirmationRequired(e);
			if (gate) {
				confirmMessage = gate.message;
				confirmOpen = true;
			} else {
				toast.error(`Restore failed: ${describeError(e)}`);
			}
		} finally {
			restoring = false;
			uploadProgress = null;
		}
	}

	function confirmAndRestore() {
		confirmOpen = false;
		confirmMessage = null;
		void doRestore(true);
	}
</script>

<h2 class="mb-4 text-lg font-semibold">Backups</h2>

<div class="rounded-box border border-base-300 bg-base-100 p-6">
	<p class="mb-4 text-sm text-base-content/70">
		Download a full backup of all collections and their data. The
		<strong>NDJSON archive</strong> is streamed to disk, so it works no matter how large the database
		is (records over 100 MB included).
	</p>
	<div class="flex flex-wrap items-center gap-2">
		<Button class="btn-primary" loading={backingUp} onclick={downloadZipBackup}>
			Download Backup (.zip)
		</Button>
		<Button class="btn-outline btn-sm" loading={backingUp} onclick={downloadJsonBackup}>
			Download JSON
		</Button>
	</div>
	{#if exportProgress !== null}
		<div class="mt-4">
			<div class="mb-1 flex justify-between text-xs text-base-content/60">
				<span>Exporting…</span>
				<span>{exportProgress}%</span>
			</div>
			<progress class="progress progress-primary w-full" value={exportProgress} max="100"
			></progress>
		</div>
	{/if}
	<p class="mt-3 text-xs text-base-content/50">
		The JSON download holds the whole database in the browser's memory — prefer the archive for
		large databases.
	</p>
</div>

<div class="mt-6 rounded-box border border-base-300 bg-base-100 p-6">
	<p class="mb-1 text-sm text-base-content/70">
		Restore a backup file (an NDJSON <code class="font-mono">.zip</code> archive or the JSON you downloaded
		above). Collections are created or updated and records are upserted by id, so relations stay intact
		and re-restoring never duplicates data.
	</p>
	<p class="mb-4 text-xs text-base-content/50">
		Also available from the CLI:
		<code class="font-mono">lazypock restore &lt;backup.zip|backup.json&gt;</code>.
	</p>

	<div class="mb-4 flex flex-wrap items-center gap-2">
		<Button class="btn-outline btn-sm" onclick={() => restoreFileInput?.click()}>
			Choose backup file
		</Button>
		<input
			bind:this={restoreFileInput}
			type="file"
			accept={ACCEPTED_FILES}
			class="hidden"
			onchange={() => {
				if (restoreFileInput?.files?.length) loadBackupFile(restoreFileInput.files[0]);
			}}
		/>
		{#if restoreFileName}
			<span class="text-sm text-base-content/70">{restoreFileName}</span>
		{/if}
	</div>

	<p class="mb-4 flex flex-wrap items-center gap-2 text-sm text-base-content/70">
		Or generate a file with an AI assistant:
		<AiPromptButton />
	</p>

	{#if parseError}
		<div class="mb-4 rounded-box border border-error/30 bg-error/10 p-3 text-xs text-error">
			{parseError}
		</div>
	{/if}

	{#if archiveFile}
		<div class="mb-4 rounded-box border border-info/30 bg-info/20 p-3 text-sm text-info">
			Archive selected: <span class="font-mono">{archiveFile.name}</span>
			<span class="text-info/70">({(archiveFile.size / 1_048_576).toFixed(1)} MB)</span> — the server
			streams it, so per-collection counts are shown in the results below.
		</div>
	{/if}

	{#if uploadProgress !== null}
		<div class="mb-4">
			<div class="mb-1 flex justify-between text-xs text-base-content/60">
				<span>Uploading archive…</span>
				<span>{uploadProgress}%</span>
			</div>
			<progress class="progress progress-primary w-full" value={uploadProgress} max="100"
			></progress>
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
	{/if}

	{#if archiveFile || parsedCollections.length > 0}
		<Switch
			variant="inline"
			class="mb-4"
			bind:checked={deleteMissing}
			description="Makes the database match the backup exactly. System collections are always kept. Leave off for a safe merge."
		>
			Delete collections not present in the backup
		</Switch>

		<Switch
			variant="inline"
			class="mb-4"
			bind:checked={atomic}
			description="All-or-nothing (recommended). Turn off to apply what can be applied and report the rest."
		>
			Roll the whole restore back if any collection fails
		</Switch>
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
							{item.records_imported ? `${item.records_imported} records` : 'schema only'}
						</span>
					</li>
				{/each}
				{#each restoreResult.errors as item (item.name)}
					<li class="text-xs text-error">✗ {item.name}: {item.error}</li>
				{/each}
			</ul>
		</div>
	{/if}

	<div class="mb-4">
		<label class="field-label" for="restore-password">Confirm your password</label>
		<input
			id="restore-password"
			type="password"
			class="field-input"
			autocomplete="current-password"
			bind:value={password}
		/>
		<p class="mt-1 text-xs text-base-content/50">
			Restoring rewrites your collections and records — confirm your superuser password.
		</p>
	</div>

	<div class="flex items-center gap-3">
		<Button
			class="btn-warning"
			disabled={(!archiveFile && parsedCollections.length === 0) || !password}
			loading={restoring}
			onclick={() => doRestore()}
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

<UndoImportButton class="mt-6" reloadToken={undoToken} />

<Modal bind:show={confirmOpen} title="This restore is large">
	<p class="text-sm">{confirmMessage}</p>
	<div class="mt-5 flex justify-end gap-2">
		<Button
			class="btn-ghost"
			onclick={() => {
				confirmOpen = false;
				confirmMessage = null;
			}}
		>
			Cancel
		</Button>
		<Button class="btn-warning" loading={restoring} onclick={confirmAndRestore}>
			Proceed without automatic undo
		</Button>
	</div>
</Modal>
