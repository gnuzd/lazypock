<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import type { Chart as ChartType } from 'chart.js';
	import Button from '$lib/components/Button.svelte';
	import DataTable from '$lib/components/DataTable.svelte';

	let logs = $state<Record<string, unknown>[]>([]);
	let loading = $state(false);
	let page = $state(1);
	let totalPages = $state(1);
	let totalItems = $state(0);
	let perPage = $state(50);
	let collectionFilter = $state('');
	let collections = $state<string[]>([]);
	// ── Inline row detail (accordion) ──
	let expandedId = $state<string | null>(null);
	let detailCache = $state<Record<string, Record<string, unknown> | null>>({});

	// Chart (created in onMount to avoid SSR window issues)
	let chartCanvas: HTMLCanvasElement | undefined = $state();
	let chartInst: ChartType | null = $state(null);
	let chartData: { x: Date; y: number }[] = $state([]);
	let chartLoading = $state(false);
	let ChartRef: typeof ChartType | null = $state(null);

	onMount(async () => {
		// Dynamically import chart.js only on client-side
		const [
			{ Chart, LineElement, PointElement, LineController, LinearScale, TimeScale, Filler, Tooltip },
			{ default: zoomPlugin }
		] = await Promise.all([
			import('chart.js'),
			import('chartjs-plugin-zoom'),
			import('chartjs-adapter-luxon')
		]);

		Chart.register(
			LineElement,
			PointElement,
			LineController,
			LinearScale,
			TimeScale,
			Filler,
			Tooltip
		);
		Chart.register(zoomPlugin);

		await loadChart(Chart);
		loadCollections();
		loadLogs();
		ChartRef = Chart;
	});

	function initChart(Chart: typeof ChartType) {
		if (!chartCanvas || chartInst) return;

		chartInst = new Chart(chartCanvas, {
			type: 'line',
			data: {
				datasets: [
					{
						label: 'Total requests',
						data: [],
						borderColor: '#e34562',
						pointBackgroundColor: '#e34562',
						backgroundColor: 'rgb(239,69,101,0.05)',
						borderWidth: 2,
						pointRadius: 1,
						pointBorderWidth: 0,
						fill: true
					}
				]
			},
			options: {
				resizeDelay: 250,
				maintainAspectRatio: false,
				animation: false,
				interaction: {
					intersect: false,
					mode: 'index'
				},
				scales: {
					y: {
						beginAtZero: true,
						grid: { color: '#edf0f3' },
						border: { color: '#e4e9ec' },
						ticks: {
							precision: 0,
							maxTicksLimit: 4,
							autoSkip: true,
							color: '#666f75'
						}
					},
					x: {
						type: 'time',
						min: Date.now() - 24 * 3600 * 1000,
						max: Date.now(),
						time: {
							unit: 'hour',
							tooltipFormat: 'DD h a'
						},
						grid: {
							color: (c: { tick?: { major?: boolean } }) => (c.tick?.major ? '#edf0f3' : '')
						},
						border: { color: '#e4e9ec' },
						ticks: {
							maxTicksLimit: 15,
							autoSkip: true,
							maxRotation: 0,
							major: { enabled: true },
							color: (c: { tick?: { major?: boolean } }) => (c.tick?.major ? '#16161a' : '#666f75')
						}
					}
				},
				plugins: {
					legend: { display: false },
					zoom: {
						pan: { enabled: true, mode: 'x' },
						zoom: {
							mode: 'x',
							pinch: { enabled: true },
							drag: {
								enabled: true,
								backgroundColor: 'rgba(255, 99, 132, 0.2)',
								borderWidth: 0,
								threshold: 10
							}
						}
					}
				}
			}
		});

		// Update chart data reactively
		$effect(() => {
			if (chartInst && chartData.length > 0) {
				chartInst.data.datasets[0].data = chartData as never;
				chartInst.update('none');
			}
		});
	}

	async function loadChart(Chart: typeof ChartType) {
		chartLoading = true;
		try {
			const res = (await client.http.get('/logs/stats')) as Record<string, unknown> | null;
			const hourly = (res?.hourly as { date: string; total: number }[]) ?? [];
			chartData = hourly.map((h) => ({
				x: new Date(h.date),
				y: h.total
			}));
			initChart(Chart);
		} catch {
			// ignore
		} finally {
			chartLoading = false;
		}
	}

	async function loadCollections() {
		try {
			const res = (await client.http.get('/logs/collections')) as Record<string, string[]> | null;
			collections = res?.items ?? [];
		} catch {
			// ignore
		}
	}

	async function loadLogs() {
		loading = true;
		try {
			let qs = `page=${page}&perPage=${perPage}`;
			if (collectionFilter) qs += `&collection=${encodeURIComponent(collectionFilter)}`;
			const res = (await client.http.get(`/logs?${qs}`)) as Record<string, unknown> | null;
			logs = (res?.items as Record<string, unknown>[]) ?? [];
			totalItems = (res?.totalItems as number) ?? 0;
			totalPages = (res?.totalPages as number) ?? 1;
		} catch {
			logs = [];
		} finally {
			loading = false;
		}
	}

	function goPage(p: number) {
		page = p;
		loadLogs();
	}

	function statusClass(status: unknown): string {
		const s = Number(status);
		if (s >= 200 && s < 300) return 'text-success';
		if (s >= 300 && s < 400) return 'text-warning';
		if (s >= 400) return 'text-error';
		return '';
	}

	function methodClass(method: unknown): string {
		const m = String(method).toUpperCase();
		if (m === 'GET') return 'text-info';
		if (m === 'POST') return 'text-success';
		if (m === 'PATCH' || m === 'PUT') return 'text-warning';
		if (m === 'DELETE') return 'text-error';
		return '';
	}

	function formatDuration(ms: unknown): string {
		const d = Number(ms);
		if (d < 1000) return d + 'ms';
		return (d / 1000).toFixed(2) + 's';
	}

	let columns = $derived([
		{ key: 'method', label: 'Method' },
		{ key: 'path', label: 'Path' },
		{ key: 'status', label: 'Status' },
		{ key: 'duration', label: 'Duration' },
		{ key: 'ip', label: 'IP' },
		{ key: 'collection', label: 'Collection' },
		{ key: 'created_at', label: 'Timestamp' }
	]);

	function formatTS(ts: unknown): string {
		if (!ts) return '—';
		const d = new Date(String(ts));
		return d.toLocaleString();
	}

	function prettyJSON(body: unknown): string {
		if (!body) return '';
		try {
			const parsed = typeof body === 'string' ? JSON.parse(body) : body;
			return JSON.stringify(parsed, null, 2);
		} catch {
			return String(body);
		}
	}

	function methodLabel(method: unknown): string {
		return String(method ?? '').toUpperCase();
	}

	// Fetch the full detail for the expanded row (lazily, cached per id).
	$effect(() => {
		const id = expandedId;
		if (!id || detailCache[id] !== undefined) return;
		detailCache[id] = null; // in-flight marker
		client.http
			.get('/logs/' + id)
			.then((res) => {
				detailCache[id] = (res as Record<string, unknown> | null) ?? {};
			})
			.catch(() => {
				detailCache[id] = {};
			});
	});

	async function clearLogs() {
		if (!confirm('Delete all request logs? This cannot be undone.')) return;
		try {
			await client.http.delete('/logs?all=true');
			loadLogs();
			if (ChartRef) loadChart(ChartRef);
		} catch {
			// ignore
		}
	}

	async function clearOldLogs() {
		try {
			await client.http.delete('/logs');
			loadLogs();
			if (ChartRef) loadChart(ChartRef);
		} catch {
			// ignore
		}
	}
