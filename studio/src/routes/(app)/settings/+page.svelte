<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import { slide } from 'svelte/transition';
	import Button from '$lib/components/Button.svelte';
	import Input from '$lib/components/Input.svelte';
	import Sidebar from '$lib/components/Sidebar.svelte';
	import { RefreshCw } from '@lucide/svelte';
	import {
		Archive,
		Check,
		Clipboard,
		Cog,
		Database,
		Download,
		HardDrive,
		Mail,
		Upload
	} from '@lucide/svelte';

	type Section =
		'application' | 'mail' | 'files' | 'backups' | 'cron' | 'export' | 'import' | 'sql';

	interface SectionItem {
		id: Section;
		label: string;
		icon: typeof Cog;
	}

	const systemSections: SectionItem[] = [
		{ id: 'application', label: 'Application', icon: Cog },
		{ id: 'mail', label: 'Mail', icon: Mail },
		{ id: 'files', label: 'Files Storage', icon: HardDrive },
		{ id: 'backups', label: 'Backups', icon: Archive },
		{ id: 'cron', label: 'Cron', icon: RefreshCw }
	];

	const syncSections: SectionItem[] = [
		{ id: 'export', label: 'Export Collections', icon: Upload },
		{ id: 'import', label: 'Import Collections', icon: Download }
	];

	const debugSections: SectionItem[] = [{ id: 'sql', label: 'SQL Console', icon: Database }];

	let activeSection = $state<Section>('application');

	// ── Application settings ──
	let appName = $state('');
	let appSaving = $state(false);
	let appSaved = $state(false);

	// ── Mail settings ──
	let mailEnabled = $state(false);
	let senderName = $state('');
	let senderAddress = $state('');
	let smtpHost = $state('');
	let smtpPort = $state('587');
	let smtpUser = $state('');
	let smtpPass = $state('');
	let smtpTls = $state(false);
	let smtpAuthMethod = $state('PLAIN');
	let smtpLocalName = $state('');
	let showMoreMail = $state(false);
	let mailSaving = $state(false);

	// ── File storage settings ──
	let storageEnabled = $state(false);
	let s3Bucket = $state('');
	let s3Region = $state('us-east-1');
	let s3AccessKey = $state('');
	let s3SecretKey = $state('');
	let s3Endpoint = $state('');
	let storageSaving = $state(false);

	// ── Export ──
	let collectionsList = $state<{ id: string; name: string; type: string }[]>([]);
	let loadingCollections = $state(false);
	let selectedExports = $state<Record<string, { id: string; name: string; type: string }>>({});
	let exportCopied = $state(false);

	// ── Import ──
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

	// ── SQL Console ──
	let sqlQuery = $state('SELECT name, type FROM _collections ORDER BY name');
	let sqlResults = $state<{ columns: string[]; rows: unknown[][] } | null>(null);
	let sqlError = $state('');
	let sqlRunning = $state(false);

	// ── Backups ──
	let backingUp = $state(false);

	onMount(async () => {
		try {
			const res = (await client.http.get('/settings')) as Record<string, unknown> | null;
			if (!res) return;
			appName = (res.app_name as string) ?? '';

			const mail = (res.mail as Record<string, unknown>) ?? {};
			mailEnabled = (mail.enabled as boolean) ?? false;
			senderName = (mail.sender_name as string) ?? '';
			senderAddress = (mail.sender_address as string) ?? '';
			smtpHost = (mail.host as string) ?? '';
			smtpPort = (mail.port as string) ?? '587';
			smtpUser = (mail.user as string) ?? '';
			smtpPass = (mail.pass as string) ?? '';
			smtpTls = (mail.tls as boolean) ?? false;
			smtpAuthMethod = (mail.auth_method as string) ?? 'PLAIN';
			smtpLocalName = (mail.local_name as string) ?? '';

			storageEnabled = (res.s3_enabled as boolean) ?? false;
			const s3 = (res.s3 as Record<string, string>) ?? {};
			s3Bucket = s3.bucket ?? '';
			s3Region = s3.region ?? 'us-east-1';
			s3AccessKey = s3.access_key ?? '';
			s3SecretKey = s3.secret_key ?? '';
			s3Endpoint = s3.endpoint ?? '';
		} catch {
			// not configured
		}
	});

	// ── Sidebar body ──

	// ── Save helpers ──

	async function saveApp() {
		appSaving = true;
		appSaved = false;
		try {
			await client.http.patch('/settings', { app_name: appName || null });
			appSaved = true;
			setTimeout(() => (appSaved = false), 2000);
		} catch {
			// ignore
		} finally {
			appSaving = false;
		}
	}

	async function saveMail() {
		mailSaving = true;
		try {
			await client.http.patch('/settings', {
				mail: {
					enabled: mailEnabled,
					sender_name: senderName || null,
					sender_address: senderAddress || null,
					host: smtpHost || null,
					port: smtpPort || null,
					user: smtpUser || null,
					pass: smtpPass || null,
					tls: smtpTls,
					auth_method: smtpAuthMethod,
					local_name: smtpLocalName || null
				}
			});
		} catch {
			// ignore
		} finally {
			mailSaving = false;
		}
	}

	async function saveStorage() {
		storageSaving = true;
		try {
			await client.http.patch('/settings', {
				s3_enabled: storageEnabled,
				s3: storageEnabled
					? {
							bucket: s3Bucket,
							region: s3Region,
							access_key: s3AccessKey,
							secret_key: s3SecretKey,
							endpoint: s3Endpoint
						}
					: null
			});
		} catch {
			// ignore
		} finally {
			storageSaving = false;
		}
	}

	// ── Export ──
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

	// ── Import ──

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

	// ── SQL Console ──
	async function runSql() {
		if (!sqlQuery.trim()) return;
		sqlRunning = true;
		sqlError = '';
		sqlResults = null;
		try {
			const res = (await client.http.post('/sql/query', { sql: sqlQuery })) as Record<
				string,
				unknown
			> | null;
			if (res?.error) {
				sqlError = res.error as string;
			} else {
				sqlResults = res as { columns: string[]; rows: unknown[][] };
			}
		} catch (e) {
			sqlError = (e as Error).message;
		} finally {
			sqlRunning = false;
		}
	}

	// ── Load collections on mount ──
	$effect(() => {
		if (activeSection === 'export' && collectionsList.length === 0) {
			loadCollections();
		}
	});

	// ── Backups ──
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
		} catch {
			// ignore
		} finally {
			backingUp = false;
		}
	}
