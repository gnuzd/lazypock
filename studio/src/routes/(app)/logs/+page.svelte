<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import type { Chart as ChartType } from 'chart.js';
	import Button from '$lib/components/Button.svelte';
	import DataTable from '$lib/components/DataTable.svelte';
	import Select from '$lib/components/Select.svelte';

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

	// Chart (created in onMount to avoid SSR window issues)
	let chartCanvas: HTMLCanvasElement | undefined = $state();
	let chartInst: ChartType | null = $state(null);
	let range = $state('24h');
	let chartSeries = $state<{ date: string; total: number; errors: number; avg_duration: number }[]>(
		[]
	);
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
		if (!chartCanvas) return;
		if (chartInst) {
			chartInst.destroy();
			chartInst = null;
		}

		const pts = (key: 'total' | 'errors' | 'avg_duration') =>
			chartSeries.map((s) => ({ x: new Date(s.date), y: s[key] }));

		chartInst = new Chart(chartCanvas, {
			type: 'line',
			data: {
				datasets: [
					{
						label: 'Requests',
						data: pts('total') as never,
						borderColor: '#38bdf8',
						pointBackgroundColor: '#38bdf8',
						backgroundColor: 'rgba(56, 189, 248, 0.08)',
						borderWidth: 2,
						pointRadius: 1,
						pointBorderWidth: 0,
						fill: true,
						tension: 0.3,
						yAxisID: 'y'
					},
					{
						label: 'Errors',
						data: pts('errors') as never,
						borderColor: '#f43f5e',
						pointBackgroundColor: '#f43f5e',
						backgroundColor: 'rgba(244, 63, 94, 0.06)',
						borderWidth: 2,
						pointRadius: 1,
						pointBorderWidth: 0,
						fill: false,
						tension: 0.3,
						yAxisID: 'y'
					},
					{
						label: 'Avg duration',
						data: pts('avg_duration') as never,
						borderColor: '#f59e0b',
						pointBackgroundColor: '#f59e0b',
						borderWidth: 2,
						pointRadius: 0,
						pointBorderWidth: 0,
						fill: false,
						tension: 0.3,
						yAxisID: 'y1'
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
						grid: { color: 'rgba(255,255,255,0.12)' },
						border: { color: 'rgba(255,255,255,0.2)' },
						ticks: {
							precision: 0,
							maxTicksLimit: 4,
							autoSkip: true,
							color: 'rgba(255,255,255,0.75)'
						}
					},
					y1: {
						beginAtZero: true,
						position: 'right',
						grid: { drawOnChartArea: false },
						border: { color: 'rgba(255,255,255,0.2)' },
						ticks: {
							maxTicksLimit: 4,
							autoSkip: true,
							color: 'rgba(255,255,255,0.75)'
						},
						title: {
							display: true,
							text: 'avg (ms)',
							color: 'rgba(255,255,255,0.6)'
						}
					},
					x: {
						type: 'time',
						min: Date.now() - (range === '7d' ? 7 : range === '30d' ? 30 : 1) * 86400000,
						max: Date.now(),
						time: {
							unit: range === '30d' ? 'day' : 'hour',
							tooltipFormat: range === '30d' ? 'MMM d' : 'MMM d HH:mm'
						},
						grid: {
							color: (c: { tick?: { major?: boolean } }) =>
								c.tick?.major ? 'rgba(255,255,255,0.25)' : 'rgba(255,255,255,0.08)'
						},
						border: { color: 'rgba(255,255,255,0.2)' },
						ticks: {
							maxTicksLimit: 15,
							autoSkip: true,
							maxRotation: 0,
							major: { enabled: true },
							color: (c: { tick?: { major?: boolean } }) =>
								c.tick?.major ? '#ffffff' : 'rgba(255,255,255,0.7)'
						}
					}
				},
				plugins: {
					legend: {
						display: true,
						position: 'top',
						labels: {
							color: 'rgba(255,255,255,0.9)',
							usePointStyle: true,
							boxWidth: 8
						}
					},
					tooltip: {
						callbacks: {
							label: (ctx) => {
								const v = ctx.parsed.y ?? 0;
								if (ctx.datasetIndex === 2) {
									const s = v < 1000 ? `${v.toFixed(0)}ms` : `${(v / 1000).toFixed(2)}s`;
									return `Avg duration: ${s}`;
								}
								return `${ctx.dataset.label}: ${v}`;
							}
						}
					},
					zoom: {
						pan: { enabled: false },
						zoom: {
							mode: 'x',
							pinch: { enabled: false },
							wheel: { enabled: false },
							drag: { enabled: false }
						}
					}
				}
			}
		});
	}

	async function loadChart(Chart: typeof ChartType, forRange: string = range) {
		chartLoading = true;
		try {
			const res = (await client.http.get(`/logs/stats?range=${forRange}`)) as Record<
				string,
				unknown
			> | null;
			chartSeries = ((res?.series as typeof chartSeries) ?? []).map((s) => ({
				...s,
				avg_duration: Number(s.avg_duration)
			}));
			initChart(Chart);
		} catch {
			// ignore
		} finally {
			chartLoading = false;
		}
	}

	// Rebuild the chart for a new range (axis bounds/units change with range).
	function onRangeChange() {
		if (ChartRef) loadChart(ChartRef);
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

	function changePerPage() {
		page = 1;
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
	<div class="mb-4 shrink-0 bg-primary text-neutral-content">
		<div class="relative h-[180px] w-full" class:opacity-50={chartLoading}>
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
				<Select
					options={[
						{ value: '24h', label: 'Last 24 hours' },
						{ value: '7d', label: 'Last 7 days' },
						{ value: '30d', label: 'Last 30 days' }
					]}
					bind:value={range}
					onchange={onRangeChange}
				/>
				{#if collections.length > 0}
					<Select
						options={[
							{ value: '', label: 'All collections' },
							...collections.map((c) => ({ value: c, label: c }))
						]}
						bind:value={collectionFilter}
						onchange={() => {
							page = 1;
							loadLogs();
						}}
					/>
				{/if}
				<Button class="btn-ghost" onclick={clearOldLogs}>Clean old (7d+)</Button>
				<Button class="btn-primary" onclick={clearLogs}>Clear all</Button>
			</div>
		</div>

		<!-- Inline row detail (accordion under the clicked row) -->
		{#snippet rowDetail(row: Record<string, unknown>)}
			<div class="text-sm">
				<div class="grid grid-cols-2 gap-3">
					<div class="text-base-content/60">Method</div>
					<div class={methodClass(row.method)}>{String(row.method ?? '')}</div>
					<div class="text-base-content/60">Path</div>
					<div class="font-mono break-all">{String(row.path ?? '')}</div>
					<div class="text-base-content/60">Status</div>
					<div class={statusClass(row.status)}>{String(row.status ?? '')}</div>
					<div class="text-base-content/60">Duration</div>
					<div>{formatDuration(row.duration)}</div>
					<div class="text-base-content/60">IP</div>
					<div class="font-mono">{String(row.ip ?? '—')}</div>
					<div class="text-base-content/60">User Agent</div>
					<div class="break-all">{String(row.user_agent ?? '—')}</div>
					<div class="text-base-content/60">Referer</div>
					<div class="break-all">{String(row.referer ?? '—')}</div>
					<div class="text-base-content/60">Collection</div>
					<div>{String(row.collection ?? '—')}</div>
					{#if row.error}
						<div class="text-base-content/60">Error</div>
						<div class="text-error">{String(row.error)}</div>
					{/if}
					<div class="text-base-content/60">Timestamp</div>
					<div>{formatTS(row.created_at)}</div>
					{#if row.body}
						<div class="col-span-2 mt-2">
							<div class="mb-1 text-base-content/60">Request Body</div>
							<pre
								class="max-h-64 overflow-auto rounded-lg border border-base-300 bg-base-200/50 p-3 font-mono text-xs break-all whitespace-pre-wrap">{prettyJSON(
									row.body
								)}</pre>
						</div>
					{/if}
				</div>
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
			<div class="mt-3 flex items-center justify-between gap-3">
				<div class="text-sm text-base-content/60">
					{totalItems} log{totalItems === 1 ? '' : 's'}
				</div>
				<div class="flex items-center gap-2">
					<Select
						options={[
							{ value: 10, label: '10 / page' },
							{ value: 25, label: '25 / page' },
							{ value: 50, label: '50 / page' },
							{ value: 100, label: '100 / page' }
						]}
						bind:value={perPage}
						onchange={changePerPage}
					/>
					<Button class="btn-ghost btn-sm" disabled={page <= 1} onclick={() => goPage(page - 1)}
						>Previous</Button
					>
					<span class="text-sm text-base-content/60">Page {page} of {totalPages}</span>
					<Button
						class="btn-ghost btn-sm"
						disabled={page >= totalPages}
						onclick={() => goPage(page + 1)}>Next</Button
					>
				</div>
			</div>
		{/if}
	</div>
</div>
