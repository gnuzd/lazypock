<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import Button from '$lib/components/Button.svelte';
	import DataTable from '$lib/components/DataTable.svelte';
	import Input from '$lib/components/Input.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Select from '$lib/components/Select.svelte';
	import Tabs from '$lib/components/Tabs.svelte';
	import { Trash2, Play, Pencil } from '@lucide/svelte';
	import '../settings.css';

	type CronJob = {
		id: string;
		name: string;
		expression: string;
		timezone: string;
		enabled: boolean;
		action: 'http' | 'sql' | 'hook';
		config: Record<string, unknown>;
		last_run_at: string | null;
		last_status: 'ok' | 'error' | 'skipped' | null;
		last_duration_ms: number | null;
		last_error: string | null;
		next_run_at: string | null;
	};

	const timezoneOptions = [
		{ value: 'UTC', label: 'UTC' },
		{ value: 'America/New_York', label: 'New York (America/New_York)' },
		{ value: 'America/Chicago', label: 'Chicago (America/Chicago)' },
		{ value: 'America/Denver', label: 'Denver (America/Denver)' },
		{ value: 'America/Los_Angeles', label: 'Los Angeles (America/Los_Angeles)' },
		{ value: 'America/Phoenix', label: 'Phoenix (America/Phoenix)' },
		{ value: 'America/Anchorage', label: 'Anchorage (America/Anchorage)' },
		{ value: 'America/Toronto', label: 'Toronto (America/Toronto)' },
		{ value: 'America/Vancouver', label: 'Vancouver (America/Vancouver)' },
		{ value: 'America/Mexico_City', label: 'Mexico City (America/Mexico_City)' },
		{ value: 'America/Sao_Paulo', label: 'São Paulo (America/Sao_Paulo)' },
		{ value: 'America/Buenos_Aires', label: 'Buenos Aires (America/Buenos_Aires)' },
		{ value: 'Europe/London', label: 'London (Europe/London)' },
		{ value: 'Europe/Dublin', label: 'Dublin (Europe/Dublin)' },
		{ value: 'Europe/Lisbon', label: 'Lisbon (Europe/Lisbon)' },
		{ value: 'Europe/Paris', label: 'Paris (Europe/Paris)' },
		{ value: 'Europe/Berlin', label: 'Berlin (Europe/Berlin)' },
		{ value: 'Europe/Madrid', label: 'Madrid (Europe/Madrid)' },
		{ value: 'Europe/Rome', label: 'Rome (Europe/Rome)' },
		{ value: 'Europe/Amsterdam', label: 'Amsterdam (Europe/Amsterdam)' },
		{ value: 'Europe/Stockholm', label: 'Stockholm (Europe/Stockholm)' },
		{ value: 'Europe/Oslo', label: 'Oslo (Europe/Oslo)' },
		{ value: 'Europe/Copenhagen', label: 'Copenhagen (Europe/Copenhagen)' },
		{ value: 'Europe/Warsaw', label: 'Warsaw (Europe/Warsaw)' },
		{ value: 'Europe/Zurich', label: 'Zurich (Europe/Zurich)' },
		{ value: 'Europe/Vienna', label: 'Vienna (Europe/Vienna)' },
		{ value: 'Europe/Athens', label: 'Athens (Europe/Athens)' },
		{ value: 'Europe/Helsinki', label: 'Helsinki (Europe/Helsinki)' },
		{ value: 'Europe/Istanbul', label: 'Istanbul (Europe/Istanbul)' },
		{ value: 'Europe/Moscow', label: 'Moscow (Europe/Moscow)' },
		{ value: 'Africa/Cairo', label: 'Cairo (Africa/Cairo)' },
		{ value: 'Africa/Lagos', label: 'Lagos (Africa/Lagos)' },
		{ value: 'Africa/Nairobi', label: 'Nairobi (Africa/Nairobi)' },
		{ value: 'Africa/Johannesburg', label: 'Johannesburg (Africa/Johannesburg)' },
		{ value: 'Asia/Dubai', label: 'Dubai (Asia/Dubai)' },
		{ value: 'Asia/Riyadh', label: 'Riyadh (Asia/Riyadh)' },
		{ value: 'Asia/Kolkata', label: 'Kolkata (Asia/Kolkata)' },
		{ value: 'Asia/Bangkok', label: 'Bangkok (Asia/Bangkok)' },
		{ value: 'Asia/Singapore', label: 'Singapore (Asia/Singapore)' },
		{ value: 'Asia/Kuala_Lumpur', label: 'Kuala Lumpur (Asia/Kuala_Lumpur)' },
		{ value: 'Asia/Hong_Kong', label: 'Hong Kong (Asia/Hong_Kong)' },
		{ value: 'Asia/Shanghai', label: 'Shanghai (Asia/Shanghai)' },
		{ value: 'Asia/Taipei', label: 'Taipei (Asia/Taipei)' },
		{ value: 'Asia/Seoul', label: 'Seoul (Asia/Seoul)' },
		{ value: 'Asia/Tokyo', label: 'Tokyo (Asia/Tokyo)' },
		{ value: 'Asia/Jerusalem', label: 'Jerusalem (Asia/Jerusalem)' },
		{ value: 'Asia/Karachi', label: 'Karachi (Asia/Karachi)' },
		{ value: 'Asia/Manila', label: 'Manila (Asia/Manila)' },
		{ value: 'Australia/Perth', label: 'Perth (Australia/Perth)' },
		{ value: 'Australia/Brisbane', label: 'Brisbane (Australia/Brisbane)' },
		{ value: 'Australia/Sydney', label: 'Sydney (Australia/Sydney)' },
		{ value: 'Australia/Melbourne', label: 'Melbourne (Australia/Melbourne)' },
		{ value: 'Pacific/Auckland', label: 'Auckland (Pacific/Auckland)' },
		{ value: 'Pacific/Honolulu', label: 'Honolulu (Pacific/Honolulu)' }
	];

	const actionTabs = ['HTTP Request', 'SQL', 'Hook'];

	const httpMethodOptions = [
		{ value: 'GET', label: 'GET' },
		{ value: 'POST', label: 'POST' },
		{ value: 'PUT', label: 'PUT' },
		{ value: 'PATCH', label: 'PATCH' },
		{ value: 'DELETE', label: 'DELETE' },
		{ value: 'HEAD', label: 'HEAD' }
	];

	let jobs = $state<CronJob[]>([]);
	let busy = $state(false);
	let error = $state('');

	// ── Editor form state ────────────────────────────
	let showEditor = $state(false);
	let editingId = $state<string | null>(null);
	let formError = $state('');
	let saving = $state(false);
	let actionTab = $state('HTTP Request');
	let preview = $state<{ runs: string[]; error: string; loading: boolean }>({
		runs: [],
		error: '',
		loading: false
	});

	let form = $state({
		name: '',
		expression: '0 0 * * *',
		timezone: 'UTC',
		enabled: true,
		action: 'http' as 'http' | 'sql' | 'hook',
		method: 'GET',
		url: '',
		headers: [{ key: '', value: '' }] as { key: string; value: string }[],
		body: '',
		statement: '',
		hookEvent: ''
	});

	onMount(refresh);

	async function refresh() {
		try {
			const res = (await client.http.get('/crons')) as { items?: CronJob[] } | null;
			jobs = res?.items ?? [];
			error = '';
		} catch {
			jobs = [];
		}
	}

	function formatDate(iso: string | null | undefined): string {
		if (!iso) return 'Never';
		try {
			return new Date(iso).toLocaleString();
		} catch {
			return iso;
		}
	}

	function formatDuration(ms: number | null): string {
		if (ms == null) return '';
		return ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(1)}s`;
	}

	// ── Editor open / close ──────────────────────────
	function openNew() {
		editingId = null;
		form = {
			name: '',
			expression: '0 0 * * *',
			timezone: 'UTC',
			enabled: true,
			action: 'http',
			method: 'GET',
			url: '',
			headers: [{ key: '', value: '' }],
			body: '',
			statement: '',
			hookEvent: ''
		};
		actionTab = 'HTTP Request';
		formError = '';
		preview = { runs: [], error: '', loading: false };
		showEditor = true;
	}

	function openEdit(job: CronJob) {
		const cfg = job.config ?? {};
		editingId = job.id;
		form = {
			name: job.name,
			expression: job.expression,
			timezone: job.timezone || 'UTC',
			enabled: job.enabled,
			action: job.action,
			method: (cfg.method as string) || 'GET',
			url: (cfg.url as string) || '',
			headers: (cfg.headers as { key: string; value: string }[])?.length
				? (cfg.headers as { key: string; value: string }[])
				: [{ key: '', value: '' }],
			body: (cfg.body as string) || '',
			statement: (cfg.statement as string) || '',
			hookEvent: (cfg.event as string) || ''
		};
		actionTab = job.action === 'http' ? 'HTTP Request' : job.action === 'sql' ? 'SQL' : 'Hook';
		formError = '';
		preview = { runs: [], error: '', loading: false };
		showEditor = true;
	}

	function closeEditor() {
		showEditor = false;
	}

	// ── Expression preview (live, debounced) ─────────
	let previewSeq = 0;
	let previewTimer: ReturnType<typeof setTimeout> | undefined;

	function updatePreview() {
		const expr = form.expression.trim();
		const seq = ++previewSeq;
		if (!expr) {
			preview = { runs: [], error: 'Expression is required', loading: false };
			return;
		}
		preview = { ...preview, loading: true };
		if (previewTimer) clearTimeout(previewTimer);
		previewTimer = setTimeout(async () => {
			try {
				const res = (await client.http.post('/crons/validate', {
					expression: expr,
					timezone: form.timezone
				})) as { valid?: boolean; error?: string; nextRuns?: string[] } | null;
				if (seq !== previewSeq) return; // stale response
				if (!res?.valid) {
					preview = { runs: [], error: res?.error ?? 'Invalid expression', loading: false };
				} else {
					preview = { runs: res.nextRuns ?? [], error: '', loading: false };
				}
			} catch {
				if (seq !== previewSeq) return;
				preview = { runs: [], error: 'Could not validate expression', loading: false };
			}
		}, 300);
	}

	// Re-validate whenever the expression, timezone or editor visibility changes.
	$effect(() => {
		void form.expression;
		void form.timezone;
		if (showEditor) updatePreview();
	});

	// ── Save ─────────────────────────────────────────
	function buildConfig(): Record<string, unknown> {
		if (form.action === 'http') {
			const headers = Object.fromEntries(
				form.headers
					.filter((h) => h.key.trim() && h.value.trim())
					.map((h) => [h.key.trim(), h.value.trim()])
			);
			return {
				url: form.url.trim(),
				method: form.method,
				headers,
				...(form.body.trim() ? { body: form.body } : {})
			};
		}
		if (form.action === 'sql') {
			return { statement: form.statement };
		}
		return form.hookEvent.trim() ? { event: form.hookEvent.trim() } : {};
	}

	async function save() {
		formError = '';
		if (!form.name.trim()) {
			formError = 'Name is required';
			return;
		}
		if (!form.expression.trim()) {
			formError = 'Expression is required';
			return;
		}
		if (form.action === 'http' && !form.url.trim()) {
			formError = 'URL is required for HTTP requests';
			return;
		}
		if (form.action === 'sql' && !form.statement.trim()) {
			formError = 'SQL statement is required';
			return;
		}

		saving = true;
		try {
			const payload = {
				name: form.name.trim(),
				expression: form.expression.trim(),
				timezone: form.timezone,
				enabled: form.enabled,
				action: form.action,
				config: buildConfig()
			};
			if (editingId) {
				await client.http.patch(`/crons/${editingId}`, payload);
			} else {
				await client.http.post('/crons', payload);
			}
			showEditor = false;
			await refresh();
		} catch (e) {
			const res = (e as { response?: { data?: { error?: string } } }).response?.data;
			formError = res?.error ?? 'Failed to save cron job';
		} finally {
			saving = false;
		}
	}

	// ── Row actions ─────────────────────────────────
	async function toggleEnabled(job: CronJob) {
		busy = true;
		try {
			await client.http.patch(`/crons/${job.id}`, { enabled: !job.enabled });
			await refresh();
		} catch {
			// ignore — refresh on next action
		} finally {
			busy = false;
		}
	}

	async function runNow(job: CronJob) {
		busy = true;
		try {
			await client.http.post(`/crons/${job.id}`);
			await refresh();
		} catch {
			// ignore
		} finally {
			busy = false;
		}
	}

	async function remove(job: CronJob) {
		if (!confirm(`Delete cron job "${job.name}"?`)) return;
		busy = true;
		try {
			await client.http.delete(`/crons/${job.id}`);
			await refresh();
		} catch {
			// ignore
		} finally {
			busy = false;
		}
	}

	// ── Table ────────────────────────────────────────
	const columns = [
		{ key: 'name', label: 'Name', class: 'w-48' },
		{ key: 'expression', label: 'Expression' },
		{ key: 'timezone', label: 'Timezone', class: 'w-36' },
		{ key: 'next_run', label: 'Next run', class: 'w-44' },
		{ key: 'last_run', label: 'Last run', class: 'w-44' },
		{ key: 'status', label: 'Status', class: 'w-24' },
		{ key: 'actions', label: '', class: 'w-40 text-right' }
	];
</script>

<h2 class="mb-4 text-lg font-semibold">Cron Jobs</h2>
<div class="mb-4 text-sm text-base-content/60">
	<p>
		Schedule recurring jobs: HTTP webhooks, SQL statements, or custom Elixir hook events. Each job
		runs on its own timezone (wall-clock), executes in a background task, and records the last run
		outcome here. Cron expressions accept 5-field (<code>min hour day month weekday</code>) or
		6-field (with seconds) formats.
	</p>
</div>

<div class="mb-4 flex items-center gap-3">
	<Button class="btn-primary" onclick={openNew}>New cron job</Button>
	{#if busy}
		<span class="text-xs text-base-content/40">working…</span>
	{/if}
</div>

{#if error}
	<div class="mb-3 rounded-box border border-error/30 bg-error/10 p-3 text-xs text-error">
		{error}
	</div>
{/if}

<DataTable
	{columns}
	rows={jobs as unknown as Record<string, unknown>[]}
	emptyLabel="No cron jobs yet. Create your first one above."
>
	{#snippet cell(row, col)}
		{@const job = row as unknown as CronJob}
		{#if col.key === 'name'}
			<span class="font-medium">{job.name}</span>
		{:else if col.key === 'expression'}
			<span class="font-mono text-xs">{job.expression}</span>
			{#if !job.enabled}
				<span class="badge badge-neutral badge-xs ml-2">disabled</span>
			{/if}
		{:else if col.key === 'timezone'}
			<span class="text-xs">{job.timezone}</span>
		{:else if col.key === 'next_run'}
			<span class="text-xs">{job.enabled ? formatDate(job.next_run_at) : '—'}</span>
		{:else if col.key === 'last_run'}
			<span class="text-xs">
				{formatDate(job.last_run_at)}
				{#if job.last_duration_ms != null}
					<span class="text-base-content/40"> ({formatDuration(job.last_duration_ms)})</span>
				{/if}
			</span>
		{:else if col.key === 'status'}
			{#if job.last_status === 'ok'}
				<span class="badge badge-success badge-sm">ok</span>
			{:else if job.last_status === 'error'}
				<span class="badge badge-error badge-sm" title={job.last_error ?? ''}>error</span>
			{:else if job.last_status === 'skipped'}
				<span class="badge badge-neutral badge-sm" title={job.last_error ?? ''}>skipped</span>
			{:else}
				<span class="badge badge-ghost badge-sm">never</span>
			{/if}
		{:else if col.key === 'actions'}
			<div class="flex items-center justify-end gap-1.5">
				<input
					type="checkbox"
					class="toggle"
					checked={job.enabled}
					onchange={() => toggleEnabled(job)}
					title={job.enabled ? 'Disable' : 'Enable'}
				/>
				<button class="btn-icon" title="Run now" disabled={busy} onclick={() => runNow(job)}>
					<Play size={14} />
				</button>
				<button class="btn-icon" title="Edit" disabled={busy} onclick={() => openEdit(job)}>
					<Pencil size={14} />
				</button>
				<button
					class="btn-icon btn-icon-danger"
					title="Delete"
					disabled={busy}
					onclick={() => remove(job)}
				>
					<Trash2 size={14} />
				</button>
			</div>
		{:else}
			{col.render ? col.render(row) : String(row[col.key] ?? '—')}
		{/if}
	{/snippet}
</DataTable>

<Modal bind:show={showEditor} title={editingId ? 'Edit cron job' : 'New cron job'}>
	<div class="grid gap-4">
		<Input label="Name" placeholder="e.g. nightly cleanup" bind:value={form.name} />

		<div class="grid grid-cols-[1fr_220px] gap-3">
			<div>
				<span class="mb-1 block text-xs text-base-content/50">Cron expression</span>
				<input
					class="input w-full font-mono text-xs"
					placeholder="*/5 * * * *"
					bind:value={form.expression}
				/>
			</div>
			<div>
				<span class="mb-1 block text-xs text-base-content/50">Timezone</span>
				<Select options={timezoneOptions} bind:value={form.timezone} />
			</div>
		</div>

		{#if preview.loading}
			<div
				class="rounded-box border border-base-300 bg-base-200/40 p-3 text-xs text-base-content/50"
			>
				Checking expression…
			</div>
		{:else if preview.error}
			<div class="rounded-box border border-error/30 bg-error/10 p-3 text-xs text-error">
				{preview.error}
			</div>
		{:else if preview.runs.length}
			<div class="rounded-box border border-base-300 bg-base-200/40 p-3">
				<p class="mb-1 text-xs font-medium text-base-content/60">
					Next 5 runs (in {form.timezone}):
				</p>
				<ul class="space-y-0.5 font-mono text-xs">
					{#each preview.runs as run (run)}
						<li>{formatDate(run)}</li>
					{/each}
				</ul>
			</div>
		{/if}

		<div class="switch-field">
			<label class="switch-label" for="cron-enabled">
				<span class="txt">Enabled — schedule this job</span>
			</label>
			<label class="switch">
				<input id="cron-enabled" type="checkbox" bind:checked={form.enabled} />
				<span class="switch-slider"></span>
			</label>
		</div>

		<div>
			<span class="mb-1 block text-xs text-base-content/50">Action</span>
			<Tabs items={actionTabs} bind:active={actionTab} />
			{#if actionTab === 'HTTP Request'}
				<div class="mt-4 grid gap-3">
					<div class="grid grid-cols-[130px_1fr] gap-3">
						<div>
							<span class="mb-1 block text-xs text-base-content/50">Method</span>
							<Select options={httpMethodOptions} bind:value={form.method} />
						</div>
						<div>
							<span class="mb-1 block text-xs text-base-content/50">URL</span>
							<input
								class="input w-full font-mono text-xs"
								placeholder="https://example.com/webhook"
								bind:value={form.url}
							/>
						</div>
					</div>
					<div>
						<span class="mb-1 block text-xs text-base-content/50">Headers</span>
						<div class="overflow-hidden rounded-box border border-base-300">
							<div
								class="flex border-b border-base-300 bg-base-200/50 text-xs text-base-content/50"
							>
								<div class="w-1/2 px-3 py-1.5">Header</div>
								<div class="w-1/2 border-l border-base-300 px-3 py-1.5">Value</div>
								<div class="w-9 shrink-0"></div>
							</div>
							{#each form.headers as header, i (i)}
								<div class="flex items-center border-b border-base-300 last:border-b-0">
									<input
										class="w-1/2 bg-transparent px-3 py-1.5 font-mono text-xs outline-none focus:bg-base-200/40"
										placeholder="X-Custom-Header"
										bind:value={header.key}
									/>
									<input
										class="w-1/2 border-l border-base-300 bg-transparent px-3 py-1.5 font-mono text-xs outline-none focus:bg-base-200/40"
										placeholder="value"
										bind:value={header.value}
									/>
									<div class="flex w-9 shrink-0 justify-center">
										<button
											class="btn-icon btn-icon-danger"
											title="Remove header"
											onclick={() => form.headers.splice(i, 1)}
											disabled={form.headers.length <= 1}
										>
											×
										</button>
									</div>
								</div>
							{/each}
						</div>
						<button
							class="mt-1 cursor-pointer border-none bg-transparent text-xs text-primary hover:underline"
							onclick={() => form.headers.push({ key: '', value: '' })}
						>
							+ Add header
						</button>
					</div>
					<div>
						<span class="mb-1 block text-xs text-base-content/50">Body (optional)</span>
						<textarea
							class="input w-full font-mono text-xs outline-none focus:outline-none"
							rows="4"
							placeholder={'{"message": "hello"}'}
							bind:value={form.body}></textarea>
					</div>
				</div>
			{:else if actionTab === 'SQL'}
				<div class="mt-4 grid gap-3">
					<div>
						<span class="mb-1 block text-xs text-base-content/50">SQL statement</span>
						<textarea
							class="input w-full font-mono text-xs outline-none focus:outline-none"
							rows="6"
							placeholder="DELETE FROM logs WHERE created_at < now() - interval '30 days'"
							bind:value={form.statement}></textarea>
					</div>
					<p class="text-xs text-base-content/50">
						Runs against your database with full privileges (superuser-owned). Useful for cleanup,
						archival and backfills.
					</p>
				</div>
			{:else}
				<div class="mt-4 grid gap-3">
					<Input
						label="Event name (optional)"
						placeholder="e.g. night_cleanup"
						help="Subscribe in priv/hooks/*.ex via Lazypock.Hooks.Cron.on_cron/1."
						bind:value={form.hookEvent}
					/>
					<p class="text-xs text-base-content/50">
						The on_cron hook event fires with the job attached; hook modules can run arbitrary
						Elixir — the escape hatch for anything the other actions can't do.
					</p>
				</div>
			{/if}
		</div>

		{#if formError}
			<p class="text-xs text-error">{formError}</p>
		{/if}

		<div class="flex justify-end gap-2">
			<Button class="btn-outline" onclick={closeEditor}>Cancel</Button>
			<Button class="btn-primary" loading={saving} onclick={save}>
				{editingId ? 'Save changes' : 'Create job'}
			</Button>
		</div>
	</div>
</Modal>
