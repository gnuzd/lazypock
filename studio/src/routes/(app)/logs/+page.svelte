<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import Button from '$lib/components/Button.svelte';

	let logs = $state<Record<string, unknown>[]>([]);
	let loading = $state(false);
	let page = $state(1);
	let totalPages = $state(1);
	let totalItems = $state(0);
	let perPage = $state(50);
	let collectionFilter = $state('');
	let collections = $state<string[]>([]);
	let detailId = $state<string | null>(null);
	let detail = $state<Record<string, unknown> | null>(null);

	// Stats
	let statsTotal = $state(0);
	let statsLast24h = $state(0);
	let statsErrors24h = $state(0);
	let statsAvgDuration = $state(0);
	// Chart (created in onMount to avoid SSR window issues)
	let chartCanvas: HTMLCanvasElement | undefined = $state();
	let chartInst: any = $state();
	let chartData: { x: Date; y: number }[] = $state([]);
	let chartLoading = $state(false);
	let ChartRef: any = $state();

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

		await loadStats();
		await loadChart(Chart);
		loadCollections();
		loadLogs();
		ChartRef = Chart;
	});

	function initChart(Chart: any) {
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
						time: {
							unit: 'hour',
							tooltipFormat: 'DD h a'
						},
						grid: {
							color: (c: any) => (c.tick?.major ? '#edf0f3' : '')
						},
						border: { color: '#e4e9ec' },
						ticks: {
							maxTicksLimit: 15,
							autoSkip: true,
							maxRotation: 0,
							major: { enabled: true },
							color: (c: any) => (c.tick?.major ? '#16161a' : '#666f75')
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
				chartInst.data.datasets[0].data = chartData;
				chartInst.update('none');
			}
		});
	}

	async function loadStats() {
		try {
			const res = (await client.http.get('/logs/stats')) as Record<string, unknown> | null;
			statsTotal = (res?.total as number) ?? 0;
			statsLast24h = (res?.last_24h as number) ?? 0;
			statsErrors24h = (res?.errors_24h as number) ?? 0;
			statsAvgDuration = (res?.avg_duration as number) ?? 0;
		} catch {
			// ignore
		}
	}

	async function loadChart(Chart: any) {
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

	function formatTS(ts: unknown): string {
		if (!ts) return '—';
		const d = new Date(String(ts));
		return d.toLocaleString();
	}

	async function viewDetail(id: string) {
		detailId = id;
		detail = null;
		try {
			const res = (await client.http.get('/logs/' + id)) as Record<string, unknown> | null;
			detail = res;
		} catch {
			detail = {};
		}
	}

	async function clearLogs() {
		if (!confirm('Delete all request logs? This cannot be undone.')) return;
		try {
			await client.http.delete('/logs?all=true');
			loadStats();
			loadLogs();
			loadChart(ChartRef);
		} catch {
			// ignore
		}
	}

	async function clearOldLogs() {
		try {
			await client.http.delete('/logs');
			loadStats();
			loadLogs();
			loadChart(ChartRef);
		} catch {
			// ignore
		}
	}
</script>

<!-- Chart -->
<div class="mb-4 bg-primary">
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

<!-- Detail pane -->
{#if detailId}
	<div class="mb-4 rounded-box border border-base-300 bg-base-100 p-4">
		<div class="mb-3 flex items-center justify-between">
			<h2 class="font-semibold">Log Detail</h2>
			<button
				class="cursor-pointer border-none bg-transparent text-sm opacity-50 hover:opacity-100"
				onclick={() => (detailId = null)}
			>
				Close
			</button>
		</div>
		{#if detail}
			<div class="grid grid-cols-2 gap-3 text-sm">
				<div class="text-base-content/60">Method</div>
				<div class={methodClass(detail.method)}>{String(detail.method ?? '')}</div>
				<div class="text-base-content/60">Path</div>
				<div class="font-mono">{String(detail.path ?? '')}</div>
				<div class="text-base-content/60">Status</div>
				<div class={statusClass(detail.status)}>{String(detail.status ?? '')}</div>
				<div class="text-base-content/60">Duration</div>
				<div>{formatDuration(detail.duration)}</div>
				<div class="text-base-content/60">IP</div>
				<div class="font-mono">{String(detail.ip ?? '—')}</div>
				<div class="text-base-content/60">User Agent</div>
				<div class="break-all">{String(detail.user_agent ?? '—')}</div>
				<div class="text-base-content/60">Referer</div>
				<div class="break-all">{String(detail.referer ?? '—')}</div>
				<div class="text-base-content/60">Collection</div>
				<div>{String(detail.collection ?? '—')}</div>
				{#if detail.error}
					<div class="text-base-content/60">Error</div>
					<div class="text-error">{String(detail.error)}</div>
				{/if}
				<div class="text-base-content/60">Timestamp</div>
				<div>{formatTS(detail.created_at)}</div>
			</div>
		{:else}
			<div class="text-sm opacity-50">Loading...</div>
		{/if}
	</div>
{/if}

<!-- Logs table -->
<div class="overflow-x-auto rounded-box border border-base-300 bg-base-100">
	<table class="w-full border-collapse text-sm">
		<thead>
			<tr class="bg-base-200 text-xs font-semibold tracking-wider text-base-content/60 uppercase">
				<th class="border-b border-base-300 px-3.5 py-2.5 text-left">Method</th>
				<th class="border-b border-base-300 px-3.5 py-2.5 text-left">Path</th>
				<th class="border-b border-base-300 px-3.5 py-2.5 text-left">Status</th>
				<th class="border-b border-base-300 px-3.5 py-2.5 text-left">Duration</th>
				<th class="border-b border-base-300 px-3.5 py-2.5 text-left">IP</th>
				<th class="border-b border-base-300 px-3.5 py-2.5 text-left">Collection</th>
				<th class="border-b border-base-300 px-3.5 py-2.5 text-left">Timestamp</th>
			</tr>
		</thead>
		<tbody>
			{#if loading}
				<tr>
					<td colspan="7" class="py-8 text-center opacity-50">Loading...</td>
				</tr>
			{:else if logs.length === 0}
				<tr>
					<td colspan="7" class="py-8 text-center opacity-50">No request logs yet.</td>
				</tr>
			{:else}
				{#each logs as log (log.id)}
					<tr
						class="cursor-pointer transition-[background] duration-(--animation-speed-fast) hover:bg-base-200"
						onclick={() => viewDetail(log.id as string)}
					>
						<td class="border-b border-base-200 px-3.5 py-2">
							<span class={'font-mono text-xs font-semibold ' + methodClass(log.method)}
								>{String(log.method ?? '').toUpperCase()}</span
							>
						</td>
						<td class="max-w-60 truncate border-b border-base-200 px-3.5 py-2 font-mono text-xs">
							{String(log.path ?? '')}
						</td>
						<td class={'border-b border-base-200 px-3.5 py-2 font-mono ' + statusClass(log.status)}
							>{String(log.status ?? '')}</td
						>
						<td class="border-b border-base-200 px-3.5 py-2 text-xs text-base-content/60"
							>{formatDuration(log.duration)}</td
						>
						<td class="border-b border-base-200 px-3.5 py-2 font-mono text-xs"
							>{String(log.ip ?? '—')}</td
						>
						<td class="border-b border-base-200 px-3.5 py-2 text-xs"
							>{String(log.collection ?? '—')}</td
						>
						<td class="border-b border-base-200 px-3.5 py-2 text-xs text-base-content/60"
							>{formatTS(log.created_at)}</td
						>
					</tr>
				{/each}
			{/if}
		</tbody>
	</table>
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
		<Button class="btn-ghost btn-sm" disabled={page >= totalPages} onclick={() => goPage(page + 1)}
			>Next</Button
		>
	</div>
{/if}
