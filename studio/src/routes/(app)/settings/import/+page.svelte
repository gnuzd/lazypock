<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import { z } from 'zod';
	import { toast } from 'svelte-sonner';
	import Button from '$lib/components/Button.svelte';
	import AiPromptButton from '$lib/components/AiPromptButton.svelte';
	import UndoImportButton from '$lib/components/UndoImportButton.svelte';
	import Switch from '$lib/components/Switch.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import { createForm } from '$lib/createForm.svelte';
	import {
		ACCEPTED_FILES,
		confirmationRequired,
		describeError,
		importJson,
		isArchive,
		parseCollections,
		summarize,
		uploadArchive
	} from '$lib/importRestore';
	import '../settings.css';

	const importSchema = z.object({
		schemas: z.string(),
		deleteMissing: z.boolean(),
		atomic: z.boolean(),
		password: z.string()
	});

	let importForm = $state(
		// deleteMissing defaults to FALSE: it is destructive (it drops every user
		// collection absent from the payload), and the published format docs say to
		// leave it off when importing a partial file.
		createForm(importSchema, { schemas: '', deleteMissing: false, atomic: true, password: '' })
	);
	let undoToken = $state(0);
	let importFileInput: HTMLInputElement | undefined = $state();
	const importPlaceholder = '[{ "id": "...", "name": "...", "type": "base", "fields": [] }]';
	let importLoadingFile = $state(false);
	let parsedCollections: { id?: string; name: string; type: string }[] = $state([]);
	let oldCollections: { id: string; name: string; type: string }[] = [];
	let loadingOldCollections = $state(false);
	let importing = $state(false);
	let importResult = $state<string | null>(null);
	// An NDJSON archive is uploaded as a file instead of being pasted as JSON.
	let archiveFile = $state<File | null>(null);
	// Upload progress percentage (null when not uploading).
	let progress = $state<number | null>(null);
	// Set when the server refuses a large import pending explicit confirmation.
	let confirmMessage = $state<string | null>(null);
	let confirmOpen = $state(false);

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

		// Archives are not pasted as JSON — hand the file straight to the upload.
		if (isArchive(file)) {
			archiveFile = file;
			importForm.form.schemas = '';
			parsedCollections = [];
			importResult = null;
			importLoadingFile = false;
			if (importFileInput) importFileInput.value = '';
			return;
		}

		archiveFile = null;
		const reader = new FileReader();
		reader.onload = async (event) => {
			importLoadingFile = false;
			importForm.form.schemas = (event.target?.result as string) ?? '';
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

		const { collections, error } = parseCollections(importForm.form.schemas);
		if (error || !collections) {
			importResult = error;
			return;
		}

		// Deduplicate by id, falling back to name (backups from older
		// versions don't carry a per-collection id).
		const seenIds: Record<string, true> = {};
		for (const c of collections as { id?: string; name?: string; type?: string }[]) {
			const key = c.id || c.name;
			if (key && c.name && !seenIds[key]) {
				seenIds[key] = true;
				parsedCollections.push({ id: c.id, name: c.name, type: c.type || 'base' });
			}
		}
	}

	function clearImport() {
		importForm.reset();
		parsedCollections = [];
		importResult = null;
		archiveFile = null;
		progress = null;
		if (importFileInput) importFileInput.value = '';
	}

	let isValidImport = $derived(
		!!importForm.form.schemas && parsedCollections.length > 0 && !importResult
	);

	// Detect changes — keyed by id when present, else by name.
	let importChanges = $derived.by(() => {
		if (!isValidImport) return { added: [], removed: [], changed: [] };
		const added: string[] = [];
		const removed: string[] = [];
		const changed: string[] = [];
		const oldMap = new Map(oldCollections.map((c) => [c.id, c]));
		const newKeys = new Set(parsedCollections.map((c) => c.id || c.name));

		for (const c of oldCollections) {
			if (!newKeys.has(c.id)) {
				if (importForm.form.deleteMissing) removed.push(c.name);
			}
		}

		for (const c of parsedCollections) {
			const old = c.id ? oldMap.get(c.id) : undefined;
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

	// An archive carries no parsed diff, so it is importable as soon as it is selected.
	let canImport = $derived(archiveFile ? true : isValidImport && hasChanges);

	async function doImport(confirm = false) {
		if (!canImport || !importForm.form.password) return;
		importing = true;
		importResult = null;
		progress = null;

		const options = {
			deleteMissing: importForm.form.deleteMissing,
			atomic: importForm.form.atomic ? ('batch' as const) : (false as const),
			password: importForm.form.password,
			confirm
		};

		try {
			const outcome = archiveFile
				? await uploadArchive(archiveFile, options, (p) => (progress = p))
				: await importJson({
						...options,
						collections: parseCollections(importForm.form.schemas).collections ?? []
					});

			const { ok, message } = summarize(outcome);
			if (ok) toast.success(message);
			else toast.error(message);
			undoToken += 1;

			if (ok && archiveFile) {
				archiveFile = null;
				if (importFileInput) importFileInput.value = '';
			}
		} catch (e) {
			const gate = confirmationRequired(e);
			if (gate) {
				confirmMessage = gate.message;
				confirmOpen = true;
			} else {
				toast.error(`Import failed: ${describeError(e)}`);
			}
		} finally {
			importing = false;
			progress = null;
		}
	}

	function confirmAndImport() {
		confirmOpen = false;
		confirmMessage = null;
		void doImport(true);
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
					Load from JSON or archive
				</button>
			</p>
			<p class="mt-2 flex flex-wrap items-center gap-2">
				New to the format? Copy a ready-made prompt and describe what you want to an AI assistant,
				then paste its JSON output below:
				<AiPromptButton />
			</p>
			<input
				bind:this={importFileInput}
				type="file"
				accept={ACCEPTED_FILES}
				class="hidden"
				onchange={() => {
					if (importFileInput?.files?.length) loadFile(importFileInput.files[0]);
				}}
			/>
		</div>

		{#if archiveFile}
			<div class="mb-4 rounded-box border border-info/30 bg-info/20 p-3 text-sm text-info">
				Uploading <span class="font-mono">{archiveFile.name}</span>
				<span class="text-info/70">({(archiveFile.size / 1_048_576).toFixed(1)} MB)</span> — the archive
				is streamed to the server, so the whole database is never held in the browser. Per-collection
				record counts appear in the result.
			</div>
		{/if}

		<div class="field mb-4">
			<label for="import-schemas" class="field-label">Collections</label>
			<textarea
				id="import-schemas"
				class="field-input font-mono text-xs"
				class:border-error={importForm.form.schemas && !isValidImport}
				spellcheck="false"
				rows="16"
				placeholder={importPlaceholder}
				bind:value={importForm.form.schemas}
				oninput={parseImport}></textarea>
			{#if importForm.form.schemas && !isValidImport}
				<p class="mt-1 text-xs text-error">
					{importResult || 'Invalid collections configuration.'}
				</p>
			{/if}
		</div>

		<Switch
			class="mb-4"
			bind:checked={importForm.form.deleteMissing}
			disabled={!archiveFile && !isValidImport}
		>
			Delete missing collections and schema fields
		</Switch>

		<Switch
			class="mb-4"
			bind:checked={importForm.form.atomic}
			disabled={!archiveFile && !isValidImport}
		>
			Roll the whole import back if any collection fails
		</Switch>

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
						<span class="rounded bg-error px-1.5 py-0.5 text-[10px] font-semibold text-white"
							>Deleted</span
						>
						<span>{name}</span>
					</label>
				{/each}
				{#each importChanges.changed as name (name)}
					<label class="flex items-center gap-2 rounded-field bg-warning/20 px-3 py-1.5 text-sm">
						<span class="rounded bg-warning px-1.5 py-0.5 text-[10px] font-semibold text-white"
							>Changed</span
						>
						<span>{name}</span>
					</label>
				{/each}
				{#each importChanges.added as name (name)}
					<label class="flex items-center gap-2 rounded-field bg-success/20 px-3 py-1.5 text-sm">
						<span class="rounded bg-success px-1.5 py-0.5 text-[10px] font-semibold text-white"
							>Added</span
						>
						<span>{name}</span>
					</label>
				{/each}
			</div>
		{/if}

		<div class="field mb-4">
			<label class="field-label" for="import-password">Confirm your password</label>
			<input
				id="import-password"
				type="password"
				class="field-input"
				autocomplete="current-password"
				bind:value={importForm.form.password}
			/>
			<p class="mt-1 text-xs text-base-content/50">
				Importing rewrites your collections and records — confirm your superuser password.
			</p>
		</div>

		{#if progress !== null}
			<div class="mb-4">
				<div class="mb-1 flex justify-between text-xs text-base-content/60">
					<span>Uploading archive…</span>
					<span>{progress}%</span>
				</div>
				<progress class="progress progress-primary w-full" value={progress} max="100"></progress>
			</div>
		{/if}

		<div class="flex items-center justify-between">
			{#if importForm.form.schemas || archiveFile}
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
				disabled={!canImport || !importForm.form.password}
				loading={importing}
				onclick={() => doImport()}
			>
				Import
			</Button>
		</div>
	</div>

	<UndoImportButton class="mt-4" reloadToken={undoToken} />
{/if}

<Modal bind:show={confirmOpen} title="This import is large">
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
		<Button class="btn-warning" loading={importing} onclick={confirmAndImport}>
			Proceed without automatic undo
		</Button>
	</div>
</Modal>
