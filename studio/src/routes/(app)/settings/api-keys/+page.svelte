<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import Button from '$lib/components/Button.svelte';
	import DataTable from '$lib/components/DataTable.svelte';
	import '../settings.css';

	type ApiKey = {
		id: string;
		created_at: string | null;
		expires_at: string | null;
		revoked: boolean;
	};

	let keys = $state<ApiKey[]>([]);
	let freshlyGenerated = $state('');
	let busy = $state(false);
	let expires = $state('0');

	onMount(refresh);

	async function refresh() {
		try {
			const res = (await client.http.get('/settings/api-keys')) as { items?: ApiKey[] } | null;
			keys = res?.items ?? [];
		} catch {
			keys = [];
		}
	}

	async function generate() {
		busy = true;
		try {
			const days = Number(expires);
			const body = days > 0 ? { expiresInDays: days } : {};
			const res = (await client.http.post('/settings/api-keys', body)) as {
				api_key?: string;
				items?: ApiKey[];
			} | null;
			if (res?.api_key) {
				freshlyGenerated = res.api_key;
				navigator.clipboard?.writeText(res.api_key).catch(() => {});
			}
			await refresh();
		} catch {
			// ignore
		} finally {
			busy = false;
		}
	}

	async function revoke(id: string) {
		busy = true;
		try {
			await client.http.delete(`/settings/api-keys/${id}`);
			await refresh();
		} catch {
			// ignore
		} finally {
			busy = false;
		}
	}

	function statusOf(key: ApiKey): string {
		if (key.revoked) return 'revoked';
		if (key.expires_at) {
			const exp = new Date(key.expires_at);
			if (exp.getTime() < Date.now()) return 'expired';
		}
		return 'active';
	}

	function formatDate(iso: string | null): string {
		if (!iso) return 'Never';
		try {
			return new Date(iso).toLocaleString();
		} catch {
			return iso;
		}
	}

	const expiryOptions = [
		{ value: '0', label: 'Never expires' },
		{ value: '7', label: '7 days' },
		{ value: '30', label: '30 days' },
		{ value: '90', label: '90 days' }
	];

	const columns = [
		{
			key: 'id',
			label: 'ID',
			class: 'w-40',
			render: (r: Record<string, unknown>) => String(r.id ?? '').slice(0, 8) + '…'
		},
		{
			key: 'created_at',
			label: 'Created',
			render: (r: Record<string, unknown>) => formatDate(r.created_at as string | null)
		},
		{
			key: 'expires_at',
			label: 'Expires',
			render: (r: Record<string, unknown>) => formatDate(r.expires_at as string | null)
		},
		{ key: 'status', label: 'Status', class: 'w-28' },
		{ key: 'actions', label: '', class: 'w-24 text-right' }
	];
</script>

<h2 class="mb-4 text-lg font-semibold">API Keys</h2>
<div class="mb-4 text-sm text-base-content/60">
	<p>
		API keys let you authenticate programmatic access to Lazypock without a browser session — for
		example running the TypeScript codegen CLI (<code>npx lazypock --url ... --apikey ...</code>).
	</p>
	<p class="mt-1">
		Keys are stored as a SHA-256 hash (the raw key is shown only once, when generated). Each key has
		an optional expiry and can be revoked individually. Treat them like passwords.
	</p>
</div>

{#if freshlyGenerated}
	<div class="mb-4 rounded border border-success/40 bg-success/10 p-4 text-sm">
		<div class="mb-2 font-semibold text-success">
			Your new API key — copy it now, it won't be shown again.
		</div>
		<div class="flex items-center gap-2">
			<code class="rounded bg-base-200 px-3 py-2 break-all select-all">{freshlyGenerated}</code>
			<Button
				class="btn-outline btn-sm"
				onclick={() => navigator.clipboard?.writeText(freshlyGenerated)}
			>
				Copy
			</Button>
		</div>
	</div>
{/if}

<div class="mb-4 flex items-end gap-3">
	<div>
		<label for="apikey-expiry" class="mb-1 block text-xs text-base-content/50">Expiry</label>
		<select id="apikey-expiry" class="select select-sm select-bordered" bind:value={expires}>
			{#each expiryOptions as opt (opt.value)}
				<option value={opt.value}>{opt.label}</option>
			{/each}
		</select>
	</div>
	<Button class="btn-primary" loading={busy} onclick={generate}>Generate API key</Button>
</div>

<DataTable
	{columns}
	rows={keys as unknown as Record<string, unknown>[]}
	loading={busy}
	emptyLabel="No API keys yet. Generate your first one above."
>
	{#snippet cell(row, col)}
		{#if col.key === 'id'}
			<span class="font-mono text-xs">{String(row.id ?? '').slice(0, 8)}…</span>
		{:else if col.key === 'created_at'}
			<span class="text-xs text-base-content/60">{formatDate(row.created_at as string | null)}</span
			>
		{:else if col.key === 'expires_at'}
			<span class="text-xs">{formatDate(row.expires_at as string | null)}</span>
		{:else if col.key === 'status'}
			{#if statusOf(row as unknown as ApiKey) === 'active'}
				<span class="badge badge-success badge-sm">active</span>
			{:else if statusOf(row as unknown as ApiKey) === 'revoked'}
				<span class="badge badge-neutral badge-sm">revoked</span>
			{:else}
				<span class="badge badge-warning badge-sm">expired</span>
			{/if}
		{:else if col.key === 'actions'}
			{#if statusOf(row as unknown as ApiKey) === 'active'}
				<Button
					class="btn-error btn-outline btn-xs"
					loading={busy}
					onclick={() => revoke(row.id as string)}
				>
					Revoke
				</Button>
			{/if}
		{:else}
			{col.render ? col.render(row) : ((row[col.key] as string) ?? '—')}
		{/if}
	{/snippet}
</DataTable>
