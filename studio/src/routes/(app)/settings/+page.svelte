<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import Button from '$lib/components/Button.svelte';

	type Section = 'application' | 'mail' | 'files' | 'backups' | 'cron' | 'export-import' | 'sql';

	const sections: { id: Section; label: string }[] = [
		{ id: 'application', label: 'Application' },
		{ id: 'mail', label: 'Mail Settings' },
		{ id: 'files', label: 'Files Storage' },
		{ id: 'backups', label: 'Backups' },
		{ id: 'cron', label: 'Cron' },
		{ id: 'export-import', label: 'Export & Import' },
		{ id: 'sql', label: 'SQL Console' }
	];

	let activeSection = $state<Section>('application');

	// ── Application settings ──
	let appName = $state('');
	let appSaving = $state(false);
	let appSaved = $state(false);

	// ── Mail settings ──
	let smtpHost = $state('');
	let smtpPort = $state('587');
	let smtpUser = $state('');
	let smtpPass = $state('');
	let smtpFrom = $state('');
	let smtpSecure = $state(false);
	let mailSaving = $state(false);
	let mailSaved = $state(false);

	// ── File storage settings ──
	let storageBackend = $state<'local' | 's3'>('local');
	let s3Bucket = $state('');
	let s3Region = $state('');
	let s3AccessKey = $state('');
	let s3SecretKey = $state('');
	let s3Endpoint = $state('');
	let storageSaving = $state(false);
	let storageSaved = $state(false);

	// ── Export/Import ──
	let exporting = $state(false);
	let exportData = $state<Record<string, unknown> | null>(null);
	let importFile = $state<File | undefined>();
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
			// Application
			appName = (res.app_name as string) ?? '';
			// Mail
			const mail = (res.mail as Record<string, string>) ?? {};
			smtpHost = mail.host ?? '';
			smtpPort = mail.port ?? '587';
			smtpUser = mail.user ?? '';
			smtpFrom = mail.from ?? '';
			const _secure = mail.secure as string | boolean | undefined;
			smtpSecure = _secure === 'true' || _secure === true;
			// File storage
			storageBackend = (res.storage_backend as 'local' | 's3') ?? 'local';
			const s3 = (res.s3 as Record<string, string>) ?? {};
			s3Bucket = s3.bucket ?? '';
			s3Region = s3.region ?? '';
			s3AccessKey = s3.access_key ?? '';
			s3SecretKey = s3.secret_key ?? '';
			s3Endpoint = s3.endpoint ?? '';
		} catch {
			// not configured
		}
	});

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
		mailSaved = false;
		try {
			await client.http.patch('/settings', {
				mail: smtpHost
					? { host: smtpHost, port: smtpPort, user: smtpUser, pass: smtpPass, from: smtpFrom, secure: smtpSecure }
					: null
			});
			mailSaved = true;
			setTimeout(() => (mailSaved = false), 2000);
		} catch {
			// ignore
		} finally {
			mailSaving = false;
		}
	}

	async function saveStorage() {
		storageSaving = true;
		storageSaved = false;
		try {
			await client.http.patch('/settings', {
				storage_backend: storageBackend,
				s3: storageBackend === 's3' ? { bucket: s3Bucket, region: s3Region, access_key: s3AccessKey, secret_key: s3SecretKey, endpoint: s3Endpoint } : null
			});
			storageSaved = true;
			setTimeout(() => (storageSaved = false), 2000);
		} catch {
			// ignore
		} finally {
			storageSaving = false;
		}
	}

	// ── Export ──
	async function doExport() {
		exporting = true;
		exportData = null;
		try {
			const res = (await client.http.get('/export')) as Record<string, unknown> | null;
			exportData = res;
		} catch {
			// ignore
		} finally {
			exporting = false;
		}
	}

	function downloadExport() {
		if (!exportData) return;
		const blob = new Blob([JSON.stringify(exportData, null, 2)], { type: 'application/json' });
		const url = URL.createObjectURL(blob);
		const a = document.createElement('a');
		a.href = url;
		a.download = `lazypock-export-${new Date().toISOString().slice(0, 10)}.json`;
		a.click();
		URL.revokeObjectURL(url);
	}

	// ── Import ──
	async function doImport() {
		if (!importFile) return;
		importing = true;
		importResult = null;
		try {
			const text = await importFile.text();
			const data = JSON.parse(text);
			const res = (await client.http.post('/import', data)) as Record<string, unknown> | null;
			if (res?.errors && (res.errors as unknown[]).length > 0) {
				importResult = `Imported ${(res.imported as unknown[]).length} collections with ${(res.errors as unknown[]).length} errors.`;
			} else {
				importResult = `Successfully imported ${(res?.imported as unknown[] ?? []).length} collections.`;
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
			const res = (await client.http.post('/sql/query', { sql: sqlQuery })) as Record<string, unknown> | null;
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

	// ── Backups ──
	async function doBackup() {
		backingUp = true;
		try {
			// Custom extension hook — generate SQL dump via export endpoint
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

<div class="flex gap-6">
	<!-- Left sidebar nav -->
	<nav class="w-48 shrink-0 space-y-1">
		{#each sections as s (s.id)}
			<button
				class="flex w-full cursor-pointer items-center gap-2 rounded-field border-none px-3 py-2 text-left text-sm transition-colors hover:bg-base-200"
				class:bg-primary={activeSection === s.id}
				class:text-primary-content={activeSection === s.id}
				onclick={() => (activeSection = s.id)}
			>
				{s.label}
			</button>
		{/each}
	</nav>

	<!-- Content -->
	<div class="min-w-0 flex-1">
		{#if activeSection === 'application'}
			<div class="mx-auto max-w-lg">
				<h2 class="mb-4 text-lg font-semibold">Application Settings</h2>
				<div class="rounded-box border border-base-300 bg-base-100 p-6">
					<div class="field mb-4">
						<label for="app-name" class="mb-1 block text-sm font-medium text-base-content/70">App Name</label>
						<input id="app-name" type="text" class="input w-full" placeholder="Lazypock" bind:value={appName} />
						<p class="mt-1 text-xs text-base-content/50">Displayed in the admin UI header.</p>
					</div>
					<div class="flex items-center gap-3">
						<Button class="btn-primary" loading={appSaving} disabled={appSaving} onclick={saveApp}>Save</Button>
						{#if appSaved}<span class="text-xs text-success">Saved!</span>{/if}
					</div>
				</div>
			</div>

		{:else if activeSection === 'mail'}
			<div class="mx-auto max-w-lg">
				<h2 class="mb-4 text-lg font-semibold">Mail Settings</h2>
				<div class="rounded-box border border-base-300 bg-base-100 p-6">
					<div class="mb-3 grid grid-cols-2 gap-3">
						<div class="field">
							<label for="smtp-host" class="mb-1 block text-xs font-medium text-base-content/70">SMTP Host</label>
							<input id="smtp-host" type="text" class="input w-full" placeholder="smtp.example.com" bind:value={smtpHost} />
						</div>
						<div class="field">
							<label for="smtp-port" class="mb-1 block text-xs font-medium text-base-content/70">Port</label>
							<input id="smtp-port" type="text" class="input w-full" placeholder="587" bind:value={smtpPort} />
						</div>
					</div>
					<div class="mb-3 grid grid-cols-2 gap-3">
						<div class="field">
							<label for="smtp-user" class="mb-1 block text-xs font-medium text-base-content/70">Username</label>
							<input id="smtp-user" type="text" class="input w-full" bind:value={smtpUser} />
						</div>
						<div class="field">
							<label for="smtp-pass" class="mb-1 block text-xs font-medium text-base-content/70">Password</label>
							<input id="smtp-pass" type="password" class="input w-full" bind:value={smtpPass} />
						</div>
					</div>
					<div class="mb-3 field">
						<label for="smtp-from" class="mb-1 block text-xs font-medium text-base-content/70">From Address</label>
						<input id="smtp-from" type="email" class="input w-full" placeholder="noreply@example.com" bind:value={smtpFrom} />
					</div>
					<div class="mb-4 flex items-center gap-2">
						<input id="smtp-secure" type="checkbox" bind:checked={smtpSecure} />
						<label for="smtp-secure" class="text-xs text-base-content/70">Use TLS/SSL</label>
					</div>
					<div class="flex items-center gap-3">
						<Button class="btn-primary" loading={mailSaving} disabled={mailSaving} onclick={saveMail}>Save</Button>
						{#if mailSaved}<span class="text-xs text-success">Saved!</span>{/if}
					</div>
				</div>
			</div>

		{:else if activeSection === 'files'}
			<div class="mx-auto max-w-lg">
				<h2 class="mb-4 text-lg font-semibold">Files Storage</h2>
				<div class="rounded-box border border-base-300 bg-base-100 p-6">
					<div class="mb-4 flex gap-4">
						<label class="flex cursor-pointer items-center gap-2">
							<input type="radio" name="storage" value="local" bind:group={storageBackend} />
							<span class="text-sm">Local Storage</span>
						</label>
						<label class="flex cursor-pointer items-center gap-2">
							<input type="radio" name="storage" value="s3" bind:group={storageBackend} />
							<span class="text-sm">S3 Compatible</span>
						</label>
					</div>

					{#if storageBackend === 's3'}
						<div class="mb-3 grid grid-cols-2 gap-3">
							<div class="field">
								<label class="mb-1 block text-xs font-medium text-base-content/70">S3 Bucket</label>
								<input type="text" class="input w-full" bind:value={s3Bucket} />
							</div>
							<div class="field">
								<label class="mb-1 block text-xs font-medium text-base-content/70">Region</label>
								<input type="text" class="input w-full" placeholder="us-east-1" bind:value={s3Region} />
							</div>
						</div>
						<div class="mb-3 grid grid-cols-2 gap-3">
							<div class="field">
								<label class="mb-1 block text-xs font-medium text-base-content/70">Access Key</label>
								<input type="text" class="input w-full" bind:value={s3AccessKey} />
							</div>
							<div class="field">
								<label class="mb-1 block text-xs font-medium text-base-content/70">Secret Key</label>
								<input type="password" class="input w-full" bind:value={s3SecretKey} />
							</div>
						</div>
						<div class="mb-4 field">
							<label class="mb-1 block text-xs font-medium text-base-content/70">Endpoint (optional)</label>
							<input type="text" class="input w-full" placeholder="https://s3.amazonaws.com" bind:value={s3Endpoint} />
						</div>
					{/if}

					<div class="flex items-center gap-3">
						<Button class="btn-primary" loading={storageSaving} disabled={storageSaving} onclick={saveStorage}>Save</Button>
						{#if storageSaved}<span class="text-xs text-success">Saved!</span>{/if}
					</div>
				</div>
			</div>

		{:else if activeSection === 'backups'}
			<div class="mx-auto max-w-lg">
				<h2 class="mb-4 text-lg font-semibold">Backups</h2>
				<div class="rounded-box border border-base-300 bg-base-100 p-6">
					<p class="mb-4 text-sm text-base-content/70">
						Download a full JSON backup of all collections and their data.
					</p>
					<Button class="btn-primary" loading={backingUp} onclick={doBackup}
						>Download Backup</Button
					>
				</div>
			</div>

		{:else if activeSection === 'cron'}
			<div class="mx-auto max-w-lg">
				<h2 class="mb-4 text-lg font-semibold">Cron Jobs</h2>
				<div class="rounded-box border border-base-300 bg-base-100 p-6">
					<div class="flex flex-col gap-3 text-sm text-base-content/70">
						<p>Cron job scheduling is not yet implemented.</p>
						<p>Planned features:</p>
						<ul class="list-inside list-disc space-y-1">
							<li>Periodic backup scheduling</li>
							<li>Log rotation</li>
							<li>Webhook-based job triggers</li>
						</ul>
					</div>
				</div>
			</div>

		{:else if activeSection === 'export-import'}
			<div class="mx-auto max-w-lg">
				<h2 class="mb-4 text-lg font-semibold">Export & Import Collections</h2>

				<div class="mb-4 rounded-box border border-base-300 bg-base-100 p-6">
					<h3 class="mb-3 text-sm font-medium">Export</h3>
					<p class="mb-3 text-xs text-base-content/60">
						Export all collections, their schema, rules, and records as JSON.
					</p>
					{#if exportData}
						<div class="mb-3">
							<p class="mb-1 text-xs text-success">
								Export ready ({(exportData.collections as unknown[]).length} collections)
							</p>
							<Button class="btn-primary btn-sm" onclick={downloadExport}>Download JSON</Button>
						</div>
					{/if}
					<Button class="btn-primary" loading={exporting} onclick={doExport}>
						{exportData ? 'Re-export' : 'Export All'}
					</Button>
				</div>

				<div class="rounded-box border border-base-300 bg-base-100 p-6">
					<h3 class="mb-3 text-sm font-medium">Import</h3>
					<p class="mb-3 text-xs text-base-content/60">
						Import collections from a previously exported JSON file. Existing collections with
						the same name will be skipped.
					</p>
					<input
						type="file"
						accept=".json"
						class="mb-3 block w-full text-sm file:mr-3 file:cursor-pointer file:rounded-field file:border-0 file:bg-primary file:px-3 file:py-1.5 file:text-sm file:text-primary-content"
						onchange={(e) => (importFile = (e.target as HTMLInputElement).files?.[0])}
					/>
					<Button class="btn-primary" loading={importing} disabled={!importFile} onclick={doImport}
						>Import</Button
					>
					{#if importResult}
						<p class="mt-2 text-xs text-base-content/60">{importResult}</p>
					{/if}
				</div>
			</div>

		{:else if activeSection === 'sql'}
			<div class="mx-auto max-w-3xl">
				<h2 class="mb-4 text-lg font-semibold">SQL Console</h2>
				<div class="rounded-box border border-base-300 bg-base-100 p-6">
					<p class="mb-3 text-xs text-base-content/60">
						Run read-only SQL queries against the database. Only SELECT, EXPLAIN, and WITH statements are allowed.
					</p>
					<textarea
						class="input w-full font-mono text-xs"
						rows="4"
						placeholder="SELECT * FROM _collections"
						bind:value={sqlQuery}
					></textarea>
					<div class="mt-2 flex items-center gap-2">
						<Button class="btn-primary btn-sm" loading={sqlRunning} onclick={runSql}
							>Run Query</Button
						>
						<button
							class="cursor-pointer border-none bg-transparent text-xs text-base-content/50 hover:text-base-content"
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
								<tr class="bg-base-200 text-left text-xs font-semibold uppercase text-base-content/60">
									{#each sqlResults.columns as col (col)}
										<th class="border-b border-base-300 px-3 py-2 font-mono">{col}</th>
									{/each}
								</tr>
							</thead>
							<tbody>
								{#each sqlResults.rows as row, i (String(i))}
									<tr class="hover:bg-base-200 {i % 2 === 1 ? 'bg-base-100/50' : ''}">
										{#each row as cell (cell)}

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
	</div>
</div>

<style>
	input[type='radio'] {
		accent-color: var(--color-primary);
	}
</style>
