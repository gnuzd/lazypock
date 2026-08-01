<script lang="ts">
	import { client } from '$lib/client';
	import Button from '$lib/components/Button.svelte';
	import DataTable from '$lib/components/DataTable.svelte';

	let sqlQuery = $state('SELECT name, type FROM _collections ORDER BY name');
	let sqlResults = $state<{ columns: string[]; rows: unknown[][] } | null>(null);
	let sqlError = $state('');
	let sqlRunning = $state(false);

	let tableColumns = $derived((sqlResults?.columns ?? []).map((c) => ({ key: c, label: c })));
	let tableRows = $derived(
		(sqlResults?.rows ?? []).map((r, i) => {
			const obj: Record<string, unknown> = { __index: i };
			sqlResults?.columns.forEach((c, j) => {
				obj[c] = r[j];
			});
			return obj;
		})
	);

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
</script>

<h2 class="mb-4 text-lg font-semibold">SQL Console</h2>
<div class="rounded-box border border-base-300 bg-base-100 p-6">
	<p class="mb-3 text-xs text-base-content/60">
		Run read-only SQL queries against the database. Only SELECT, EXPLAIN, and WITH statements are
		allowed.
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
	<div class="mt-3">
		<DataTable columns={tableColumns} rows={tableRows} zebra emptyLabel="No rows returned.">
			{#snippet cell(row, col)}
				{#if row[col.key] == null}
					<span class="opacity-50">NULL</span>
				{:else}
					{String(row[col.key])}
				{/if}
			{/snippet}
		</DataTable>
	</div>
{/if}