</script>

{#snippet bodyContent()}
	<div
		class="mb-1 px-3.5 pt-2 text-[10px] font-semibold tracking-wider text-base-content/40 uppercase"
	>
		System
	</div>
	{#each systemSections as s (s.id)}
		<button
			class="mx-1.5 flex w-[calc(100%-12px)] cursor-pointer items-center gap-2 rounded-field border-none px-3 py-1.5 text-left text-sm text-base-content transition-[background] duration-(--animation-speed-fast) hover:bg-base-200"
			class:bg-base-200={s.id === activeSection}
			class:font-medium={s.id === activeSection}
			role="button"
			tabindex="0"
			onclick={() => (activeSection = s.id)}
			onkeydown={(e) => {
				if (e.key === 'Enter') activeSection = s.id;
			}}
		>
			<s.icon class="h-4 w-4 shrink-0 opacity-60" />
			<span class="flex-1 truncate">{s.label}</span>
		</button>
	{/each}

	<div
		class="mt-2 mb-1 px-3.5 pt-2 text-[10px] font-semibold tracking-wider text-base-content/40 uppercase"
	>
		Sync
	</div>
	{#each syncSections as s (s.id)}
		<button
			class="mx-1.5 flex w-[calc(100%-12px)] cursor-pointer items-center gap-2 rounded-field border-none px-3 py-1.5 text-left text-sm text-base-content transition-[background] duration-(--animation-speed-fast) hover:bg-base-200"
			class:bg-base-200={s.id === activeSection}
			class:font-medium={s.id === activeSection}
			role="button"
			tabindex="0"
			onclick={() => (activeSection = s.id)}
			onkeydown={(e) => {
				if (e.key === 'Enter') activeSection = s.id;
			}}
		>
			<s.icon class="h-4 w-4 shrink-0 opacity-60" />
			<span class="flex-1 truncate">{s.label}</span>
		</button>
	{/each}

	<div
		class="mt-2 mb-1 px-3.5 pt-2 text-[10px] font-semibold tracking-wider text-base-content/40 uppercase"
	>
		Debug
	</div>
	{#each debugSections as s (s.id)}
		<button
			class="mx-1.5 flex w-[calc(100%-12px)] cursor-pointer items-center gap-2 rounded-field border-none px-3 py-1.5 text-left text-sm text-base-content transition-[background] duration-(--animation-speed-fast) hover:bg-base-200"
			class:bg-base-200={s.id === activeSection}
			class:font-medium={s.id === activeSection}
			role="button"
			tabindex="0"
			onclick={() => (activeSection = s.id)}
			onkeydown={(e) => {
				if (e.key === 'Enter') activeSection = s.id;
			}}
		>
			<s.icon class="h-4 w-4 shrink-0 opacity-60" />
			<span class="flex-1 truncate">{s.label}</span>
		</button>
	{/each}
{/snippet}

<!-- Layout: sidebar left, main content right -->
<div class="flex flex-1 overflow-hidden">
	<Sidebar body={bodyContent} />
	<main class="flex-1 overflow-auto p-6">
		<div
			class="mx-auto"
			style="max-width: {activeSection === 'export' || activeSection === 'import'
				? '1200px'
				: '870px'}"
		>
			{#if activeSection === 'application'}
				<h2 class="mb-4 text-lg font-semibold">Application Settings</h2>
				<div class="rounded-box border border-base-300 bg-base-100 p-6">
					<Input
						label="App Name"
						placeholder="Lazypock"
						bind:value={appName}
						help="Displayed in the admin UI header."
					/>
					<div class="mt-4 flex items-center gap-3">
						<Button class="btn-primary" loading={appSaving} disabled={appSaving} onclick={saveApp}
							>Save</Button
						>
						{#if appSaved}<span class="text-xs text-success">Saved!</span>{/if}
					</div>
				</div>
			{:else if activeSection === 'mail'}
				<h2 class="mb-4 text-lg font-semibold">Mail Settings</h2>
				<div class="rounded-box border border-base-300 bg-base-100 p-6">
					<div class="mb-4 text-sm text-base-content/60">
						<p>Configure common settings for sending emails.</p>
					</div>

					<div class="mb-6 flex flex-col gap-3 sm:flex-row sm:items-start">
						<div class="flex-1">
							<Input label="Sender name" placeholder="John Doe" bind:value={senderName} />
						</div>
						<div class="flex-1">
							<Input
								label="Sender address"
								placeholder="noreply@example.com"
								type="email"
								bind:value={senderAddress}
							/>
						</div>
					</div>

					<!-- SMTP toggle -->
					<div class="switch-field mb-4">
						<label class="switch-label" for="mail-enabled">
							<span class="txt">Use SMTP mail server <strong>(recommended)</strong></span>
						</label>
						<label class="switch">
							<input id="mail-enabled" type="checkbox" bind:checked={mailEnabled} />
							<span class="switch-slider"></span>
						</label>
					</div>

					{#if mailEnabled}
						<div transition:slide={{ duration: 150 }}>
							<div class="flex flex-col gap-3 sm:flex-row">
								<div class="flex-[5]">
									<Input
										label="SMTP server host"
										placeholder="smtp.example.com"
										bind:value={smtpHost}
										required
									/>
								</div>
								<div class="flex-[3]">
									<Input label="Port" placeholder="587" bind:value={smtpPort} required />
								</div>
								<div class="flex-[4]">
									<Input label="Username" bind:value={smtpUser} />
								</div>
								<div class="flex-[4]">
									<Input label="Password" type="password" bind:value={smtpPass} />
								</div>
							</div>

							<button
								type="button"
								class="mt-2 mb-4 cursor-pointer border-none bg-transparent text-sm text-base-content/50 hover:text-base-content"
								onclick={() => (showMoreMail = !showMoreMail)}
							>
								{showMoreMail ? 'Hide more options' : 'Show more options'}
							</button>

							{#if showMoreMail}
								<div transition:slide={{ duration: 150 }}>
									<div class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-12">
										<div class="lg:col-span-4">
											<div class="field">
												<label class="field-label">TLS encryption</label>
												<select class="field-input" bind:value={smtpTls}>
													<option value={false}>Auto (StartTLS)</option>
													<option value={true}>Always</option>
												</select>
											</div>
										</div>
										<div class="lg:col-span-4">
											<div class="field">
												<label class="field-label">AUTH method</label>
												<select class="field-input" bind:value={smtpAuthMethod}>
													<option value="PLAIN">PLAIN (default)</option>
													<option value="LOGIN">LOGIN</option>
												</select>
											</div>
										</div>
										<div class="lg:col-span-4">
											<Input
												label="EHLO/HELO domain"
												placeholder="Default to localhost"
												bind:value={smtpLocalName}
											/>
										</div>
									</div>
								</div>
							{/if}
						</div>
					{/if}

					<div class="mt-6 flex items-center justify-end gap-3">
						<Button class="btn-primary" loading={mailSaving} onclick={saveMail}>Save changes</Button
						>
					</div>
				</div>
			{:else if activeSection === 'files'}
				<h2 class="mb-4 text-lg font-semibold">Files Storage</h2>
				<div class="rounded-box border border-base-300 bg-base-100 p-6">
					<div class="mb-4 text-sm text-base-content/60">
						<p>By default Lazypock uses the local file system to store uploaded files.</p>
						<p>
							If you have limited disk space, you could optionally connect to an S3 compatible
							storage.
						</p>
					</div>

					<!-- S3 toggle -->
					<div class="switch-field mb-4">
						<label class="switch-label" for="storage-enabled">
							<span class="txt">Use S3 storage</span>
						</label>
						<label class="switch">
							<input id="storage-enabled" type="checkbox" bind:checked={storageEnabled} />
							<span class="switch-slider"></span>
						</label>
					</div>

					{#if storageEnabled}
						<div transition:slide={{ duration: 150 }}>
							<div class="flex flex-col gap-3">
								<div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
									<Input label="S3 Bucket" placeholder="my-bucket" bind:value={s3Bucket} required />
									<Input label="Region" placeholder="us-east-1" bind:value={s3Region} />
								</div>
								<div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
									<Input label="Access Key" bind:value={s3AccessKey} />
									<Input label="Secret Key" type="password" bind:value={s3SecretKey} />
								</div>
								<Input
									label="Endpoint (optional)"
									placeholder="https://s3.amazonaws.com"
									bind:value={s3Endpoint}
								/>
							</div>
						</div>
					{/if}

					<div class="flex items-center justify-end gap-3">
						<Button class="btn-primary" loading={storageSaving} onclick={saveStorage}
							>Save changes</Button
						>
					</div>
				</div>
			{:else if activeSection === 'backups'}
				<h2 class="mb-4 text-lg font-semibold">Backups</h2>
				<div class="rounded-box border border-base-300 bg-base-100 p-6">
					<p class="mb-4 text-sm text-base-content/70">
						Download a full JSON backup of all collections and their data.
					</p>
					<Button class="btn-primary" loading={backingUp} onclick={doBackup}>Download Backup</Button
					>
				</div>
			{:else if activeSection === 'cron'}
				<h2 class="mb-4 text-lg font-semibold">Cron</h2>
				<div class="rounded-box border border-base-300 bg-base-100 p-6">
					<p class="text-sm text-base-content/60">Cron job scheduling coming soon.</p>
				</div>
			{:else if activeSection === 'export'}
				<h2 class="mb-4 text-lg font-semibold">Export Collections</h2>
				<div class="mb-4 text-sm text-base-content/60">
					<p>
						Below you'll find your current collections configuration that you could import in
						another environment.
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
										<span class="flex items-center gap-1 text-success"
											><Check class="h-3 w-3" />Copied</span
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
			{:else if activeSection === 'import'}
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
									<label
										class="flex items-center gap-2 rounded-field bg-error/20 px-3 py-1.5 text-sm"
									>
										<span
											class="text-white rounded bg-error px-1.5 py-0.5 text-[10px] font-semibold"
											>Deleted</span
										>
										<span>{name}</span>
									</label>
								{/each}
								{#each importChanges.changed as name (name)}
									<label
										class="flex items-center gap-2 rounded-field bg-warning/20 px-3 py-1.5 text-sm"
									>
										<span
											class="text-white rounded bg-warning px-1.5 py-0.5 text-[10px] font-semibold"
											>Changed</span
										>
										<span>{name}</span>
									</label>
								{/each}
								{#each importChanges.added as name (name)}
									<label
										class="flex items-center gap-2 rounded-field bg-success/20 px-3 py-1.5 text-sm"
									>
										<span
											class="text-white rounded bg-success px-1.5 py-0.5 text-[10px] font-semibold"
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
			{/if}
		</div>

		<!-- SQL Console is full-width -->
		{#if activeSection === 'sql'}
			<div class="mx-auto" style="max-width:100%">
				<h2 class="mb-4 text-lg font-semibold">SQL Console</h2>
				<div class="rounded-box border border-base-300 bg-base-100 p-6">
					<p class="mb-3 text-xs text-base-content/60">
						Run read-only SQL queries against the database. Only SELECT, EXPLAIN, and WITH
						statements are allowed.
					</p>
					<textarea
						class="input w-full font-mono text-xs outline-none focus:outline-none"
						rows="12"
						placeholder="SELECT * FROM _collections"
						bind:value={sqlQuery}></textarea>
					<div class="mt-2 flex items-center gap-2">
						<Button class="btn-primary" loading={sqlRunning} onclick={runSql}>Run Query</Button>
						<button
							class="cursor-pointer border-none bg-transparent text-sm text-base-content/50 hover:text-base-content"
							onclick={() => {
								sqlQuery = 'SELECT name, type FROM _collections ORDER BY name';
								sqlResults = null;
								sqlError = '';
							}}
						>
							Reset
						</button>
					</div>
				</div>

				{#if sqlError}
					<div class="mt-3 rounded-box border border-error/30 bg-error/10 p-3 text-xs text-error">
						{sqlError}
					</div>
				{/if}

				{#if sqlResults}
					<div class="mt-3 overflow-x-auto rounded-box border border-base-300 bg-base-100">
						<table class="w-full border-collapse text-xs">
							<thead>
								<tr
									class="bg-base-200 text-left text-xs font-semibold text-base-content/60 uppercase"
								>
									{#each sqlResults.columns as col (col)}
										<th class="border-b border-base-300 px-3 py-2 font-mono">{col}</th>
									{/each}
								</tr>
							</thead>
							<tbody>
								{#each sqlResults.rows as row, i (String(i))}
									<tr class="hover:bg-base-200 {i % 2 === 1 ? 'bg-base-100/50' : ''}">
										{#each row as cell, j (j)}
											<td class="max-w-60 truncate border-b border-base-200 px-3 py-1.5 font-mono">
												{cell == null ? 'NULL' : String(cell)}
											</td>
										{/each}
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				{/if}
			</div>
		{/if}
	</main>
</div>

<style>
	/* input[type='radio'] {
		accent-color: var(--color-primary);
	} */
	.checkbox {
		accent-color: var(--color-primary);
	}

	/* ── Switch toggle ── */
	.switch-field {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		padding: 12px;
		border-radius: var(--radius-field);
		background: color-mix(in oklab, var(--color-base-content) 8%, var(--color-base-100));
		transition: background var(--animation-speed, 0.2s);
	}
	.switch-field:focus-within {
		background: color-mix(in oklab, var(--color-base-content) 12%, var(--color-base-100));
	}
	.switch-label {
		flex: 1;
		cursor: pointer;
		font-size: 0.9375rem;
		color: var(--color-base-content);
	}
	.switch-label .txt {
		opacity: 0.85;
	}
	.switch {
		position: relative;
		display: inline-block;
		width: 44px;
		height: 24px;
		flex-shrink: 0;
	}
	.switch input {
		opacity: 0;
		width: 0;
		height: 0;
		position: absolute;
	}
	.switch-slider {
		position: absolute;
		cursor: pointer;
		inset: 0;
		background: color-mix(in oklab, var(--color-base-content) 25%, transparent);
		border-radius: 24px;
		transition: 0.2s;
	}
	.switch-slider::before {
		content: '';
		position: absolute;
		height: 18px;
		width: 18px;
		left: 3px;
		bottom: 3px;
		background: white;
		border-radius: 50%;
		transition: 0.2s;
	}
	.switch input:checked + .switch-slider {
		background: var(--color-primary);
	}
	.switch input:checked + .switch-slider::before {
		transform: translateX(20px);
	}
	.switch input:disabled + .switch-slider {
		opacity: 0.4;
		cursor: not-allowed;
	}

	/* ── Select field (for TLS/Auth) ── */
	.field {
		position: relative;
		display: block;
		outline: 0;
		width: 100%;
		min-width: 0;
		border-radius: var(--radius-field);
		background: color-mix(in oklab, var(--color-base-content) 8%, var(--color-base-100));
		transition: background var(--animation-speed, 0.2s);
	}
	.field:focus-within {
		background: color-mix(in oklab, var(--color-base-content) 12%, var(--color-base-100));
	}
	.field-label {
		display: flex;
		width: 100%;
		gap: 5px;
		align-items: center;
		min-height: 24px;
		padding: 9px 12px 1px;
		font-weight: bold;
		white-space: normal;
		opacity: 0.7;
		font-size: 0.875rem;
		transition: color var(--animation-speed, 0.2s);
		color: var(--color-base-content);
	}
	.field-input {
		display: inline-block;
		vertical-align: top;
		outline: 0;
		border: 0;
		margin: 0;
		width: 100%;
		background: none;
		font-weight: normal;
		line-height: 1;
		letter-spacing: inherit;
		padding: 10px 12px;
		color: var(--color-base-content);
		font-size: 0.9375rem;
		font-family: var(--font-sans, system-ui, sans-serif);
	}
	.field-input:focus,
	.field-input:focus-visible,
	.field-input:focus-within {
		outline: 0;
	}
	select.field-input {
		appearance: none;
		cursor: pointer;
	}

	/* ── Grid helpers ── */
	.lg\:col-span-4 {
		grid-column: span 4 / span 4;
	}
</style>