</script>

<div class="flex flex-1 flex-col overflow-hidden">
	<!-- Chart -->
	<div class="mb-4 shrink-0 bg-primary">
		<div class="relative h-[170px] w-full" class:opacity-50={chartLoading}>
			{#if chartLoading}
				<div class="absolute inset-0 z-50 flex items-center justify-center">
					<span class="loading loading-spinner loading-sm" />
				</div>
			{/if}
			<canvas
				bind:this={chartCanvas}
				class="h-full w-full"
				ondblclick={() => chartInst?.resetZoom()}
			/>
		</div>
	</div>

	<div class="flex min-h-0 flex-1 flex-col overflow-hidden p-4">
		<!-- Filters bar -->
		<div class="mb-4 flex items-center justify-between gap-2 p-4">
			<h1 class="text-xl font-semibold">Request Logs</h1>
			<div class="flex items-center gap-2">
				{#if collections.length > 0}
					<select
						class="input input-sm"
						bind:value={collectionFilter}
						onchange={() => {
							page = 1;
							loadLogs();
						}}
					>
						<option value="">All collections</option>
						{#each collections as coll (coll)}
							<option value={coll}>{coll}</option>
						{/each}
					</select>
				{/if}
				<Button class="btn-ghost btn-sm" onclick={clearOldLogs}>Clean old (7d+)</Button>
				<Button class="btn-primary btn-sm" onclick={clearLogs}>Clear all</Button>
			</div>
		</div>

		<!-- Inline row detail (accordion under the clicked row) -->
		{#snippet rowDetail(row: Record<string, unknown>)}
			{@const id = row.id as string}
			{@const d = detailCache[id]}
			<div class="text-sm">
				{#if d === undefined || d === null}
					<div class="py-2 opacity-50">Loading detail...</div>
				{:else}
					<div class="grid grid-cols-2 gap-3">
						<div class="text-base-content/60">Method</div>
						<div class={methodClass(d.method)}>{String(d.method ?? '')}</div>
						<div class="text-base-content/60">Path</div>
						<div class="font-mono break-all">{String(d.path ?? '')}</div>
						<div class="text-base-content/60">Status</div>
						<div class={statusClass(d.status)}>{String(d.status ?? '')}</div>
						<div class="text-base-content/60">Duration</div>
						<div>{formatDuration(d.duration)}</div>
						<div class="text-base-content/60">IP</div>
						<div class="font-mono">{String(d.ip ?? '—')}</div>
						<div class="text-base-content/60">User Agent</div>
						<div class="break-all">{String(d.user_agent ?? '—')}</div>
						<div class="text-base-content/60">Referer</div>
						<div class="break-all">{String(d.referer ?? '—')}</div>
						<div class="text-base-content/60">Collection</div>
						<div>{String(d.collection ?? '—')}</div>
						{#if d.error}
							<div class="text-base-content/60">Error</div>
							<div class="text-error">{String(d.error)}</div>
						{/if}
						<div class="text-base-content/60">Timestamp</div>
						<div>{formatTS(d.created_at)}</div>
						{#if d.body}
							<div class="col-span-2 mt-2">
								<div class="mb-1 text-base-content/60">Request Body</div>
								<pre
									class="max-h-64 overflow-auto rounded-lg border border-base-300 bg-base-200/50 p-3 font-mono text-xs break-all whitespace-pre-wrap">{prettyJSON(
										d.body
									)}</pre>
							</div>
						{/if}
					</div>
				{/if}
			</div>
		{/snippet}

		<!-- Logs table -->
		<div class="min-h-0">
			<DataTable
				fillHeight
				{columns}
				rows={logs}
				{loading}
				emptyLabel="No request logs yet."
				bind:expandedId
				detail={rowDetail}
			>
				{#snippet cell(row, col)}
					{#if col.key === 'method'}
						<span class={'font-mono text-xs font-semibold ' + methodClass(row.method)}
							>{methodLabel(row.method)}</span
						>
					{:else if col.key === 'path'}
						<span class="font-mono text-xs">{String(row.path ?? '')}</span>
					{:else if col.key === 'status'}
						<span class={'font-mono ' + statusClass(row.status)}>{String(row.status ?? '')}</span>
					{:else if col.key === 'duration'}
						<span class="text-xs text-base-content/60">{formatDuration(row.duration)}</span>
					{:else if col.key === 'ip'}
						<span class="font-mono text-xs">{String(row.ip ?? '—')}</span>
					{:else if col.key === 'collection'}
						<span class="text-xs">{String(row.collection ?? '—')}</span>
					{:else if col.key === 'created_at'}
						<span class="text-xs text-base-content/60">{formatTS(row.created_at)}</span>
					{:else}
						{col.render ? col.render(row) : ((row[col.key] as string) ?? '—')}
					{/if}
				{/snippet}
			</DataTable>
		</div>

		<!-- Pagination -->
		{#if totalPages > 1}
			<div class="mt-3 flex items-center justify-center gap-2">
				<Button class="btn-ghost btn-sm" disabled={page <= 1} onclick={() => goPage(page - 1)}
					>Previous</Button
				>
				<span class="text-sm text-base-content/60">
					Page {page} of {totalPages} ({totalItems} total)
				</span>
				<Button
					class="btn-ghost btn-sm"
					disabled={page >= totalPages}
					onclick={() => goPage(page + 1)}>Next</Button
				>
			</div>
		{/if}
	</div>
</div>
